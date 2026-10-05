//! Named-pipe server: receives events from glowby-hook.exe.
//!
//! A named pipe is a local-only channel (`\\.\pipe\name`). Unlike a TCP port it
//! can't be reached from the network, needs no firewall rule, and we lock it to
//! your Windows account with a security descriptor.

use crate::permissions::GateKind;
use crate::settings::ChatMode;
use crate::state::{self, AppState, lock};
use crate::{chat, commands, pet_window};
use glowby_protocol::{CHAT_GATE_EVENT, HookEnvelope, HookReply, SHOW_EVENT};
use std::ffi::c_void;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
use tokio::sync::oneshot;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

const MAX_MESSAGE_BYTES: u64 = 32 * 1024 * 1024;

/// Tools Glowby's own chat may use without asking (they only read).
const READ_ONLY_TOOLS: &[&str] = &[
    "Read", "Glob", "Grep", "LS", "NotebookRead", "TodoWrite", "TaskCreate", "TaskUpdate", "TaskList", "TaskGet",
    "ToolSearch", "Task", "Agent", "Skill",
];
const EDIT_TOOLS: &[&str] = &["Edit", "MultiEdit", "Write", "NotebookEdit"];

pub async fn run(app: AppHandle) {
    let name = glowby_protocol::pipe_name();
    let mut first = true;
    loop {
        let server = match create_instance(&name, first) {
            Ok(server) => server,
            Err(e) if first => {
                // Someone else owns the pipe name. Hooks will fail open; tell the user in Settings.
                let msg = format!("Couldn't open Glowby's pipe ({e}). Claude Code still works normally.");
                eprintln!("Glowby: {msg}");
                lock(&app.state::<AppState>().ui).pipe_error = Some(msg);
                return;
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        first = false;
        if server.connect().await.is_err() {
            continue;
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move { handle_client(app, server).await });
    }
}

fn create_instance(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first) // fails if another program already created this pipe
        .reject_remote_clients(true)
        .pipe_mode(PipeMode::Byte)
        .access_inbound(true)
        .access_outbound(true);
    match security_descriptor() {
        Some(descriptor) => {
            let mut attrs = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor as *mut c_void,
                bInheritHandle: 0,
            };
            unsafe { options.create_with_security_attributes_raw(name, &mut attrs as *mut _ as *mut c_void) }
        }
        None => options.create(name),
    }
}

/// "D:P(A;;GA;;;<your SID>)" = a protected access list with a single entry:
/// full access for your account, nobody else.
fn security_descriptor() -> Option<usize> {
    static DESCRIPTOR: OnceLock<Option<usize>> = OnceLock::new();
    *DESCRIPTOR.get_or_init(|| {
        let sid = glowby_protocol::win::current_user_sid().and_then(|s| glowby_protocol::win::sid_to_string(&s))?;
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})").encode_utf16().chain(std::iter::once(0)).collect();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        (ok != 0 && !descriptor.is_null()).then_some(descriptor as usize) // kept for the app's lifetime
    })
}

async fn handle_client(app: AppHandle, pipe: NamedPipeServer) {
    let (read_half, mut write_half) = tokio::io::split(pipe);
    let mut reader = BufReader::new(read_half.take(MAX_MESSAGE_BYTES));
    let mut line = String::new();
    match tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {}
        _ => return,
    }
    let Ok(envelope) = serde_json::from_str::<HookEnvelope>(&line) else { return };
    crate::applog::debug(format!("hook event: {} (pet chat: {})", envelope.event, envelope.from_pet_chat));

    if envelope.event == SHOW_EVENT {
        pet_window::peek(&app, 4);
        commands::open_settings_window(&app);
        return;
    }
    if envelope.event == "StatusLine" {
        // Claude Code's status line: only the usage-limit numbers, no session activity
        crate::limits::on_claude_status(&app, &envelope.payload);
        crate::guard::on_status(&app, &envelope.payload);
        return;
    }
    if envelope.event == "UserPromptSubmit" {
        // the usual activity update, then the cold-chat guard decides: go or stop once
        on_event(&app, &envelope);
        if envelope.wants_reply {
            let reply = crate::guard::on_prompt(&app, &envelope.payload, envelope.from_pet_chat).await;
            if matches!(reply, HookReply::Deny { .. }) {
                // the message never reached Claude: this turn is over, not "thinking"
                let mut p = envelope.payload.clone();
                if let Some(o) = p.as_object_mut() {
                    o.insert("last_assistant_message".into(), "Glowby stopped this message to save your usage".into());
                }
                lock(&app.state::<AppState>().tracker).apply("Stop", &p, false);
                state::publish(&app);
            }
            let mut text = serde_json::to_string(&reply).unwrap_or_else(|_| "{\"kind\":\"pass\"}".into());
            text.push('\n');
            let _ = write_half.write_all(text.as_bytes()).await;
            let _ = write_half.flush().await;
        }
        return;
    }
    if !envelope.wants_reply {
        on_event(&app, &envelope);
        return;
    }

    let reply = match begin_gate(&app, &envelope) {
        Gate::Now(reply) => reply,
        Gate::Wait(id, rx) => {
            let mut probe = [0u8; 1];
            tokio::select! {
                answer = rx => answer.unwrap_or(HookReply::Pass),
                // The hook process ended (Claude Code closed, or you answered in the terminal).
                _ = reader.read(&mut probe) => {
                    abandon_gate(&app, id);
                    return;
                }
            }
        }
    };
    let mut text = serde_json::to_string(&reply).unwrap_or_else(|_| "{\"kind\":\"pass\"}".into());
    text.push('\n');
    let _ = write_half.write_all(text.as_bytes()).await;
    let _ = write_half.flush().await;
}

/// A normal (non-question) hook event: update status / mood, maybe pop out.
fn on_event(app: &AppHandle, envelope: &HookEnvelope) {
    let state = app.state::<AppState>();
    crate::wellbeing::check(app); // catches a break reminder that is already overdue
    let attention = lock(&state.tracker).apply(&envelope.event, &envelope.payload, envelope.from_pet_chat);
    if envelope.from_pet_chat {
        chat::on_hook_activity(app, &envelope.event, &envelope.payload);
    }
    let settings = state.settings();
    let project = crate::sessions::project_name(crate::sessions::str_field(&envelope.payload, "cwd").unwrap_or(""));
    let mut pop_out = false;
    // Tests / builds: sick while failing, happy again when they pass.
    if matches!(envelope.event.as_str(), "PostToolUse" | "PostToolUseFailure")
        && let Some((kind, text)) = crate::actions::on_tool_result(app, &envelope.event, &envelope.payload)
    {
        if kind == "done" {
            // tests / build pass again: the squad pet of that session gets XP too
            crate::squad::fixed(app, crate::sessions::str_field(&envelope.payload, "session_id").unwrap_or(""));
        }
        state::toast(app, kind, text, project.clone(), 8);
        let wanted = if kind == "failed" { settings.pet.show_on_attention } else { settings.pet.show_on_done };
        pop_out |= wanted && !envelope.from_pet_chat;
    }
    if !envelope.from_pet_chat {
        match &attention {
            Some(crate::sessions::Attention::Done(text)) => {
                crate::sounds::play(app, crate::sounds::Sound::Done);
                if settings.pet.show_on_done {
                    state::toast(app, "done", text.clone(), project, 5);
                    pop_out = true;
                }
            }
            Some(crate::sessions::Attention::NeedsYou(text)) => {
                crate::sounds::play(app, crate::sounds::Sound::Alert);
                if settings.pet.show_on_attention {
                    state::toast(app, "attention", text.clone(), project, 8);
                    pop_out = true;
                }
            }
            Some(crate::sessions::Attention::Failed(text)) => {
                crate::sounds::play(app, crate::sounds::Sound::Error);
                if settings.pet.show_on_attention {
                    state::toast(app, "failed", text.clone(), project, 8);
                    pop_out = true;
                }
            }
            None => {}
        }
    }
    if pop_out {
        pet_window::show(app);
    }
    // Smart features: remember the repo, count coding time, learn mode.
    let payload = &envelope.payload;
    crate::projects::note_cwd(app, crate::sessions::str_field(payload, "cwd").unwrap_or(""));
    crate::quests::tick(app);
    crate::learn::on_event(app, &envelope.event, payload);
    crate::limits::on_event(app, &envelope.event, payload);
    crate::guard::on_event(app, &envelope.event, payload, envelope.from_pet_chat);
    let session = crate::sessions::str_field(payload, "session_id").unwrap_or("");
    let tools_this_turn = lock(&state.tracker).tools_this_turn(session);
    crate::squad::on_event(app, &envelope.event, payload, envelope.from_pet_chat, tools_this_turn);
    if envelope.event == "PostToolUse" {
        if let Some(file) = crate::quests::written_test_file(payload) {
            crate::quests::test_written(app, file);
        }
        let command = payload.pointer("/tool_input/command").and_then(serde_json::Value::as_str).unwrap_or("");
        if command.contains("git push") {
            crate::github::after_push(app);
        }
    }
    // Progression: you're coding (streak, energy, break timer); finished turns earn XP.
    match envelope.event.as_str() {
        "UserPromptSubmit" => {
            crate::wellbeing::activity(app);
            crate::progress::activity(app, true);
        }
        "Stop" => {
            let session = crate::sessions::str_field(payload, "session_id").unwrap_or("");
            let tools = lock(&state.tracker).tools_this_turn(session);
            crate::progress::task_finished(app, tools > 0);
            crate::projects::save(app);
        }
        _ => {}
    }
    state::publish(app);
}

enum Gate {
    Now(HookReply),
    Wait(u64, oneshot::Receiver<HookReply>),
}

/// A question that needs your answer (permission prompt or chat gate).
fn begin_gate(app: &AppHandle, envelope: &HookEnvelope) -> Gate {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let tool = crate::sessions::str_field(&envelope.payload, "tool_name").unwrap_or("");
    let is_chat = envelope.event == CHAT_GATE_EVENT;
    // Auto-allow (timed or full), unless the never-auto list or the project
    // folder rule says to ask. Glowby's own read-only quick actions still block edits.
    let read_only_chat = is_chat && lock(&state.chat).read_only && EDIT_TOOLS.contains(&tool);
    if !read_only_chat && crate::autoallow::try_allow(app, &envelope.payload) {
        state::publish(app);
        return Gate::Now(HookReply::Allow);
    }
    if lock(&state.ui).game_active {
        return Gate::Now(HookReply::Pass);
    }
    let kind = if is_chat {
        let read_only_run = lock(&state.chat).read_only;
        let command = envelope.payload.pointer("/tool_input/command").and_then(serde_json::Value::as_str).unwrap_or("");
        // Plain read-only commands like `git log` don't need your OK.
        if matches!(tool, "Bash" | "PowerShell") && crate::actions::is_safe_read_command(command) {
            return Gate::Now(HookReply::Allow);
        }
        if read_only_run && EDIT_TOOLS.contains(&tool) {
            return Gate::Now(HookReply::Deny {
                message: "This quick action is read-only, so Glowby blocked file changes. Explain instead.".into(),
            });
        }
        let needs_ok = match (read_only_run, settings.chat.mode) {
            (true, _) | (false, ChatMode::Ask) => !READ_ONLY_TOOLS.contains(&tool),
            (false, ChatMode::ReadOnly) => false,
            (false, ChatMode::AcceptEdits) => !READ_ONLY_TOOLS.contains(&tool) && !EDIT_TOOLS.contains(&tool),
        };
        if !needs_ok {
            return Gate::Now(HookReply::Pass);
        }
        GateKind::ChatGate
    } else {
        if !settings.permissions.enabled {
            return Gate::Now(HookReply::Pass);
        }
        GateKind::Permission
    };

    let timeout = Duration::from_secs(settings.permissions.timeout_secs as u64);
    let (tx, rx) = oneshot::channel();
    let id = lock(&state.perms).push(kind, &envelope.payload, timeout, tx);
    lock(&state.tracker).apply("PermissionRequest", &envelope.payload, envelope.from_pet_chat);

    // If you don't answer in time: terminal prompts go back to the terminal,
    // chat-gate questions are denied (the chat has no terminal to fall back to).
    let app_for_timeout = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(timeout).await;
        let fallback = match kind {
            GateKind::Permission => HookReply::Pass,
            GateKind::ChatGate => HookReply::Deny { message: "No answer in Glowby in time, so it was not allowed.".into() },
        };
        resolve(&app_for_timeout, id, fallback);
    });

    crate::sounds::play(app, crate::sounds::Sound::Alert);
    if settings.pet.show_on_permission {
        pet_window::show(app);
    }
    state::publish(app);
    Gate::Wait(id, rx)
}

/// Sends your answer to the waiting hook.
pub fn resolve(app: &AppHandle, id: u64, reply: HookReply) -> bool {
    let state = app.state::<AppState>();
    let (session, delivered) = {
        let mut queue = lock(&state.perms);
        let session = queue.session_id(id);
        let delivered = queue.resolve(id, reply).is_some();
        (session, delivered)
    };
    if let Some(session) = session {
        // The question is no longer waiting even if its hook disconnected.
        lock(&state.tracker).permission_answered(&session);
        state::publish(app);
    }
    delivered
}

fn abandon_gate(app: &AppHandle, id: u64) {
    let state = app.state::<AppState>();
    let session = lock(&state.perms).forget(id);
    if let Some(session) = session {
        lock(&state.tracker).permission_answered(&session);
        state::publish(app);
    }
}
