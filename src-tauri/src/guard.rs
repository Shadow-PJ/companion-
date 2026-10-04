//! Glowby watches your Claude Code chats and steps in before usage is wasted,
//! instead of only telling you afterwards:
//!
//! * **Cold-chat guard.** Claude Code caches your conversation for a while
//!   (5 minutes or 1 hour). After that, the next message re-sends the whole
//!   chat, and for a big chat that one message can eat most of a 5-hour limit.
//!   When you send a message to a big chat whose cache has gone cold, Glowby
//!   stops it once (Claude Code's UserPromptSubmit hook), puts a fresh-start
//!   note plus your message on the clipboard, and tells you what sending would
//!   cost. Paste into a new chat to continue cheaply, or send again within
//!   10 minutes to go ahead anyway.
//! * **Cache reminder.** A couple of minutes before a big chat's cache goes
//!   cold (if you haven't replied), Glowby pops out.
//! * **Big chat helper.** When a chat gets very long, Glowby writes the
//!   fresh-start note for you and suggests starting over.
//! * **Limit hits.** When Claude says you've hit a usage limit, Glowby reads
//!   the exact reset time from Claude's own message (see limits.rs).
//!
//! Everything is read from the transcript on this PC. The fresh-start note is
//! built locally (no AI, no tokens) and only goes to your clipboard.

use crate::sessions::str_field;
use crate::state::{AppState, lock};
use glowby_detective::live::{self, ChatState};
use glowby_protocol::HookReply;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// The end of the transcript that holds the last reply (lines can be big).
const STATE_TAIL_BYTES: u64 = 4 * 1024 * 1024;
/// The end of the transcript used for the fresh-start note.
const NOTE_TAIL_BYTES: u64 = 8 * 1024 * 1024;
/// The reminder comes this long before the cache goes cold.
const REMIND_BEFORE_SECS: i64 = 120;
/// A little slack after the cold time before the guard steps in.
const COLD_GRACE_SECS: i64 = 30;
/// After a stopped message, sending again within this time goes through.
const PASS_SECS: i64 = 10 * 60;
/// A limit hit older than this is history, not news.
const RECENT_HIT_SECS: i64 = 30 * 60;
/// Chats not heard from for this long are forgotten.
const FORGET_SECS: i64 = 12 * 3600;

fn now_secs() -> i64 {
    chrono::Local::now().timestamp()
}

#[derive(Clone, Default)]
struct Chat {
    transcript: String,
    cwd: String,
    /// Claude Code's permission mode in this chat ("default", "auto", …).
    mode: String,
    seen_at: i64,
    state: ChatState,
    /// From the status line (terminal sessions): when the cache goes cold.
    status_expires: i64,
    last_prompt: i64,
    /// The cold time we already reminded you about.
    reminded_for: i64,
    /// The chat size at the last "big chat" alert.
    big_warned: u64,
    /// Sending again until then goes through (you were warned).
    pass_until: i64,
}

impl Chat {
    fn expires_at(&self) -> i64 {
        if self.status_expires > self.state.last_reply_at { self.status_expires } else { self.state.expires_at() }
    }
}

#[derive(Default)]
pub struct Guard {
    chats: HashMap<String, Chat>,
    /// The chat the latest alert is about ("Copy fresh-start note").
    alert_session: Option<String>,
}

pub fn tokens_text(n: u64) -> String {
    if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1e6) } else { format!("{}k", n / 1000) }
}

fn clock(t: i64) -> String {
    let Some(dt) = chrono::DateTime::from_timestamp(t, 0).map(|d| d.with_timezone(&chrono::Local)) else { return "?".into() };
    if now_secs() - t > 20 * 3600 { dt.format("%a %H:%M").to_string() } else { dt.format("%H:%M").to_string() }
}

fn read_state(transcript: &str) -> Option<ChatState> {
    (!transcript.is_empty()).then(|| live::chat_state(&crate::limits::tail_of(Path::new(transcript), STATE_TAIL_BYTES)))
}

/// Remembers which chat this event is from (and its transcript and mode).
fn note_chat(app: &AppHandle, payload: &Value) -> Option<String> {
    let sid = str_field(payload, "session_id")?.to_string();
    let state = app.state::<AppState>();
    let mut g = lock(&state.guard);
    let now = now_secs();
    g.chats.retain(|_, c| now - c.seen_at < FORGET_SECS);
    let chat = g.chats.entry(sid.clone()).or_default();
    chat.seen_at = now;
    if let Some(t) = str_field(payload, "transcript_path").filter(|t| !t.is_empty()) {
        chat.transcript = t.to_string();
    }
    if let Some(cwd) = str_field(payload, "cwd").filter(|c| !c.is_empty()) {
        chat.cwd = cwd.to_string();
    }
    if let Some(mode) = str_field(payload, "permission_mode") {
        chat.mode = mode.to_string();
    }
    Some(sid)
}

fn is_claude_chat(payload: &Value, from_pet_chat: bool) -> bool {
    !from_pet_chat && crate::sessions::agent_of(payload) == "claude"
}

/// Every (non-question) hook event.
pub fn on_event(app: &AppHandle, event: &str, payload: &Value, from_pet_chat: bool) {
    if !is_claude_chat(payload, from_pet_chat) {
        return;
    }
    let Some(sid) = note_chat(app, payload) else { return };
    if matches!(event, "Stop" | "StopFailure" | "SessionStart") {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || refresh(&app, &sid));
    }
}

/// Claude Code's status line (terminal sessions): the exact cold time.
pub fn on_status(app: &AppHandle, payload: &Value) {
    let Some(sid) = str_field(payload, "session_id") else { return };
    let expires = payload.pointer("/prompt_cache/expires_at").and_then(Value::as_i64).unwrap_or(0);
    if expires > 0 {
        let state = app.state::<AppState>();
        lock(&state.guard).chats.entry(sid.to_string()).or_default().status_expires = expires;
    }
}

/// After a reply: re-read the chat; limit hit? big chat? schedule the reminder.
fn refresh(app: &AppHandle, sid: &str) {
    let state = app.state::<AppState>();
    let transcript = lock(&state.guard).chats.get(sid).map(|c| c.transcript.clone()).unwrap_or_default();
    let Some(chat_state) = read_state(&transcript) else { return };
    let now = now_secs();
    if let Some(hit) = chat_state.limit_hit.as_ref().filter(|h| now - h.at < RECENT_HIT_SECS) {
        lock(&state.guard).alert_session = Some(sid.to_string());
        crate::limits::claude_limit_hit(app, hit);
    }
    let chat = {
        let mut g = lock(&state.guard);
        let Some(chat) = g.chats.get_mut(sid) else { return };
        chat.state = chat_state;
        chat.clone()
    };
    let s = state.settings().detective;
    if !s.enabled {
        return;
    }
    let size = chat.state.context;
    let big_again = chat.big_warned == 0 || size >= chat.big_warned + chat.big_warned / 2;
    if s.big_chat && size >= s.big_chat_tokens && big_again {
        let offer = crate::actions::Offer {
            kind: "bigChat",
            title: format!("This chat is getting expensive ({} tokens)", tokens_text(size)),
            detail: "Every message re-sends the whole conversation, and after a break all of it costs full price again. A fresh chat is much cheaper: I wrote a note with what you're working on.".into(),
            project: crate::sessions::project_name(&chat.cwd),
            url: None,
            dir: None,
        };
        if crate::alerts::raise(app, offer, Duration::from_secs(30 * 60), None) {
            let mut g = lock(&state.guard);
            if let Some(c) = g.chats.get_mut(sid) {
                c.big_warned = size;
            }
            g.alert_session = Some(sid.to_string());
        }
    }
    if s.cache_reminder && size >= s.guard_min_tokens {
        schedule_reminder(app.clone(), sid.to_string(), now);
    }
}

fn schedule_reminder(app: AppHandle, sid: String, stopped: i64) {
    tauri::async_runtime::spawn(async move {
        // the status line (if any) updates right after a reply
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop {
            let state = app.state::<AppState>();
            let Some(chat) = lock(&state.guard).chats.get(&sid).cloned() else { return };
            let expires = chat.expires_at();
            let now = now_secs();
            if chat.last_prompt > stopped || expires <= now || chat.reminded_for == expires {
                return; // you replied, it's already cold, or we told you
            }
            let wait = expires - REMIND_BEFORE_SECS - now;
            if wait > 0 {
                tokio::time::sleep(Duration::from_secs(wait as u64)).await;
                continue; // check again: you may have replied meanwhile
            }
            if let Some(c) = lock(&state.guard).chats.get_mut(&sid) {
                c.reminded_for = expires;
            }
            let size = tokens_text(chat.state.context);
            let offer = crate::actions::Offer {
                kind: "cacheSoon",
                title: format!("Reply by {} to keep this chat cheap", clock(expires)),
                detail: format!(
                    "Claude's cache of this {size}-token chat goes cold at {}. After that, your next message re-sends all {size} tokens at full price. Coming back later? Start a fresh chat with the note instead.",
                    clock(expires)
                ),
                project: crate::sessions::project_name(&chat.cwd),
                url: None,
                dir: None,
            };
            let notification = (format!("Reply by {} to keep your chat cheap", clock(expires)), format!("Its {size}-token cache goes cold then."));
            if crate::alerts::raise(&app, offer, Duration::from_secs((REMIND_BEFORE_SECS + 60) as u64), Some(notification)) {
                lock(&state.guard).alert_session = Some(sid.clone());
            }
            return;
        }
    });
}

// ---------------------------------------------------------------- the guard (a question: go or stop?)

/// Claude Code's UserPromptSubmit: let the message through, or stop it once
/// because it would re-send a big, cold chat.
pub async fn on_prompt(app: &AppHandle, payload: &Value, from_pet_chat: bool) -> HookReply {
    if !is_claude_chat(payload, from_pet_chat) {
        return HookReply::Pass;
    }
    let Some(sid) = note_chat(app, payload) else { return HookReply::Pass };
    let state = app.state::<AppState>();
    let now = now_secs();
    let prompt = str_field(payload, "prompt").unwrap_or("").to_string();
    let chat = {
        let mut g = lock(&state.guard);
        let Some(chat) = g.chats.get_mut(&sid) else { return HookReply::Pass };
        chat.last_prompt = now;
        chat.clone()
    };
    let s = state.settings().detective;
    // slash commands (/clear, /compact …) are your explicit choice
    if !s.enabled || !s.guard || prompt.trim_start().starts_with('/') {
        return HookReply::Pass;
    }
    let transcript = chat.transcript.clone();
    let Ok(Some(fresh)) = tauri::async_runtime::spawn_blocking(move || read_state(&transcript)).await else { return HookReply::Pass };
    let chat = {
        let mut g = lock(&state.guard);
        let Some(c) = g.chats.get_mut(&sid) else { return HookReply::Pass };
        c.state = fresh;
        c.clone()
    };
    let size = chat.state.context;
    let cold_since = chat.expires_at();
    let cold = size >= s.guard_min_tokens && chat.state.last_reply_at > 0 && now > cold_since + COLD_GRACE_SECS;
    if !cold {
        return HookReply::Pass;
    }
    if now < chat.pass_until {
        // you sent it again after the warning: your call
        if let Some(c) = lock(&state.guard).chats.get_mut(&sid) {
            c.pass_until = 0;
        }
        crate::applog::line(format!("guard: you sent to a cold {}-token chat again, letting it through", tokens_text(size)));
        return HookReply::Pass;
    }
    {
        let mut g = lock(&state.guard);
        if let Some(c) = g.chats.get_mut(&sid) {
            c.pass_until = now + PASS_SECS;
        }
        g.alert_session = Some(sid.clone()); // the guard always wins the card (top weight)
    }
    let copied = {
        let (transcript, cwd, app2) = (chat.transcript.clone(), chat.cwd.clone(), app.clone());
        tauri::async_runtime::spawn_blocking(move || {
            let note = live::handoff_note(&crate::limits::tail_of(Path::new(&transcript), NOTE_TAIL_BYTES), &cwd);
            crate::clipboard::write_text(&app2, format!("{note}\n\nMy next message:\n{prompt}"))
        })
        .await
        .unwrap_or(false)
    };
    crate::applog::line(format!("guard: stopped a message to a cold {}-token chat", tokens_text(size)));
    let size = tokens_text(size);
    let clip = if copied {
        "I put a fresh-start note with your message on the clipboard: paste it into a new chat (Ctrl+V)."
    } else {
        "Copy the fresh-start note (button below) into a new chat."
    };
    let offer = crate::actions::Offer {
        kind: "guard",
        title: "I stopped that message to save your usage".into(),
        detail: format!(
            "This chat is {size} tokens and its cache went cold at {}. Sending would re-send all of it at full price, which can be most of a 5-hour limit. {clip} Or send it again within 10 minutes to go ahead anyway.",
            clock(cold_since)
        ),
        project: crate::sessions::project_name(&chat.cwd),
        url: None,
        dir: None,
    };
    let _ = crate::alerts::raise(
        app,
        offer,
        Duration::from_secs(PASS_SECS as u64),
        Some((
            "Glowby stopped an expensive message".into(),
            if copied {
                format!("Your {size}-token chat went cold. Start fresh with the note on your clipboard, or send again to go ahead.")
            } else {
                format!("Your {size}-token chat went cold. Start a fresh chat (Glowby has a note for it), or send again to go ahead.")
            },
        )),
    );
    HookReply::Deny {
        message: format!(
            "Glowby: this chat's cache went cold, so this message would re-send the whole conversation (≈ {size} tokens at once). {} Or send the same message again within 10 minutes to go ahead anyway.",
            if copied { "A fresh-start note with your message is on your clipboard: paste it into a new chat to continue cheaply." } else { "Start a new chat to continue cheaply." }
        ),
    }
}

/// "Copy fresh-start note" on an alert.
pub fn copy_note(app: &AppHandle) {
    let state = app.state::<AppState>();
    let chat = {
        let g = lock(&state.guard);
        let sid = g.alert_session.clone();
        sid.and_then(|s| g.chats.get(&s).cloned())
            .or_else(|| g.chats.values().max_by_key(|c| c.seen_at).cloned())
    };
    let Some(chat) = chat.filter(|c| !c.transcript.is_empty()) else {
        crate::state::toast(app, "info", "No Claude chat to write a note about yet.".into(), String::new(), 5);
        crate::state::publish(app);
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let note = live::handoff_note(&crate::limits::tail_of(Path::new(&chat.transcript), NOTE_TAIL_BYTES), &chat.cwd);
        let text = if crate::clipboard::write_text(&app, note) {
            "Copied! Open a new chat and paste it (Ctrl+V)."
        } else {
            "Couldn't use the clipboard right now. Try again in a moment."
        };
        crate::state::toast(&app, "done", text.into(), crate::sessions::project_name(&chat.cwd), 8);
        crate::state::publish(&app);
    });
}

/// Why auto-allow may have nothing to do: every live Claude chat runs in a
/// mode where Claude doesn't ask.
pub fn auto_mode_note(app: &AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    let g = lock(&state.guard);
    let now = now_secs();
    let recent: Vec<&Chat> = g.chats.values().filter(|c| now - c.seen_at < 30 * 60 && !c.mode.is_empty()).collect();
    let quiet = |m: &str| matches!(m, "auto" | "bypassPermissions" | "dontAsk");
    (!recent.is_empty() && recent.iter().all(|c| quiet(&c.mode))).then(|| {
        "Your Claude chat runs in Claude's own Auto mode, so it doesn't ask anything and there's nothing for me to allow. I answer questions from chats in normal mode and from Codex.".to_string()
    })
}
