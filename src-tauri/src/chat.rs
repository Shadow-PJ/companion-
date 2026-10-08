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
    pub running_agent: String,
    pub reply: String,
    pub activity: String,
    pub error: Option<String>,
    /// What you asked (or the quick action's name), shown above the reply.
    pub title: String,
    /// Full last prompt, kept in memory so a failed request can be edited and retried.
    pub question: String,
    /// Files you dropped on Glowby, sent with the next message.
    pub attachments: Vec<PathBuf>,
    /// The running request may only read: the chat gate blocks file edits.
    pub read_only: bool,
    /// Chatting with a squad pet: talk to (a copy of) that session instead.
    pub target: Option<crate::squad::ChatTarget>,
    cancel: Option<oneshot::Sender<()>>,
}

/// One message to Claude Code from the chat box, a quick action, or an offer.
pub struct ChatRequest {
    pub message: String,
    pub title: String,
    pub read_only: bool,
    /// Run in this project folder instead of the chat's usual one.
    pub dir: Option<String>,
    /// Typed in the chat box: goes to the squad pet you're chatting with, if any.
    /// (Quick actions, offers and the briefing always use the normal chat.)
    pub use_target: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentView {
    pub name: String,
    pub path: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatView {
    pub agent: &'static str,
    pub reply_agent: Option<&'static str>,
    pub agent_mode: String,
    pub enabled: bool,
    pub busy: bool,
    pub reply: String,
    pub activity: String,
    pub error: Option<String>,
    pub title: String,
    pub attachments: Vec<AttachmentView>,
    pub question: String,
    pub auth_required: bool,
    pub project: String,
    pub project_path: String,
    /// "chosen" (from Settings), "recent" (your last Claude Code session) or "none".
    pub folder_source: &'static str,
    pub has_project: bool,
    pub has_conversation: bool,
    /// The squad pet you're chatting with, if any.
    pub squad_name: Option<String>,
}

/// Files dropped on Glowby wait here until you send a message.
pub fn attach(app: &AppHandle, paths: Vec<PathBuf>) {
    let state = app.state::<AppState>();
    let mut chat = lock(&state.chat);
    for path in paths {
        if !chat.attachments.contains(&path) && chat.attachments.len() < 10 {
            chat.attachments.push(path);
        }
    }
    chat.reply.clear();
    chat.error = None;
    chat.title.clear();
    chat.question.clear();
}

pub fn remove_attachment(app: &AppHandle, index: usize) {
    let state = app.state::<AppState>();
    let mut chat = lock(&state.chat);
    if index < chat.attachments.len() {
        chat.attachments.remove(index);
    }
}

/// The prompt text for attachments, and the extra folders Claude may read
/// (`--add-dir`) for files that live outside the project.
fn attachment_context(attachments: &[PathBuf], project_dir: &str) -> (String, Vec<PathBuf>) {
    if attachments.is_empty() {
        return (String::new(), Vec::new());
    }
    let project = project_dir.to_lowercase();
    let mut text = String::from("\n\nFiles I'm sharing with you:\n");
    let mut dirs: Vec<PathBuf> = Vec::new();
    for path in attachments {
        let is_dir = path.is_dir();
        text.push_str(&format!("- {}{}\n", path.display(), if is_dir { " (folder)" } else { "" }));
        let dir = if is_dir { Some(path.clone()) } else { path.parent().map(Path::to_path_buf) };
        if let Some(dir) = dir
            && !dir.to_string_lossy().to_lowercase().starts_with(&project)
            && !dirs.contains(&dir)
        {
            dirs.push(dir);
        }
    }
    (text, dirs)
}

/// The folder the chat works in, picked automatically so you don't have to:
/// 1. the one you chose in Settings (if you did),
/// 2. the folder of the Claude Code or Codex session that's active right now,
/// 3. the last folder any session worked in (remembered across restarts),
/// 4. your most recent project.
pub fn effective_dir(state: &AppState, settings: &Settings) -> (String, &'static str) {
    let chosen = settings.chat.project_dir.trim();
    if !chosen.is_empty() && Path::new(chosen).is_dir() {
        return (project_dir(chosen), "chosen");
    }
    if let Some(dir) = lock(&state.tracker).latest_project_dir().filter(|d| Path::new(d).is_dir()) {
        return (project_dir(&dir), "recent");
    }
    match lock(&state.projects).remembered_dir(|d| Path::new(d).is_dir()) {
        Some(dir) => (project_dir(&dir), "recent"),
        None => (String::new(), "none"),
    }
}

/// A selected .git directory is repository metadata, not the chat workspace.
pub fn project_dir(dir: &str) -> String {
    let path = Path::new(dir.trim_end_matches(['/', '\\']));
    if path.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(".git")) {
        return path.parent().filter(|parent| parent.is_dir())
            .map(|parent| parent.to_string_lossy().into_owned()).unwrap_or_else(|| dir.into());
    }
    dir.into()
}

pub fn effective_agent(state: &AppState, settings: &Settings, target: Option<&str>) -> &'static str {
    if let Some(id) = target { return lock(&state.tracker).session_agent(id).unwrap_or_else(|| if crate::squad::is_claude_session(id) { "claude" } else { "codex" }); }
    match settings.chat.agent.as_str() { "codex" => "codex", "claude" => "claude", _ => lock(&state.tracker).latest_agent() }
}

fn session_key(agent: &str, dir: &str) -> String { if agent == "codex" { format!("codex:{dir}") } else { dir.to_string() } }

pub fn view(state: &AppState, settings: &Settings) -> ChatView {
    let target_id = lock(&state.chat).target.as_ref().map(|t| t.session_id.clone());
    let active = effective_agent(state, settings, target_id.as_deref());
    let target = lock(&state.chat).target.as_ref().map(|t| (t.dir.clone(), t.name.clone(), t.session_id.clone()));
    let (dir, source, has_conversation, squad_name) = match target {
        Some((dir, name, id)) => {
            let forked = lock(&state.squad).members.get(&id).is_some_and(|m| !m.fork.is_empty());
            (dir, "squad", forked, Some(name))
        }
        None => {
            let (dir, source) = effective_dir(state, settings);
            let has = lock(&state.chat_sessions).contains_key(&session_key(active, &dir));
            (dir, source, has, None)
        }
    };
    let chat = lock(&state.chat);
    let agent = if chat.busy { if chat.running_agent == "codex" { "codex" } else { "claude" } } else { active };
    ChatView {
        agent,
        reply_agent: (!chat.running_agent.is_empty()).then_some(if chat.running_agent == "codex" { "codex" } else { "claude" }),
        agent_mode: settings.chat.agent.clone(),
        enabled: settings.chat.enabled,
        busy: chat.busy,
        reply: chat.reply.clone(),
        activity: chat.activity.clone(),
        error: chat.error.clone(),
        title: chat.title.clone(),
        question: chat.question.clone(),
        auth_required: chat.running_agent == "claude" && chat.error.as_deref().is_some_and(is_claude_auth_error),
        attachments: chat
            .attachments
            .iter()
            .map(|p| AttachmentView {
                name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()),
                path: p.display().to_string(),
            })
            .collect(),
        project: crate::sessions::project_name(&dir),
        has_project: !dir.is_empty(),
        project_path: dir,
        folder_source: source,
        has_conversation,
        squad_name,
    }
}

/// Chat with a squad pet: your messages go to a copy of its session
/// (`--resume <id> --fork-session`), so Claude knows what that session did,
/// but the session still running in your terminal is never touched.
pub fn set_target(app: &AppHandle, target: Option<crate::squad::ChatTarget>) {
    let state = app.state::<AppState>();
    let mut chat = lock(&state.chat);
    let same = match (&chat.target, &target) {
        (Some(a), Some(b)) => a.session_id == b.session_id,
        (None, None) => true,
        _ => false,
    };
    if !same && !chat.busy {
        chat.reply.clear();
        chat.error = None;
        chat.title.clear();
        chat.question.clear();
    }
    chat.target = target;
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

/// Sends a request and waits for the reply. Problems are shown in the chat bubble.
pub async fn send(app: AppHandle, request: ChatRequest) -> Result<(), String> {
    let result = send_inner(&app, request).await;
    if let Err(e) = &result {
        let state = app.state::<AppState>();
        let mut chat = lock(&state.chat);
        if !chat.busy {
            chat.error = Some(e.clone());
        }
        drop(chat);
        state::publish(&app);
    }
    result
}

async fn send_inner(app: &AppHandle, request: ChatRequest) -> Result<(), String> {
    let app = app.clone();
    let state = app.state::<AppState>();
    let settings = state.settings();
    if !settings.chat.enabled {
        return Err("Chat is turned off in Settings.".into());
    }
    let requested_target = request.use_target.then(|| lock(&state.chat).target.as_ref().map(|target| target.session_id.clone())).flatten();
    let requested_agent = effective_agent(&state, &settings, requested_target.as_deref());
    {
        let mut chat = lock(&state.chat);
        if chat.busy {
            return Err("Still waiting for the last reply.".into());
        }
        chat.question = crate::sessions::shorten(request.message.trim(), MAX_REPLY_CHARS);
        chat.title = crate::sessions::shorten(&request.title, 90);
        chat.running_agent = requested_agent.into();
        chat.reply.clear();
        chat.error = None;
    }
    let target = if request.use_target {
        lock(&state.chat).target.as_ref().map(|t| (t.session_id.clone(), t.dir.clone()))
    } else {
        lock(&state.chat).target = None;
        None
    };
    let dir = match (&target, request.dir.clone().filter(|d| Path::new(d).is_dir())) {
        (Some((_, dir)), _) => dir.clone(),
        (None, Some(d)) => d,
        (None, None) => effective_dir(&state, &settings).0,
    };
    let dir = project_dir(&dir);
    if dir.is_empty() {
        return Err("Choose a project folder first (click the folder chip).".into());
    }
    let message = request.message.trim().to_string();
    if message.is_empty() {
        return Ok(());
    }
    let agent = effective_agent(&state, &settings, target.as_ref().map(|t| t.0.as_str()));
    let key = session_key(agent, &dir);
    let claude = if agent == "claude" { Some(find_claude(&settings.chat.claude_path).ok_or("Couldn't find Claude Code. Set its path in Settings → Chat.")?) } else { None };
    if agent == "codex" && crate::codex_chat::find_codex().is_none() { return Err("Couldn't find Codex. Install the Codex CLI or desktop app and sign in.".into()); }
    let chat_hooks = if agent == "claude" { Some(write_chat_hooks(&app)?) } else { None };
    // (session to resume, make a copy of it first?)
    let resume: Option<(String, bool)> = match &target {
        Some((session_id, _)) => {
            let fork = crate::squad::fork_of(&app, session_id);
            if settings.chat.keep_conversation && !fork.is_empty() {
                Some((fork, false))
            } else {
                Some((session_id.clone(), true))
            }
        }
        None if settings.chat.keep_conversation => lock(&state.chat_sessions).get(&key).cloned().map(|id| (id, false)),
        None => None,
    };

    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    let attachments = {
        let mut chat = lock(&state.chat);
        if chat.busy {
            return Err("Still waiting for the last reply.".into());
        }
        chat.running_agent = agent.into();
        chat.busy = true;
        chat.reply.clear();
        chat.error = None;
        chat.title = crate::sessions::shorten(&request.title, 90);
        chat.read_only = request.read_only;
        chat.activity = "Thinking…".into();
        chat.cancel = Some(cancel_tx);
        std::mem::take(&mut chat.attachments)
    };
    state::publish(&app);
    let (attachment_text, extra_dirs) = attachment_context(&attachments, &dir);
    let message = format!("{message}{attachment_text}");

    let outcome = if agent == "codex" {
        crate::codex_chat::run(&app, &dir, message, settings.chat.mode, request.read_only, resume, &mut cancel_rx).await
    } else {
    let mut cmd = tokio::process::Command::new(claude.as_ref().expect("Claude runner"));
    cmd.args(["-p", "--output-format", "stream-json", "--verbose", "--settings"]).arg(chat_hooks.as_ref().expect("Claude hooks"));
    match settings.chat.mode {
        ChatMode::Ask => {}
        ChatMode::ReadOnly => {
            cmd.args(["--permission-mode", "plan"]);
        }
        ChatMode::AcceptEdits if !request.read_only => {
            cmd.args(["--permission-mode", "acceptEdits"]);
        }
        ChatMode::AcceptEdits => {}
    }
    for extra in &extra_dirs {
        cmd.arg("--add-dir").arg(extra);
    }
    if let Some((id, fork)) = &resume {
        cmd.args(["--resume", id]);
        if *fork {
            cmd.arg("--fork-session");
        }
    }
    cmd.current_dir(&dir)
        .env(PET_CHAT_ENV, "1")
        .env_remove("ANTHROPIC_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .creation_flags(CREATE_NO_WINDOW);

    run_claude(&app, cmd, message, &mut cancel_rx).await
    }.map_err(|error| if agent == "claude" { friendly_claude_error(&error) } else { error });
    {
        let mut chat = lock(&state.chat);
        chat.busy = false;
        chat.read_only = false;
        chat.cancel = None;
        chat.activity.clear();
        match &outcome {
            // squad chats remember their copy on the squad pet (below)
            Ok(Some(session_id)) if settings.chat.keep_conversation && target.is_none() => {
                let mut sessions = lock(&state.chat_sessions);
                sessions.insert(key.clone(), session_id.clone());
                let _ = settings::save_json(&state.paths.chat_sessions_file, &*sessions);
            }
            Ok(_) => {}
            Err(e) => {
                if agent == "claude" && is_claude_auth_error(e) {
                    chat.reply.clear();
                }
                // Failed sends keep shared files available for the user's next attempt.
                for path in &attachments {
                    if !chat.attachments.contains(path) && chat.attachments.len() < 10 {
                        chat.attachments.push(path.clone());
                    }
                }
                chat.error = Some(e.clone());
            }
        }
    }
    if let (Ok(Some(copy)), Some((squad_session, _))) = (&outcome, &target)
        && settings.chat.keep_conversation
    {
        crate::squad::set_fork(&app, squad_session, copy);
    }
    crate::sounds::play(&app, if outcome.is_ok() { crate::sounds::Sound::Done } else { crate::sounds::Sound::Error });
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
                        if is_error && final_text.as_ref().is_none_or(|text| text.trim().is_empty()) {
                            final_text = event.get("errors").and_then(Value::as_array).map(|errors| {
                                errors.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")
                            }).filter(|text| !text.is_empty());
                        }
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
    if status.success() && !is_error {
        return Ok(session_id);
    }
    let detail = stderr_text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").trim().to_string();
    if is_claude_auth_error(&stderr_text) {
        return Err(friendly_claude_error(&stderr_text));
    }
    Err(format!("Claude Code stopped: {}", crate::sessions::shorten(&detail, 300)))
}

fn is_claude_auth_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("claude code sign-in required")
        || error.contains("failed to authenticate")
        || error.contains("authentication_error")
        || error.contains("not logged in")
        || error.contains("not signed in")
        || error.contains("please run /login")
        || (error.contains("oauth") && ["expired", "refresh", "invalid", "revoked"].iter().any(|word| error.contains(word)))
}

fn friendly_claude_error(error: &str) -> String {
    if is_claude_auth_error(error) {
        "Claude Code sign-in required. This CLI's login is missing or expired. Sign in, check the login, then retry your question. Connecting hooks does not sign you in.".into()
    } else {
        error.into()
    }
}

/// Only the user opens this interactive login; Glowby never reads its credentials.
pub fn sign_in_claude(app: &AppHandle) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    static LOGIN: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);
    let state = app.state::<AppState>();
    if lock(&state.chat).busy {
        return Err("Stop the current request before signing in.".into());
    }
    if lock(&state.ui).game_active {
        return Err("Leave fullscreen game mode before opening the sign-in window.".into());
    }
    let exe = find_claude(&state.settings().chat.claude_path).ok_or("Couldn't find Claude Code. Check its path in Settings → Chat.")?;
    let mut login = lock(&LOGIN);
    if let Some(child) = login.as_mut() && child.try_wait().map_err(|e| e.to_string())?.is_none() {
        return Err("A Claude sign-in window is already open. Finish or close that window first.".into());
    }
    let shell = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows folder is unavailable.")?)
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let child = std::process::Command::new(shell)
        .args(["-NoLogo", "-NoProfile", "-Command", "& $env:GLOWBY_CLAUDE_LOGIN_EXE auth login --claudeai; Read-Host 'Press Enter to close this sign-in window'"])
        // The executable path is data, never interpolated into shell code.
        .env("GLOWBY_CLAUDE_LOGIN_EXE", exe)
        .env_remove("ANTHROPIC_API_KEY")
        .current_dir(&state.paths.config_dir)
        .creation_flags(0x0000_0010) // CREATE_NEW_CONSOLE: intentional, user-clicked login.
        .spawn().map_err(|e| format!("Couldn't open Claude sign-in: {e}"))?;
    *login = Some(child);
    Ok(())
}

/// One bounded local status check, without a model request or background polling.
pub async fn check_claude_login(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let exe = find_claude(&state.settings().chat.claude_path).ok_or("Couldn't find Claude Code. Check its path in Settings → Chat.")?;
    let mut command = tokio::process::Command::new(exe);
    command.args(["auth", "status", "--json"])
        .env_remove("ANTHROPIC_API_KEY")
        .current_dir(&state.paths.config_dir)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .kill_on_drop(true).creation_flags(CREATE_NO_WINDOW);
    let output = tokio::time::timeout(std::time::Duration::from_secs(12), command.output())
        .await.map_err(|_| "Claude login check timed out.".to_string())?
        .map_err(|e| format!("Couldn't check Claude login: {e}"))?;
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|_| "Claude Code returned an unrecognized login status. Try claude auth status in your terminal.".to_string())?;
    let signed_in = value.get("loggedIn").and_then(Value::as_bool).ok_or("Claude Code didn't report whether it is signed in.")?;
    if signed_in {
        let mut chat = lock(&state.chat);
        if !chat.busy && chat.running_agent == "claude" && chat.error.as_deref().is_some_and(is_claude_auth_error) {
            chat.error = None;
        }
        drop(chat);
        state::publish(app);
    }
    Ok(signed_in)
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
    // With a squad pet: drop the copy, so the next message starts a fresh one.
    let squad_session = lock(&state.chat).target.as_ref().map(|t| t.session_id.clone());
    if let Some(id) = squad_session {
        crate::squad::set_fork(app, &id, "");
    } else {
        let (dir, _) = effective_dir(&state, &state.settings());
        let mut sessions = lock(&state.chat_sessions);
        let agent = effective_agent(&state, &state.settings(), None);
        sessions.remove(&session_key(agent, &dir));
        let _ = settings::save_json(&state.paths.chat_sessions_file, &*sessions);
    }
    {
        let mut chat = lock(&state.chat);
        if !chat.busy {
            chat.reply.clear();
            chat.error = None;
            chat.title.clear();
            chat.question.clear();
        }
    }
    state::publish(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_outside_the_project_get_add_dir() {
        let tmp = std::env::temp_dir();
        let inside = PathBuf::from(r"C:\code\app\src\main.rs");
        let outside = tmp.join("notes.txt");
        let (text, dirs) = attachment_context(&[inside.clone(), outside.clone()], r"C:\code\app");
        assert!(text.contains("main.rs") && text.contains("notes.txt"));
        assert_eq!(dirs, vec![tmp]);
    }
}
