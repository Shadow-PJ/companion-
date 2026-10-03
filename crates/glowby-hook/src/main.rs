//! glowby-hook.exe: the tiny program Claude Code (and Codex) run for every hook event.
//!
//! Usage (from ~/.claude/settings.json, "exec form", no shell involved):
//!     glowby-hook.exe <EventName>        with the event JSON on stdin
//!     glowby-hook.exe StatusLine         as Claude Code's status line: prints a short
//!                                        line and passes your usage limits to Glowby
//!
//! GOLDEN RULE: FAIL OPEN.
//! Whatever goes wrong (Glowby closed, crashed, slow, garbage data, a bug in this
//! file), we exit with code 0 and print nothing. To Claude Code that means
//! "the hook has no opinion", so it carries on exactly as if Glowby didn't exist.
//! We never use exit code 2, which is Claude Code's "block this action" signal.

use glowby_protocol::{CHAT_GATE_EVENT, HookEnvelope, HookReply, PET_CHAT_ENV, PROTOCOL_VERSION};
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long we try to reach Glowby before giving up (it's normally < 1 ms).
const CONNECT_BUDGET: Duration = Duration::from_millis(150);
/// Hard cap on waiting for a permission answer. Glowby answers or passes well before this.
const MAX_DECISION_WAIT: Duration = Duration::from_secs(590);
/// Ignore absurdly large inputs instead of loading them all into memory.
const MAX_STDIN_BYTES: u64 = 16 * 1024 * 1024;
/// Long strings (file contents in Write/Edit calls) are cut to this many bytes.
const MAX_STRING_BYTES: usize = 4000;
const ERROR_PIPE_BUSY: i32 = 231;

/// Even if something blocks unexpectedly, a hook process never lives longer than this.
const WATCHDOG_EXTRA: Duration = Duration::from_secs(15);

fn main() {
    // Watchdog: whatever happens, this process ends itself (exit 0 = no opinion).
    let waits_for_answer = std::env::args().nth(1).is_some_and(|e| e == "PermissionRequest" || e == CHAT_GATE_EVENT);
    let limit = if waits_for_answer { MAX_DECISION_WAIT + WATCHDOG_EXTRA } else { WATCHDOG_EXTRA };
    std::thread::spawn(move || {
        std::thread::sleep(limit);
        std::process::exit(0);
    });

    // catch_unwind turns any unexpected panic into "no output", never a crash code.
    let output = std::panic::catch_unwind(run).ok().flatten();
    if let Some(text) = output {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    }
    std::process::exit(0);
}

/// Set GLOWBY_HOOK_DEBUG=1 to see on stderr why an event wasn't delivered.
fn debug(step: &str) {
    if std::env::var_os("GLOWBY_HOOK_DEBUG").is_some() {
        eprintln!("glowby-hook: {step}");
    }
}

fn run() -> Option<String> {
    let Some(event) = std::env::args().nth(1) else {
        debug("no event name argument");
        return None;
    };
    let wants_reply = event == "PermissionRequest" || event == CHAT_GATE_EVENT;

    let mut payload = match read_one_json_value() {
        Ok(v) => v,
        Err(e) => {
            debug(&format!("couldn't read the event from stdin: {e}"));
            return None;
        }
    };
    if event == STATUS_LINE {
        // Always print the line (even when Glowby is closed); pass on only the
        // small part Glowby needs: your plan's usage limits.
        let text = status_text(&payload);
        let limits = json!({
            "session_id": payload.get("session_id"),
            "cwd": payload.pointer("/workspace/current_dir").or_else(|| payload.get("cwd")),
            "rate_limits": payload.get("rate_limits"),
            "prompt_cache": payload.get("prompt_cache"),
        });
        let _ = send(&event, false, limits);
        return Some(text);
    }
    trim_long_strings(&mut payload);
    let pipe = send(&event, wants_reply, payload)?;
    if !wants_reply {
        return None;
    }
    let reply = wait_for_reply(pipe)?;
    render_reply(&event, reply)
}

const STATUS_LINE: &str = "StatusLine";

/// "Glowby · Opus · ctx 12% · 5h 23% · week 41%" from Claude Code's status line data.
fn status_text(p: &Value) -> String {
    let num = |path: &str| p.pointer(path).and_then(Value::as_f64);
    let mut parts = vec!["Glowby".to_string()];
    if let Some(model) = p.pointer("/model/display_name").and_then(Value::as_str) {
        parts.push(model.to_string());
    }
    if let Some(c) = num("/context_window/used_percentage") {
        parts.push(format!("ctx {c:.0}%"));
    }
    if let Some(v) = num("/rate_limits/five_hour/used_percentage") {
        parts.push(format!("5h {v:.0}%"));
    }
    if let Some(v) = num("/rate_limits/seven_day/used_percentage") {
        parts.push(format!("week {v:.0}%"));
    }
    parts.join(" · ")
}

/// Connects to Glowby and sends one event. Returns the pipe (to wait for a reply).
fn send(event: &str, wants_reply: bool, payload: Value) -> Option<File> {

    let Some(pipe) = connect() else {
        debug("Glowby isn't listening (not running?)");
        return None;
    };
    if !server_is_trusted(&pipe) {
        debug("pipe owner isn't your Windows account, not sending");
        return None;
    }
    debug(&format!("sending {event}"));

    let envelope = HookEnvelope {
        v: PROTOCOL_VERSION,
        event: event.to_string(),
        wants_reply,
        from_pet_chat: std::env::var_os(PET_CHAT_ENV).is_some(),
        payload,
    };
    let mut line = serde_json::to_string(&envelope).ok()?;
    line.push('\n');
    (&pipe).write_all(line.as_bytes()).ok()?;
    (&pipe).flush().ok()?;
    Some(pipe)
}

/// Reads exactly ONE JSON object from stdin and stops at its closing brace.
/// We must not wait for end-of-file: Claude Code may keep stdin open after
/// writing the event, and a hook waiting for EOF would then hang forever.
fn read_one_json_value() -> Result<Value, String> {
    let mut input = std::io::stdin().lock();
    // Skip a UTF-8 byte-order mark (Windows PowerShell adds one when piping).
    if input.fill_buf().map_err(|e| e.to_string())?.starts_with(&[0xEF, 0xBB, 0xBF]) {
        input.consume(3);
    }
    serde_json::Deserializer::from_reader(input.take(MAX_STDIN_BYTES))
        .into_iter::<Value>()
        .next()
        .ok_or("stdin was empty")?
        .map_err(|e| format!("stdin isn't JSON: {e}"))
}

/// Opens the named pipe. If Glowby isn't running the pipe doesn't exist and we
/// return None immediately, so the hook costs Claude Code almost nothing.
fn connect() -> Option<File> {
    let name = glowby_protocol::pipe_name();
    let started = Instant::now();
    loop {
        match OpenOptions::new().read(true).write(true).open(&name) {
            Ok(file) => return Some(file),
            // All pipe instances busy for a moment: retry briefly.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && started.elapsed() < CONNECT_BUDGET => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

#[cfg(windows)]
fn server_is_trusted(pipe: &File) -> bool {
    glowby_protocol::win::pipe_server_is_same_user(pipe)
}

#[cfg(not(windows))]
fn server_is_trusted(_pipe: &File) -> bool {
    false
}

/// Blocking reads on a pipe have no timeout on Windows, so a helper thread does
/// the read while we wait with a deadline. If Glowby closes or crashes, the read
/// fails at once and we fall back to "no opinion".
fn wait_for_reply(pipe: File) -> Option<HookReply> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let ok = BufReader::new(pipe).read_line(&mut line).map(|n| n > 0).unwrap_or(false);
        let _ = tx.send(if ok { serde_json::from_str::<HookReply>(line.trim()).ok() } else { None });
    });
    rx.recv_timeout(MAX_DECISION_WAIT).ok().flatten()
}

/// Turns Glowby's answer into the exact JSON Claude Code expects on stdout.
fn render_reply(event: &str, reply: HookReply) -> Option<String> {
    let value = match (event, reply) {
        (_, HookReply::Pass) => return None,
        ("PermissionRequest", HookReply::Allow) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": { "behavior": "allow" }
            }
        }),
        ("PermissionRequest", HookReply::Deny { message }) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": { "behavior": "deny", "message": message }
            }
        }),
        (CHAT_GATE_EVENT, HookReply::Allow) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "permissionDecisionReason": "Approved in Glowby"
            }
        }),
        (CHAT_GATE_EVENT, HookReply::Deny { message }) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": message
            }
        }),
        _ => return None,
    };
    serde_json::to_string(&value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_json_value_is_read_without_waiting_for_eof() {
        // A reader that would block forever after the object = what an unclosed stdin looks like.
        struct NeverEnds(std::io::Cursor<Vec<u8>>);
        impl Read for NeverEnds {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = self.0.read(buf)?;
                if n == 0 { panic!("hook waited for end-of-file") } else { Ok(n) }
            }
        }
        let input = NeverEnds(std::io::Cursor::new(br#"{"session_id":"x","n":1}"#.to_vec()));
        let value = serde_json::Deserializer::from_reader(input).into_iter::<Value>().next().unwrap().unwrap();
        assert_eq!(value["n"], 1);
    }

    fn parse(text: Option<String>) -> Value {
        serde_json::from_str(&text.expect("expected output")).unwrap()
    }

    #[test]
    fn permission_allow_matches_claude_code_format() {
        let v = parse(render_reply("PermissionRequest", HookReply::Allow));
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
        assert_eq!(v["hookSpecificOutput"]["decision"]["behavior"], "allow");
    }

    #[test]
    fn permission_deny_carries_message() {
        let v = parse(render_reply("PermissionRequest", HookReply::Deny { message: "no".into() }));
        assert_eq!(v["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(v["hookSpecificOutput"]["decision"]["message"], "no");
    }

    #[test]
    fn chat_gate_uses_pre_tool_use_format() {
        let v = parse(render_reply(CHAT_GATE_EVENT, HookReply::Allow));
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "allow");
    }

    #[test]
    fn status_line_shows_usage_when_claude_code_provides_it() {
        let full = json!({
            "model": { "display_name": "Opus" },
            "context_window": { "used_percentage": 12.4 },
            "rate_limits": { "five_hour": { "used_percentage": 23.5, "resets_at": 1 }, "seven_day": { "used_percentage": 41.2 } }
        });
        assert_eq!(status_text(&full), "Glowby · Opus · ctx 12% · 5h 24% · week 41%");
        assert_eq!(status_text(&json!({})), "Glowby", "no data: still prints a line");
    }

    #[test]
    fn pass_prints_nothing() {
        assert!(render_reply("PermissionRequest", HookReply::Pass).is_none());
        assert!(render_reply(CHAT_GATE_EVENT, HookReply::Pass).is_none());
        assert!(render_reply("PreToolUse", HookReply::Allow).is_none());
    }

    #[test]
    fn long_strings_keep_start_and_end_on_char_boundaries() {
        let text = format!("START{}END", "é".repeat(5000));
        let mut v = json!({ "content": text });
        trim_long_strings(&mut v);
        let s = v["content"].as_str().unwrap();
        assert!(s.len() < MAX_STRING_BYTES + 40);
        assert!(s.starts_with("START") && s.ends_with("END") && s.contains("[trimmed]"));
    }
}

/// Keeps pipe traffic and Glowby's memory small. Keeps the START and the END of
/// long strings: errors and test summaries are usually at the end of the output.
fn trim_long_strings(value: &mut Value) {
    match value {
        Value::String(s) if s.len() > MAX_STRING_BYTES => {
            let half = MAX_STRING_BYTES / 2;
            let mut head_end = half;
            while !s.is_char_boundary(head_end) {
                head_end -= 1;
            }
            let mut tail_start = s.len() - half;
            while !s.is_char_boundary(tail_start) {
                tail_start += 1;
            }
            *s = format!("{}\n…[trimmed]…\n{}", &s[..head_end], &s[tail_start..]);
        }
        Value::Array(items) => items.iter_mut().for_each(trim_long_strings),
        Value::Object(map) => map.values_mut().for_each(trim_long_strings),
        _ => {}
    }
}
