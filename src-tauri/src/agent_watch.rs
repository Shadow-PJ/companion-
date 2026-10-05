//! File-change events keep usage fresh even when desktop sessions don't emit hooks.
//! Reads only bounded local log tails. No polling, no transcript text is persisted.
use crate::state::{self, AppState, lock};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tauri::{AppHandle, Manager};

pub struct LogWatcher {
    _watcher: Mutex<RecommendedWatcher>,
}
#[derive(Clone, Default)]
struct Meta {
    id: String,
    cwd: String,
    agent: String,
}

fn metadata(path: &Path) -> Meta {
    use std::io::Read;
    let agent = if path
        .to_string_lossy()
        .replace('\\', "/")
        .contains("/.codex/")
    {
        "codex"
    } else {
        "claude"
    };
    let mut bytes = Vec::new();
    if let Ok(f) = std::fs::File::open(path) {
        let _ = f.take(64 * 1024).read_to_end(&mut bytes);
    }
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            let p = if v.get("type").and_then(Value::as_str) == Some("session_meta") {
                v.get("payload").unwrap_or(&v)
            } else {
                &v
            };
            let id = p
                .get("id")
                .or_else(|| p.get("sessionId"))
                .or_else(|| p.get("session_id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let cwd = p.get("cwd").and_then(Value::as_str).unwrap_or("");
            if !id.is_empty() && !cwd.is_empty() {
                return Meta {
                    id: id.into(),
                    cwd: cwd.into(),
                    agent: agent.into(),
                };
            }
        }
    }
    Meta {
        id: path
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default(),
        agent: agent.into(),
        ..Default::default()
    }
}

pub fn spawn(app: &AppHandle) {
    let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
        return;
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let Ok(mut watcher) =
        notify::recommended_watcher(move |result: Result<notify::Event, notify::Error>| {
            if let Ok(event) = result
                && matches!(
                    event.kind,
                    notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
                        | notify::EventKind::Create(_)
                )
            {
                for path in event.paths {
                    if path.extension().is_some_and(|x| x == "jsonl") {
                        let _ = tx.send(path);
                    }
                }
            }
        })
    else {
        crate::applog::line("Could not watch agent logs; hook updates still work.");
        return;
    };
    for dir in [home.join(".codex/sessions"), home.join(".claude/projects")] {
        if dir.is_dir()
            && let Err(e) = watcher.watch(&dir, RecursiveMode::Recursive)
        {
            crate::applog::line(format!("Agent log watcher: {e}"));
        }
    }
    app.manage(LogWatcher {
        _watcher: Mutex::new(watcher),
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut meta: HashMap<PathBuf, Meta> = HashMap::new();
        let mut seen: HashMap<PathBuf, u64> = HashMap::new();
        while let Some(first) = rx.recv().await {
            let mut paths = HashSet::from([first]);
            tokio::time::sleep(Duration::from_millis(1800)).await;
            while let Ok(path) = rx.try_recv() {
                paths.insert(path);
            }
            let settings = app.state::<AppState>().settings();
            if !settings.limits.enabled && !settings.chat.enabled {
                continue;
            }
            for path in paths.into_iter().take(30) {
                let Ok(file) = std::fs::metadata(&path) else {
                    continue;
                };
                let len = file.len();
                if seen.get(&path) == Some(&len) {
                    continue;
                }
                seen.insert(path.clone(), len);
                let info = meta.entry(path.clone()).or_insert_with(|| metadata(&path));
                if info.cwd.is_empty() {
                    *info = metadata(&path);
                }
                let info = info.clone();
                let text = crate::limits::tail_of(&path, 256 * 1024);
                if info.agent == "codex" {
                    crate::limits::on_codex_log(&app, &text);
                } else {
                    crate::limits::refresh_from_claude_log(&app);
                }
                let event = activity(&text, &info.agent);
                if let Some(event) = event
                    && !info.id.is_empty()
                    && !info.cwd.is_empty()
                {
                    let payload = json!({"session_id":info.id,"cwd":info.cwd,"_glowby_agent":info.agent,"transcript_path":path.to_string_lossy()});
                    // Existing hook sessions are authoritative. Log fallback only tracks a session
                    // when no recent hook has updated it; no XP/quests are awarded twice.
                    let state = app.state::<AppState>();
                    lock(&state.tracker).apply_log(event, &payload);
                    state::publish(&app);
                }
            }
            if meta.len() > 200 {
                meta.clear();
                seen.clear();
            }
        }
    });
}

fn activity(text: &str, agent: &str) -> Option<&'static str> {
    for line in text.lines().rev().take(30) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if agent == "codex" {
            if v.get("type").and_then(Value::as_str) == Some("session_meta") { return Some("SessionStart"); }
            match v.pointer("/payload/type").and_then(Value::as_str) {
                Some("task_complete" | "turn_complete") => return Some("Stop"),
                Some("task_started" | "user_message") => return Some("UserPromptSubmit"),
                Some("exec_command_begin" | "agent_message" | "token_count") => {
                    return Some("PreToolUse");
                }
                _ => {}
            }
        } else {
            match v.get("type").and_then(Value::as_str) {
                Some("user") => return Some("UserPromptSubmit"),
                Some("assistant") => {
                    if v.pointer("/message/stop_reason").and_then(Value::as_str) == Some("end_turn")
                    {
                        return Some("Stop");
                    }
                    return Some("PreToolUse");
                }
                _ => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_activity_skips_unknown_lines() {
        assert_eq!(
            activity(
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"}}\nfuture",
                "codex"
            ),
            Some("Stop")
        );
        assert_eq!(
            activity("{\"type\":\"user\"}", "claude"),
            Some("UserPromptSubmit")
        );
        assert_eq!(activity("garbage", "codex"), None);
    }
}
