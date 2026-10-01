//! The chat box: runs `claude -p` (Claude Code's non-interactive mode) in your
//! project folder with your normal Claude login, and streams the reply back.
//!
//! Safety notes:
//! * Your message goes in through stdin, never on the command line, so special
//!   characters can't be misread as extra arguments.
//! * We start claude.exe directly (not through cmd.exe), with no console window.
//! * ANTHROPIC_API_KEY is removed from the child's environment so your existing
//!   Claude login is used, as you asked (no API key).

use crate::hooks_installer;
use crate::sessions::describe_tool;
use crate::settings::{self, ChatMode, Settings};
use crate::state::{self, AppState, lock};
use crate::pet_window;
use glowby_protocol::{CHAT_GATE_EVENT, PET_CHAT_ENV};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_REPLY_CHARS: usize = 20_000;

#[derive(Default)]
pub struct ChatState {
    pub busy: bool,
    pub reply: String,
    pub activity: String,
    pub error: Option<String>,
    cancel: Option<oneshot::Sender<()>>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatView {
    pub enabled: bool,
    pub busy: bool,
    pub reply: String,
    pub activity: String,
    pub error: Option<String>,
    pub project: String,
    pub project_path: String,
    /// "chosen" (from Settings), "recent" (your last Claude Code session) or "none".
    pub folder_source: &'static str,
    pub has_project: bool,
    pub has_conversation: bool,
}

/// The folder the chat works in: the one you chose, otherwise the folder of
/// your most recent Claude Code session.
pub fn effective_dir(state: &AppState, settings: &Settings) -> (String, &'static str) {
    let chosen = settings.chat.project_dir.trim();
    if !chosen.is_empty() && Path::new(chosen).is_dir() {
        return (chosen.to_string(), "chosen");
    }
    match lock(&state.tracker).latest_project_dir() {
        Some(dir) if Path::new(&dir).is_dir() => (dir, "recent"),
        _ => (String::new(), "none"),
    }
}

pub fn view(state: &AppState, settings: &Settings) -> ChatView {
    let (dir, source) = effective_dir(state, settings);
    let has_conversation = lock(&state.chat_sessions).contains_key(&dir);
    let chat = lock(&state.chat);
    ChatView {
        enabled: settings.chat.enabled,
        busy: chat.busy,
        reply: chat.reply.clone(),
        activity: chat.activity.clone(),
        error: chat.error.clone(),
        project: crate::sessions::project_name(&dir),
        has_project: !dir.is_empty(),
        project_path: dir,
        folder_source: source,
        has_conversation,
    }
}

/// Finds the real claude.exe (not the npm .cmd shim).
pub fn find_claude(override_path: &str) -> Option<PathBuf> {
    if !override_path.trim().is_empty() {
        let p = PathBuf::from(override_path.trim());
        return p.is_file().then_some(p);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let exe = dir.join("claude.exe");
            if exe.is_file() {
                return Some(exe);
            }
            let shim = dir.join("claude.cmd");
            if shim.is_file()
                && let Some(exe) = exe_from_npm_shim(&shim)
            {
                return Some(exe);
            }
        }
    }
    let env_dir = |var: &str| std::env::var_os(var).map(PathBuf::from);
    [
        env_dir("APPDATA").map(|d| d.join(r"npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe")),
        env_dir("USERPROFILE").map(|d| d.join(r".local\bin\claude.exe")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

/// npm's claude.cmd contains a line like `"%dp0%\node_modules\...\claude.exe" %*`.
fn exe_from_npm_shim(shim: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(shim).ok()?;
    for line in text.lines() {
        if let Some(start) = line.find("%dp0%") {
            let rest = &line[start + 5..];
            let end = rest.to_ascii_lowercase().find(".exe")? + 4;
            let candidate = shim.parent()?.join(rest[..end].trim_start_matches(['\\', '/']));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Extra settings passed only to Glowby's own `claude -p`: a PreToolUse hook that
/// asks you on Glowby before the chat edits files or runs commands.
fn write_chat_hooks(app: &AppHandle) -> Result<PathBuf, String> {
    let hook = hooks_installer::ensure_hook_binary(app)?;
    let config = json!({
        "hooks": {
            "PreToolUse": [{
                "hooks": [{
                    "type": "command",
                    "command": hook.to_string_lossy(),
                    "args": [CHAT_GATE_EVENT],
                    "timeout": 600
                }]
            }]
        }
    });
    let path = app.state::<AppState>().paths.chat_settings_file.clone();
    settings::save_json(&path, &config).map_err(|e| format!("Couldn't write chat settings: {e}"))?;
    Ok(path)
}

pub async fn send(app: AppHandle, message: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    if !settings.chat.enabled {
        return Err("Chat is turned off in Settings.".into());
    }
    let (dir, _) = effective_dir(&state, &settings);
    if dir.is_empty() {
        return Err("Choose a project folder first (click the folder chip).".into());
    }
    let message = message.trim().to_string();
    if message.is_empty() {
        return Ok(());
    }
    let claude = find_claude(&settings.chat.claude_path)
        .ok_or("Couldn't find claude.exe. Set its path in Settings → Chat.")?;
    let chat_hooks = write_chat_hooks(&app)?;
    let resume = if settings.chat.keep_conversation { lock(&state.chat_sessions).get(&dir).cloned() } else { None };

    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    {
        let mut chat = lock(&state.chat);
        if chat.busy {
            return Err("Still waiting for the last reply.".into());
        }
        chat.busy = true;
        chat.reply.clear();
        chat.error = None;
        chat.activity = "Thinking…".into();
        chat.cancel = Some(cancel_tx);
    }
    state::publish(&app);

    let mut cmd = tokio::process::Command::new(&claude);
    cmd.args(["-p", "--output-format", "stream-json", "--verbose", "--settings"]).arg(&chat_hooks);
    match settings.chat.mode {
        ChatMode::Ask => {}
        ChatMode::ReadOnly => {
            cmd.args(["--permission-mode", "plan"]);
        }
        ChatMode::AcceptEdits => {
            cmd.args(["--permission-mode", "acceptEdits"]);
        }
    }
    if let Some(id) = &resume {
        cmd.args(["--resume", id]);
    }
    cmd.current_dir(&dir)
        .env(PET_CHAT_ENV, "1")
        .env_remove("ANTHROPIC_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .creation_flags(CREATE_NO_WINDOW);

    let outcome = run_claude(&app, cmd, message, &mut cancel_rx).await;
    {
        let mut chat = lock(&state.chat);
        chat.busy = false;
        chat.cancel = None;
        chat.activity.clear();
        match &outcome {
            Ok(Some(session_id)) if settings.chat.keep_conversation => {
                let mut sessions = lock(&state.chat_sessions);
                sessions.insert(dir.clone(), session_id.clone());
                let _ = settings::save_json(&state.paths.chat_sessions_file, &*sessions);
            }
            Ok(_) => {}
            Err(e) => chat.error = Some(e.clone()),
        }
    }
    // If you closed the chat while waiting, pop out to show the reply.
    let chat_open = lock(&state.ui).chat_open;
    if !chat_open {
        let text = match &outcome {
            Ok(_) => "Your chat reply is ready. Click me to read it.".to_string(),
            Err(e) => e.clone(),
        };
        state::toast(&app, "info", text, crate::sessions::project_name(&dir), 8);
        pet_window::show(&app);
    }
    state::publish(&app);
    outcome.map(|_| ())
}

/// Runs claude and parses its stream-json output. Returns the session id.
async fn run_claude(
    app: &AppHandle,
    mut cmd: tokio::process::Command,
    message: String,
    cancel: &mut oneshot::Receiver<()>,
) -> Result<Option<String>, String> {
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Claude Code: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(message.as_bytes()).await.map_err(|e| e.to_string())?;
        drop(stdin); // closing stdin tells claude the message is complete
    }
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_string(&mut buf).await;
        buf
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut session_id = None;
    let mut final_text: Option<String> = None;
    let mut is_error = false;
    loop {
        tokio::select! {
            _ = &mut *cancel => {
                let _ = child.kill().await;
                return Err("Stopped.".into());
            }
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
                if let Some(id) = event.get("session_id").and_then(Value::as_str) {
                    session_id = Some(id.to_string());
                }
                match event.get("type").and_then(Value::as_str) {
                    Some("assistant") => on_assistant_message(app, &event),
                    Some("result") => {
                        is_error = event.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                        final_text = event.get("result").and_then(Value::as_str).map(str::to_string);
                    }
                    _ => {}
                }
            }
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let stderr_text = stderr_task.await.unwrap_or_default();

    let state = app.state::<AppState>();
    if let Some(text) = final_text {
        if is_error {
            return Err(crate::sessions::shorten(&text, 400));
        }
        lock(&state.chat).reply = crate::sessions::shorten(&text, MAX_REPLY_CHARS);
        return Ok(session_id);
    }
    if status.success() {
        return Ok(session_id);
    }
    let detail = stderr_text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").trim().to_string();
    Err(format!("Claude Code stopped: {}", crate::sessions::shorten(&detail, 300)))
}

/// Streams text and "using tool X" updates into the chat bubble.
fn on_assistant_message(app: &AppHandle, event: &Value) {
    let Some(blocks) = event.pointer("/message/content").and_then(Value::as_array) else { return };
    let state = app.state::<AppState>();
    {
        let mut chat = lock(&state.chat);
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        if !chat.reply.is_empty() {
                            chat.reply.push_str("\n\n");
                        }
                        chat.reply.push_str(text);
                        if chat.reply.len() > MAX_REPLY_CHARS {
                            chat.reply = crate::sessions::shorten(&chat.reply, MAX_REPLY_CHARS);
                        }
                    }
                }
                Some("tool_use") => {
                    let fake = json!({ "tool_name": block.get("name"), "tool_input": block.get("input") });
                    chat.activity = describe_tool(&fake);
                }
                _ => {}
            }
        }
    }
    state::publish(app);
}

/// Hook events from the chat's own session also update the "what is it doing" line.
pub fn on_hook_activity(app: &AppHandle, event: &str, payload: &Value) {
    if event == "PreToolUse" {
        let state = app.state::<AppState>();
        let mut chat = lock(&state.chat);
        if chat.busy {
            chat.activity = describe_tool(payload);
        }
    }
}

pub fn cancel(app: &AppHandle) {
    if let Some(tx) = lock(&app.state::<AppState>().chat).cancel.take() {
        let _ = tx.send(());
    }
}

/// "New chat": forget the conversation for the current project folder.
pub fn new_conversation(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (dir, _) = effective_dir(&state, &state.settings());
    {
        let mut sessions = lock(&state.chat_sessions);
        sessions.remove(&dir);
        let _ = settings::save_json(&state.paths.chat_sessions_file, &*sessions);
    }
    {
        let mut chat = lock(&state.chat);
        if !chat.busy {
            chat.reply.clear();
            chat.error = None;
        }
    }
    state::publish(app);
}
