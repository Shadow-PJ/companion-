//! Local log events fill in activity and public replies when desktop hooks are absent.
//! Bounded reads; no polling, AI calls, persisted transcripts, or private reasoning.
use crate::state::{self, AppState, lock};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::{Value, json};
use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}, sync::Mutex, time::{Duration, SystemTime}};
use tauri::{AppHandle, Manager};

pub struct LogWatcher { _watcher: Mutex<RecommendedWatcher> }
#[derive(Clone)]
struct Root { path: PathBuf, agent: &'static str }
#[derive(Clone, Default)]
struct Meta { id: String, cwd: String, hidden: bool }
#[derive(Default)]
struct Snapshot {
    event: Option<&'static str>,
    at: i64,
    tool: String,
    command: String,
    reply: Option<(String, i64)>,
}

fn roots() -> Vec<Root> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        roots.push(Root { path: home.join(".codex/sessions"), agent: "codex" });
        roots.push(Root { path: home.join(".claude/projects"), agent: "claude" });
    }
    for (key, child, agent) in [("CODEX_HOME", "sessions", "codex"), ("CLAUDE_CONFIG_DIR", "projects", "claude")] {
        if let Some(root) = std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from) {
            let path = root.join(child);
            if !roots.iter().any(|r| r.path == path) { roots.push(Root { path, agent }); }
        }
    }
    roots
}

pub fn claude_roots() -> Vec<PathBuf> {
    roots().into_iter().filter(|r| r.agent == "claude").map(|r| r.path).collect()
}

fn timestamp(v: &Value) -> i64 {
    v.get("timestamp").and_then(Value::as_str)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map_or(0, |t| t.timestamp_millis())
}

fn public_text(value: &Value) -> String {
    match value {
        Value::String(s) => crate::sessions::shorten(s, 20_000),
        Value::Array(blocks) => {
            let mut text = String::new();
            for block in blocks {
                let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
                let part = if block.is_string() { block.as_str() }
                    else if matches!(kind, "text" | "output_text") { block.get("text").and_then(Value::as_str) }
                    else { None };
                if let Some(part) = part {
                    if !text.is_empty() { text.push('\n'); }
                    text.extend(part.chars().take(20_000usize.saturating_sub(text.chars().count())));
                    if text.chars().count() >= 20_000 { break; }
                }
            }
            text
        }
        _ => String::new(),
    }
}

fn metadata(path: &Path, tail: &str, agent: &str) -> Meta {
    use std::io::Read;
    let mut head = Vec::new();
    if let Ok(file) = std::fs::File::open(path) { let _ = file.take(128 * 1024).read_to_end(&mut head); }
    let head = String::from_utf8_lossy(&head);
    let mut meta = Meta::default();
    for line in head.lines().chain(tail.lines().rev()) {
        let Ok(v) = serde_json::from_str::<Value>(line.trim_start_matches('\u{feff}')) else { continue };
        let session_meta = v.get("type").and_then(Value::as_str) == Some("session_meta");
        let p = if session_meta { v.get("payload").unwrap_or(&v) } else { &v };
        let id = if agent == "codex" && session_meta { p.get("id").or_else(|| p.get("session_id")) }
            else { p.get("sessionId").or_else(|| p.get("session_id")) };
        if meta.id.is_empty() { meta.id = id.and_then(Value::as_str).unwrap_or("").into(); }
        if meta.cwd.is_empty() { meta.cwd = p.get("cwd").and_then(Value::as_str).map(crate::chat::project_dir).unwrap_or_default(); }
        meta.hidden |= v.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || p.pointer("/source/subagent").is_some();
        if !meta.id.is_empty() && !meta.cwd.is_empty() { break; }
    }
    if meta.id.is_empty() { meta.id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(); }
    meta
}

fn snapshot(text: &str, agent: &str) -> Snapshot {
    let mut out = Snapshot::default();
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) { continue; }
        let at = timestamp(&v);
        let outer = v.get("type").and_then(Value::as_str).unwrap_or("");
        let mut event = None;
        let mut reply = String::new();
        let mut tool = String::new();
        let mut command = String::new();
        if agent == "codex" {
            let p = v.get("payload").unwrap_or(&v);
            let kind = p.get("type").and_then(Value::as_str).unwrap_or("");
            match (outer, kind) {
                ("event_msg", "task_complete" | "turn_complete") => {
                    event = Some("Stop");
                    reply = p.get("last_agent_message").map(public_text).unwrap_or_default();
                }
                ("event_msg", "task_started" | "user_message") => event = Some("UserPromptSubmit"),
                ("event_msg", "turn_aborted" | "task_interrupted") => event = Some("Stop"),
                ("event_msg", "agent_message") => {
                    if p.get("phase").and_then(Value::as_str) == Some("final") || out.event == Some("Stop") {
                        event = Some("Stop");
                        reply = p.get("message").or_else(|| p.get("text")).map(public_text).unwrap_or_default();
                    } else { event = Some("PreToolUse"); }
                }
                ("response_item", "message") if p.get("role").and_then(Value::as_str) == Some("assistant") => {
                    if p.get("phase").and_then(Value::as_str) == Some("final")
                        || (p.get("phase").is_none() && out.event == Some("Stop")) {
                        event = Some("Stop");
                        reply = p.get("content").map(public_text).unwrap_or_default();
                    } else { event = Some("PreToolUse"); }
                }
                ("event_msg", "item_started" | "item_completed") => {
                    let item = p.get("item").unwrap_or(&Value::Null);
                    match item.get("type").and_then(Value::as_str).unwrap_or("") {
                        "AgentMessage" | "agent_message" if item.get("phase").and_then(Value::as_str) == Some("final") => {
                            event = Some("Stop"); reply = item.get("text").or_else(|| item.get("content")).map(public_text).unwrap_or_default();
                        }
                        "UserMessage" | "user_message" => event = Some("UserPromptSubmit"),
                        "CommandExecution" | "command_execution" => {
                            event = Some("PreToolUse"); tool = "Bash".into();
                            command = match item.get("command") {
                                Some(Value::String(s)) => s.clone(),
                                Some(Value::Array(parts)) => parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
                                _ => String::new(),
                            };
                        }
                        "FileChange" | "file_change" => { event = Some("PreToolUse"); tool = "Edit".into(); }
                        "McpToolCall" | "mcp_tool_call" | "WebSearch" | "web_search" => event = Some("PreToolUse"),
                        _ => {}
                    }
                }
                ("event_msg", "exec_command_begin") => { event = Some("PreToolUse"); tool = "Bash".into(); }
                ("response_item", "function_call" | "custom_tool_call") => {
                    event = Some("PreToolUse"); tool = p.get("name").and_then(Value::as_str).unwrap_or("a tool").into();
                }
                _ => {}
            }
        } else {
            match outer {
                "user" if v.get("isMeta").and_then(Value::as_bool) != Some(true) && v.get("isCompactSummary").and_then(Value::as_bool) != Some(true) => {
                    let result = v.pointer("/message/content").and_then(Value::as_array).is_some_and(|blocks| blocks.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")));
                    event = Some(if result { "PreToolUse" } else { "UserPromptSubmit" });
                }
                "assistant" => {
                    let content = v.pointer("/message/content").unwrap_or(&Value::Null);
                    let answer = public_text(content);
                    if v.pointer("/message/stop_reason").and_then(Value::as_str) == Some("end_turn") && !answer.is_empty() {
                        event = Some("Stop"); reply = answer;
                    } else if let Some(block) = content.as_array().and_then(|blocks| blocks.iter().find(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))) {
                        event = Some("PreToolUse"); tool = block.get("name").and_then(Value::as_str).unwrap_or("a tool").into();
                        command = block.pointer("/input/command").and_then(Value::as_str).unwrap_or("").into();
                    } else if !answer.is_empty() { event = Some("PreToolUse"); }
                }
                "system" if matches!(v.get("subtype").and_then(Value::as_str), Some("turn_duration" | "stop_hook_summary")) => event = Some("Stop"),
                _ => {}
            }
        }
        if out.event.is_none() && event.is_some() {
            out.event = event; out.at = at; out.tool = tool; out.command = command;
        }
        if out.reply.is_none() && !reply.trim().is_empty() { out.reply = Some((reply, at)); }
        if out.event.is_some() && out.reply.is_some() { break; }
    }
    out
}

fn recent_files(roots: &[Root]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs: Vec<_> = roots.iter().map(|r| r.path.clone()).collect();
    let since = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
    let mut visited = 0;
    while let Some(dir) = dirs.pop() {
        visited += 1; if visited > 1024 { break; }
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() && entry.file_name() != "subagents" { dirs.push(path); }
            else if path.extension().is_some_and(|e| e == "jsonl") && meta.modified().is_ok_and(|m| m >= since) {
                files.push((meta.modified().ok(), path));
            }
        }
    }
    files.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    files.into_iter().take(12).map(|(_, path)| path).collect()
}

pub fn spawn(app: &AppHandle) {
    let roots = roots();
    let watched_roots = roots.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let startup_tx = tx.clone();
    let Ok(mut watcher) = notify::recommended_watcher(move |result: Result<notify::Event, notify::Error>| {
        if let Ok(event) = result && matches!(event.kind, notify::EventKind::Modify(_) | notify::EventKind::Create(_)) {
            for path in event.paths {
                if path.extension().is_some_and(|x| x == "jsonl") && watched_roots.iter().any(|r| path.starts_with(&r.path)) {
                    let _ = tx.send(path);
                }
            }
        }
    }) else { crate::applog::line("Could not watch agent logs; hook updates still work."); return; };
    let mut registered = HashSet::new();
    for root in &roots {
        let dir = if root.path.is_dir() { Some(root.path.as_path()) } else { root.path.parent().filter(|dir| dir.is_dir()) };
        if let Some(dir) = dir && registered.insert(dir.to_path_buf()) && let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
            crate::applog::line(format!("Agent log watcher: {e}"));
        }
    }
    crate::applog::line(format!("Agent log watcher: {} folders; restoring recent activity", registered.len()));
    app.manage(LogWatcher { _watcher: Mutex::new(watcher) });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let startup_roots = roots.clone();
        if let Ok(paths) = tauri::async_runtime::spawn_blocking(move || recent_files(&startup_roots)).await {
            for path in paths { let _ = startup_tx.send(path); }
        }
        drop(startup_tx);
        let mut meta: HashMap<PathBuf, Meta> = HashMap::new();
        let mut seen = HashMap::new();
        let mut events: HashMap<PathBuf, i64> = HashMap::new();
        let started_at = chrono::Utc::now().timestamp_millis();
        while let Some(first) = rx.recv().await {
            let mut paths = HashSet::from([first]);
            tokio::time::sleep(Duration::from_millis(400)).await;
            while let Ok(path) = rx.try_recv() { paths.insert(path); }
            let settings = app.state::<AppState>().settings();
            let mut claude_changed = false;
            for path in paths {
                if path.components().any(|c| c.as_os_str() == "subagents") { continue; }
                let Some(agent) = roots.iter().find(|r| path.starts_with(&r.path)).map(|r| r.agent) else { continue };
                let Ok(file) = std::fs::metadata(&path) else { continue };
                let fingerprint = (file.len(), file.modified().ok());
                if seen.get(&path) == Some(&fingerprint) { continue; }
                seen.insert(path.clone(), fingerprint);
                let text = crate::limits::tail_of(&path, 512 * 1024);
                let info = meta.entry(path.clone()).or_insert_with(|| metadata(&path, &text, agent));
                if info.cwd.is_empty() { *info = metadata(&path, &text, agent); }
                if info.hidden || info.id.is_empty() || info.cwd.is_empty() { continue; }
                let state = app.state::<AppState>();
                let pet_session = lock(&state.tracker).is_pet_session(&info.id)
                    || lock(&state.chat_sessions).values().any(|id| id == &info.id);
                if pet_session { continue; }
                if settings.limits.enabled {
                    if agent == "codex" { crate::limits::on_codex_log(&app, &text); }
                    else { claude_changed = true; }
                }
                let observed = snapshot(&text, agent);
                if let Some(event) = observed.event && observed.at > 0 && observed.at > *events.get(&path).unwrap_or(&0) {
                    events.insert(path.clone(), observed.at);
                    if chrono::Utc::now().timestamp_millis() - observed.at < 10 * 60 * 1000 {
                        let payload = json!({"session_id":info.id,"cwd":info.cwd,"_glowby_agent":agent,"_glowby_observed_at":observed.at,"transcript_path":path.to_string_lossy(),"tool_name":observed.tool,"tool_input":{"command":observed.command}});
                        lock(&state.tracker).apply_log(event, &payload);
                        crate::projects::note_cwd(&app, &info.cwd);
                        crate::applog::debug(format!("agent log: {agent} {event}"));
                    }
                }
                if settings.chat.show_replies && let Some((text, at)) = observed.reply && at > 0 {
                    let fresh = lock(&state.tracker).record_reply(agent, &info.id, &info.cwd, &text, at);
                    if fresh {
                        crate::applog::debug(format!("agent reply: {agent}, {} characters (memory only)", text.chars().count()));
                        if at >= started_at && chrono::Utc::now().timestamp_millis() - at < 15_000 && settings.pet.show_on_done {
                            state::toast(&app, "done", format!("{} replied. Read the answer below.", if agent == "codex" { "Codex" } else { "Claude" }), crate::sessions::project_name(&info.cwd), 5);
                            state::hold_out(&app, 12);
                            crate::sounds::play(&app, crate::sounds::Sound::Done);
                            crate::pet_window::show(&app);
                        }
                    }
                }
                state::publish(&app);
            }
            if claude_changed { crate::limits::refresh_from_claude_log(&app); }
            if seen.len() > 200 { meta.clear(); seen.clear(); events.clear(); }
        }
    });
}

#[cfg(test)]
fn activity(text: &str, agent: &str) -> Option<&'static str> { snapshot(text, agent).event }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_activity_skips_unknown_lines() {
        assert_eq!(activity("{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"}}\nfuture", "codex"), Some("Stop"));
        assert_eq!(activity("{\"type\":\"user\"}", "claude"), Some("UserPromptSubmit"));
        assert_eq!(activity("garbage", "codex"), None);
    }
}
