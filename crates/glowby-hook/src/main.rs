//! glowby-hook.exe: the tiny program Claude Code runs for every hook event.
//!
//! Usage (from ~/.claude/settings.json, "exec form", no shell involved):
//!     glowby-hook.exe <EventName>        with the event JSON on stdin
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

fn main() {
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

    let mut raw = String::new();
    if let Err(e) = std::io::stdin().take(MAX_STDIN_BYTES).read_to_string(&mut raw) {
        debug(&format!("couldn't read stdin: {e}"));
        return None;
    }
    let mut payload: Value = match serde_json::from_str(raw.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => {
            debug(&format!("stdin isn't JSON: {e}"));
            return None;
        }
    };
    trim_long_strings(&mut payload);

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
        event: event.clone(),
        wants_reply,
        from_pet_chat: std::env::var_os(PET_CHAT_ENV).is_some(),
        payload,
    };
    let mut line = serde_json::to_string(&envelope).ok()?;
    line.push('\n');
    (&pipe).write_all(line.as_bytes()).ok()?;
    (&pipe).flush().ok()?;

    if !wants_reply {
        return None;
    }
    let reply = wait_for_reply(pipe)?;
    render_reply(&event, reply)
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
    fn pass_prints_nothing() {
        assert!(render_reply("PermissionRequest", HookReply::Pass).is_none());
        assert!(render_reply(CHAT_GATE_EVENT, HookReply::Pass).is_none());
        assert!(render_reply("PreToolUse", HookReply::Allow).is_none());
    }

    #[test]
    fn long_strings_are_trimmed_on_char_boundaries() {
        let mut v = json!({ "content": "é".repeat(5000) });
        trim_long_strings(&mut v);
        let s = v["content"].as_str().unwrap();
        assert!(s.len() < MAX_STRING_BYTES + 20);
        assert!(s.ends_with("[trimmed]"));
    }
}

/// Keeps pipe traffic and Glowby's memory small: the pet only needs a preview.
fn trim_long_strings(value: &mut Value) {
    match value {
        Value::String(s) if s.len() > MAX_STRING_BYTES => {
            let mut cut = MAX_STRING_BYTES;
            while !s.is_char_boundary(cut) {
                cut -= 1;
            }
            s.truncate(cut);
            s.push_str(" …[trimmed]");
        }
        Value::Array(items) => items.iter_mut().for_each(trim_long_strings),
        Value::Object(map) => map.values_mut().for_each(trim_long_strings),
        _ => {}
    }
}
