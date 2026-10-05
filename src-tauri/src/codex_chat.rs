//! Codex chat through its real CLI, with the existing login and a bounded sandbox.
use crate::settings::ChatMode;
use crate::state::{self, AppState, lock};
use serde_json::Value;
use std::{path::PathBuf, process::Stdio};
use tauri::{AppHandle, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::oneshot,
};

pub fn find_codex() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let exe = dir.join("codex.exe");
            if exe.is_file() {
                return Some(exe);
            }
            for arch in ["x86_64", "aarch64"] {
                for prefix in [
                    "node_modules/@openai/codex",
                    "node_modules/@openai/codex/node_modules/@openai/codex-win32-x64",
                    "node_modules/@openai/codex-win32-x64",
                ] {
                    let exe = dir
                        .join(prefix)
                        .join(format!("vendor/{arch}-pc-windows-msvc/codex/codex.exe"));
                    if exe.is_file() {
                        return Some(exe);
                    }
                }
            }
        }
    }
    // Codex desktop's CLI distribution, even when its path wasn't inherited at startup.
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("OpenAI/Codex/bin");
    let mut versions: Vec<_> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let exe = e.path().join("codex.exe");
            exe.is_file()
                .then(|| (e.metadata().and_then(|m| m.modified()).ok(), exe))
        })
        .collect();
    versions.sort_by_key(|a| std::cmp::Reverse(a.0));
    versions.into_iter().next().map(|(_, p)| p)
}

/// Parsed separately so stream variations can be tested without spending AI usage.
pub enum Update {
    Session(String),
    Text(String),
    Activity(String),
    Error(String),
    Completed,
    Ignore,
}
pub fn parse_event(v: &Value) -> Update {
    let text = |path| {
        v.pointer(path)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    match text("/type").as_str() {
        "thread.started" => Update::Session(text("/thread_id")),
        "item.completed" if text("/item/type") == "agent_message" => {
            Update::Text(text("/item/text"))
        }
        "item.started" => Update::Activity(match text("/item/type").as_str() {
            "command_execution" => crate::sessions::shorten(&text("/item/command"), 100),
            "file_change" => "Changing files…".into(),
            "web_search" => "Searching…".into(),
            "mcp_tool_call" => "Using a tool…".into(),
            _ => "Working…".into(),
        }),
        "turn.completed" => Update::Completed,
        "turn.failed" => Update::Error(text("/error/message")),
        "error" => Update::Error(text("/message")),
        _ => Update::Ignore,
    }
}

pub async fn run(
    app: &AppHandle,
    dir: &str,
    message: String,
    mode: ChatMode,
    read_only: bool,
    resume: Option<(String, bool)>,
    cancel: &mut oneshot::Receiver<()>,
) -> Result<Option<String>, String> {
    let exe = find_codex()
        .ok_or("Couldn't find Codex. Install the Codex CLI or desktop app and sign in.")?;
    let mut cmd = tokio::process::Command::new(exe);
    // Non-interactive Ask uses read-only. Accept edits is an explicit workspace-write choice.
    // Never bypass sandboxing or hook trust. No prompt is passed through a shell.
    let sandbox = if read_only || mode != ChatMode::AcceptEdits {
        "read-only"
    } else {
        "workspace-write"
    };
    cmd.args([
        "-a",
        "on-request",
        "-s",
        sandbox,
        "exec",
        "--json",
        "--color",
        "never",
        "--skip-git-repo-check",
    ]);
    if let Some((id, fork)) = resume {
        cmd.arg(if fork { "fork" } else { "resume" }).arg(id);
    }
    cmd.arg("-")
        .current_dir(dir)
        .env(glowby_protocol::PET_CHAT_ENV, "1")
        .env_remove("OPENAI_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .creation_flags(0x0800_0000);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't start Codex: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(message.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        drop(stdin);
    }
    let stdout = child.stdout.take().ok_or("Codex stdout unavailable")?;
    let mut stderr = child.stderr.take().ok_or("Codex stderr unavailable")?;
    let stderr_task = tokio::spawn(async move {
        let mut s = String::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_string(&mut s).await;
        s
    });
    let mut lines = BufReader::new(stdout).lines();
    let mut session = None;
    let mut error = None;
    loop {
        tokio::select! {
            _ = &mut *cancel => {let _=child.kill().await;return Err("Stopped.".into());},
            line=lines.next_line() => {
                let Ok(Some(line))=line else{break};let Ok(event)=serde_json::from_str::<Value>(&line) else{continue};
                match parse_event(&event) {
                    Update::Session(id)=>session=Some(id),
                    Update::Error(e)=>error=Some(e),
                    Update::Completed=>error=None,
                    Update::Text(text)=>{let state=app.state::<AppState>();let mut chat=lock(&state.chat);if !chat.reply.is_empty(){chat.reply.push_str("\n\n");}chat.reply.push_str(&text);chat.reply=crate::sessions::shorten(&chat.reply,20000);drop(chat);state::publish(app);},
                    Update::Activity(text)=>{lock(&app.state::<AppState>().chat).activity=text;state::publish(app);},
                    Update::Ignore=>{},
                }
            }
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let stderr = stderr_task.await.unwrap_or_default();
    if let Some(e) = error {
        return Err(crate::sessions::shorten(&e, 500));
    }
    if !status.success() {
        return Err(format!(
            "Codex stopped: {}",
            crate::sessions::shorten(
                stderr
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("Check your login, hook trust or permissions in Codex."),
                400
            )
        ));
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn stream_events_are_tolerant_and_keep_agent_text() {
        assert!(
            matches!(parse_event(&json!({"type":"thread.started","thread_id":"s"})),Update::Session(s) if s=="s")
        );
        assert!(
            matches!(parse_event(&json!({"type":"item.completed","item":{"type":"agent_message","text":"Done"}})),Update::Text(s) if s=="Done")
        );
        assert!(
            matches!(parse_event(&json!({"type":"turn.failed","error":{"message":"Limit reached"}})),Update::Error(s) if s=="Limit reached")
        );
        assert!(matches!(
            parse_event(&json!({"type":"future"})),
            Update::Ignore
        ));
    }
}
