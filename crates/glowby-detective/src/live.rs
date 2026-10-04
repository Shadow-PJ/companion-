//! Live checks on one running chat, from the end of its transcript:
//! * how big the conversation is and when its cache goes cold (so Glowby can
//!   warn before one message re-sends the whole chat);
//! * whether Claude just hit a usage limit, and the exact time it resets;
//! * a short handoff note to start a fresh chat with (built locally, no AI).

use crate::{Call, num, time_of};
use serde_json::Value;

/// Cache lifetime when the log doesn't say (the longer one: fewer false alarms).
pub const DEFAULT_TTL_IF_UNKNOWN: i64 = 3600;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LimitHit {
    pub at: i64,
    /// Unix seconds, from Claude's own message.
    pub resets_at: i64,
    /// "five_hour", "seven_day" …
    pub kind: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatState {
    /// Tokens the next message re-sends (the whole conversation so far).
    pub context: u64,
    /// When the last reply finished (unix seconds, 0 = none yet).
    pub last_reply_at: i64,
    /// Cache lifetime in seconds (from the log: 300 or 3600), if known.
    pub ttl: Option<i64>,
    pub limit_hit: Option<LimitHit>,
}

impl ChatState {
    /// When the cache goes cold (each reply refreshes it).
    pub fn expires_at(&self) -> i64 {
        self.last_reply_at + self.ttl.unwrap_or(DEFAULT_TTL_IF_UNKNOWN)
    }
}

/// Reads the state of a chat from (the end of) its transcript.
pub fn chat_state(text: &str) -> ChatState {
    let mut state = ChatState::default();
    for line in text.lines() {
        let limit_line = line.contains("\"quotaLimits\"");
        if !(line.contains("\"assistant\"") || line.contains("ompact") || limit_line) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue; // subagents have their own cache
        }
        let Some(t) = time_of(&v) else { continue };
        if let Some(q) = v.get("quotaLimits").filter(|q| q.get("status").and_then(Value::as_str) == Some("rejected")) {
            state.limit_hit = Some(LimitHit {
                at: t,
                resets_at: q.get("resetsAt").and_then(Value::as_i64).unwrap_or(0),
                kind: q.get("rateLimitType").and_then(Value::as_str).unwrap_or("").to_string(),
            });
            continue;
        }
        let compact = v.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
            || v.get("subtype").and_then(Value::as_str) == Some("compact_boundary");
        if compact {
            // after /compact the next reply only re-sends the short summary
            state.context = 0;
            continue;
        }
        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(msg) = v.get("message") else { continue };
        if msg.get("model").and_then(Value::as_str) == Some("<synthetic>") {
            continue; // Claude Code's own error messages, not a real reply
        }
        let Some(u) = msg.get("usage").filter(|u| u.is_object()) else { continue };
        let cc = u.get("cache_creation").cloned().unwrap_or(Value::Null);
        let call = Call {
            t,
            input: num(u, "input_tokens"),
            output: num(u, "output_tokens"),
            create: num(u, "cache_creation_input_tokens"),
            read: num(u, "cache_read_input_tokens"),
            create_1h: num(&cc, "ephemeral_1h_input_tokens"),
            create_5m: num(&cc, "ephemeral_5m_input_tokens"),
            ..Default::default()
        };
        if call.create_1h > 0 {
            state.ttl = Some(3600);
        } else if call.create_5m > 0 || (call.create > 0 && cc.is_null()) {
            state.ttl = Some(300);
        }
        if call.context() > 0 {
            state.context = call.context() + call.output;
            state.last_reply_at = t;
        }
    }
    state
}

// ---------------------------------------------------------------- handoff note

const MAX_PROMPT_CHARS: usize = 500;
const MAX_ANSWER_CHARS: usize = 1500;
const MAX_FILES: usize = 15;

fn shorten(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

/// Removes Claude Code's own tags (system reminders, pasted blocks, command
/// wrappers) from a prompt, keeping what you typed.
fn clean_prompt(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let name: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        let close = format!("</{name}>");
        match (name.is_empty(), after.find(&close)) {
            (false, Some(end)) => rest = &after[end + close.len()..],
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A note to paste into a fresh chat: your recent requests, the files that
/// changed, and how the last answer ended. Built from the transcript on this
/// PC; nothing is sent anywhere.
pub fn handoff_note(text: &str, cwd: &str) -> String {
    let mut prompts: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut last_answer = String::new();
    for line in text.lines() {
        if !(line.contains("\"user\"") || line.contains("\"assistant\"")) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let flag = |k: &str| v.get(k).and_then(Value::as_bool) == Some(true);
        if flag("isSidechain") || flag("isMeta") || flag("isCompactSummary") || flag("isApiErrorMessage") {
            continue;
        }
        let Some(msg) = v.get("message") else { continue };
        let content = msg.get("content").cloned().unwrap_or(Value::Null);
        match v.get("type").and_then(Value::as_str) {
            Some("user") => {
                let has_tool_result = content.as_array().is_some_and(|b| b.iter().any(|x| x.get("type").and_then(Value::as_str) == Some("tool_result")));
                if has_tool_result {
                    continue;
                }
                let prompt = clean_prompt(&text_of(&content));
                // "[Request interrupted by user]" and similar notes from Claude Code itself
                if prompt.len() >= 2 && !prompt.starts_with('[') && prompts.last() != Some(&prompt) {
                    prompts.push(prompt);
                }
            }
            Some("assistant") => {
                for block in content.as_array().into_iter().flatten() {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                        let path = block.pointer("/input/file_path").or_else(|| block.pointer("/input/notebook_path")).and_then(Value::as_str);
                        let scratch = |p: &str| p.contains("\\Temp\\") || p.contains("/Temp/") || p.starts_with("/tmp/");
                        if let (true, Some(path)) = (crate::EDIT_TOOLS.contains(&name), path.filter(|p| !scratch(p))) {
                            files.retain(|f| f != path);
                            files.push(path.to_string());
                        }
                    }
                }
                let answer = text_of(&content);
                if !answer.trim().is_empty() {
                    last_answer = answer;
                }
            }
            _ => {}
        }
    }
    let mut note = String::from(
        "Let's continue my work from an earlier chat. I started this new chat to save usage (the old one got very long).\n",
    );
    if !cwd.is_empty() {
        note.push_str(&format!("\nProject folder: {cwd}\n"));
    }
    if !files.is_empty() {
        let shown: Vec<&str> = files.iter().rev().take(MAX_FILES).map(String::as_str).collect();
        note.push_str(&format!("\nFiles changed in that chat (newest first):\n{}\n", shown.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n")));
    }
    if !prompts.is_empty() {
        let start = prompts.len().saturating_sub(4);
        note.push_str("\nMy last requests there (oldest first):\n");
        for (i, p) in prompts[start..].iter().enumerate() {
            note.push_str(&format!("{}. {}\n", i + 1, shorten(p, MAX_PROMPT_CHARS)));
        }
    }
    if !last_answer.is_empty() {
        note.push_str(&format!("\nHow your last answer there ended:\n{}\n", shorten(&last_answer, MAX_ANSWER_CHARS)));
    }
    note.push_str("\nLook only at the files you need, and ask me if something is unclear.");
    note
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lines(items: &[Value]) -> String {
        items.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n")
    }

    fn reply(ts: &str, read: u64, create_1h: u64) -> Value {
        json!({ "type": "assistant", "timestamp": ts, "sessionId": "s",
            "message": { "id": ts, "model": "claude-opus", "content": [{ "type": "text", "text": "ok" }],
                "usage": { "input_tokens": 2, "output_tokens": 100, "cache_read_input_tokens": read, "cache_creation_input_tokens": create_1h,
                    "cache_creation": { "ephemeral_1h_input_tokens": create_1h, "ephemeral_5m_input_tokens": 0 } } } })
    }

    #[test]
    fn size_and_cold_time_come_from_the_last_reply() {
        let text = lines(&[reply("2026-10-03T20:37:00Z", 900_000, 1000), reply("2026-10-03T20:37:40Z", 962_000, 1200)]);
        let s = chat_state(&text);
        assert_eq!(s.context, 962_000 + 1200 + 2 + 100);
        assert_eq!(s.ttl, Some(3600));
        let last = chrono::DateTime::parse_from_rfc3339("2026-10-03T20:37:40Z").unwrap().timestamp();
        assert_eq!(s.expires_at(), last + 3600);
        assert!(s.limit_hit.is_none());
    }

    #[test]
    fn a_limit_hit_carries_the_exact_reset_time_and_compact_resets_the_size() {
        let hit = json!({ "type": "assistant", "timestamp": "2026-10-04T15:44:18Z", "error": "rate_limit", "isApiErrorMessage": true,
            "message": { "model": "<synthetic>", "content": [{ "type": "text", "text": "You've hit your session limit" }],
                "usage": { "input_tokens": 0, "output_tokens": 0 } },
            "quotaLimits": { "status": "rejected", "resetsAt": 1791133800, "rateLimitType": "five_hour" } });
        let compact = json!({ "type": "system", "subtype": "compact_boundary", "timestamp": "2026-10-04T15:50:00Z" });
        let s = chat_state(&lines(&[reply("2026-10-04T15:43:10Z", 40_000, 926_000), hit]));
        assert_eq!(s.limit_hit, Some(LimitHit { at: 1791128658, resets_at: 1791133800, kind: "five_hour".into() }));
        assert!(s.context > 900_000, "the synthetic error message doesn't count as a reply");
        let s = chat_state(&lines(&[reply("2026-10-04T15:43:10Z", 40_000, 926_000), compact]));
        assert_eq!(s.context, 0);
    }

    #[test]
    fn handoff_note_keeps_your_requests_and_files_but_not_tags() {
        let user = |ts: &str, text: &str| json!({ "type": "user", "timestamp": ts, "message": { "role": "user", "content": text } });
        let edit = json!({ "type": "assistant", "timestamp": "2026-10-04T10:01:00Z",
            "message": { "content": [{ "type": "tool_use", "name": "Edit", "input": { "file_path": "C:/p/src/main.rs" } }] } });
        let tool_result = json!({ "type": "user", "timestamp": "2026-10-04T10:01:05Z",
            "message": { "content": [{ "type": "tool_result", "tool_use_id": "x", "content": "secret file text" }] } });
        let answer = json!({ "type": "assistant", "timestamp": "2026-10-04T10:02:00Z",
            "message": { "content": [{ "type": "text", "text": "Added the login page. Next: tests." }] } });
        let text = lines(&[
            user("2026-10-04T10:00:00Z", "add a login page <system-reminder>internal stuff</system-reminder>"),
            edit,
            tool_result,
            answer,
            user("2026-10-04T10:03:00Z", "now write tests"),
        ]);
        let note = handoff_note(&text, "C:/p");
        assert!(note.contains("1. add a login page\n"), "{note}");
        assert!(note.contains("2. now write tests"));
        assert!(note.contains("- C:/p/src/main.rs"));
        assert!(note.contains("Added the login page. Next: tests."));
        assert!(!note.contains("internal stuff") && !note.contains("secret file text"));
    }
}
