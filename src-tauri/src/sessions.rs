//! Turns the stream of hook events into "what is Claude doing?" and a mood.
//!
//! Everything here is pure logic (no windows, no I/O), which makes it easy to
//! reason about: events go in, `mood()` / `status()` come out.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mood {
    Idle,
    Working,
    Happy,
    Alert,
    Sleepy,
    Sick,
}

impl Mood {
    pub fn parse(text: &str) -> Option<Mood> {
        Some(match text {
            "idle" => Mood::Idle,
            "working" => Mood::Working,
            "happy" => Mood::Happy,
            "alert" => Mood::Alert,
            "sleepy" => Mood::Sleepy,
            "sick" => Mood::Sick,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Thinking,
    Working,
    NeedsYou,
    Done,
    Failed,
}

pub struct Session {
    pub agent: &'static str,
    pub last_hook: Option<Instant>,
    pub project: String,
    pub cwd: String,
    pub phase: Phase,
    pub activity: String,
    pub last_event: Instant,
    pub phase_since: Instant,
    pub from_pet_chat: bool,
    /// Tools used since your last prompt (a turn that used tools did real work).
    pub tools_this_turn: u32,
    pub started: Instant,
    /// Where Claude Code keeps this session's conversation (from the hook payload).
    pub transcript_path: String,
}

/// One live session, for squad mode.
#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub id: String,
    pub project: String,
    pub cwd: String,
    pub phase: Phase,
    pub activity: String,
    pub mood: Mood,
    pub started: Instant,
    pub transcript_path: String,
}

/// Why Glowby might pop out on its own.
pub enum Attention {
    Done(String),
    NeedsYou(String),
    Failed(String),
}

/// How long each temporary mood lasts.
const HAPPY_FOR: Duration = Duration::from_secs(6);
const SICK_FOR: Duration = Duration::from_secs(45);
/// A "working" session with no events for this long probably crashed or was closed.
const STALE_AFTER: Duration = Duration::from_secs(10 * 60);
/// No Claude activity at all for this long: Glowby gets sleepy.
const SLEEPY_AFTER: Duration = Duration::from_secs(30 * 60);

pub struct Tracker {
    sessions: HashMap<String, Session>,
    last_any_event: Instant,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    pub agent: &'static str,
    pub project: String,
    pub activity: String,
    pub phase: Phase,
    pub from_pet_chat: bool,
    /// How many other live sessions exist besides this one.
    pub others: usize,
}

impl Tracker {
    pub fn new() -> Self {
        Self { sessions: HashMap::new(), last_any_event: Instant::now() }
    }

    /// Feeds one hook event in. Returns a reason to pop out, if any.
    pub fn apply(&mut self, event: &str, p: &Value, from_pet_chat: bool) -> Option<Attention> {
        let now = Instant::now();
        let id = str_field(p, "session_id").unwrap_or("unknown").to_string();
        self.last_any_event = now;

        if event == "SessionEnd" {
            self.sessions.remove(&id);
            return None;
        }

        let cwd = str_field(p, "cwd").unwrap_or("").to_string();
        let project = project_name(&cwd);
        let session = self.sessions.entry(id).or_insert_with(|| Session {
            agent: agent_of(p),
            last_hook: Some(now),
            project: project.clone(),
            cwd: cwd.clone(),
            phase: Phase::Idle,
            activity: "Ready".into(),
            last_event: now,
            phase_since: now,
            from_pet_chat,
            tools_this_turn: 0,
            started: now,
            transcript_path: String::new(),
        });
        if let Some(path) = str_field(p, "transcript_path").filter(|p| !p.is_empty()) {
            session.transcript_path = path.to_string();
        }
        match event {
            "UserPromptSubmit" => session.tools_this_turn = 0,
            "PreToolUse" => session.tools_this_turn += 1,
            _ => {}
        }
        if !project.is_empty() {
            session.project = project;
            session.cwd = cwd;
        }
        session.agent = if p.get("_glowby_agent").is_some() || p.get("turn_id").is_some() || p.get("transcript_path").is_some() { agent_of(p) } else { session.agent };
        session.last_hook = Some(now);
        session.last_event = now;
        session.from_pet_chat |= from_pet_chat;

        let mut attention = None;
        let (phase, activity) = match event {
            "SessionStart" => (Phase::Idle, "Session started".to_string()),
            "UserPromptSubmit" => (Phase::Thinking, "Thinking…".to_string()),
            "PreToolUse" => (Phase::Working, describe_tool(p)),
            "PostToolUse" | "PostToolUseFailure" => (Phase::Working, session.activity.clone()),
            "PermissionRequest" => (Phase::NeedsYou, "Asking for permission".to_string()),
            "Notification" => match str_field(p, "notification_type").unwrap_or("") {
                "permission_prompt" | "agent_needs_input" | "elicitation_dialog" | "elicitation_url_dialog" => {
                    let msg = str_field(p, "message").unwrap_or("Claude needs you").to_string();
                    attention = Some(Attention::NeedsYou(msg.clone()));
                    (Phase::NeedsYou, msg)
                }
                _ => return None,
            },
            "Stop" => {
                let summary = first_line(str_field(p, "last_assistant_message").unwrap_or(""), 140);
                let text = if summary.is_empty() { "Finished".to_string() } else { summary };
                attention = Some(Attention::Done(text.clone()));
                (Phase::Done, text)
            }
            "StopFailure" => {
                let text = match str_field(p, "error_type").or_else(|| str_field(p, "error")).unwrap_or("") {
                    "rate_limit" => "Claude hit a rate limit and stopped.".to_string(),
                    "overloaded" => "Anthropic's servers are busy; Claude stopped.".to_string(),
                    "authentication_failed" => "Claude Code has a login problem.".to_string(),
                    "server_error" => "Claude stopped because of a server error.".to_string(),
                    "" => "Claude stopped because of an API error.".to_string(),
                    other => format!("Claude stopped: {}", other.replace('_', " ")),
                };
                attention = Some(Attention::Failed(text.clone()));
                (Phase::Failed, text)
            }
            _ => return None,
        };
        if session.phase != phase {
            session.phase_since = now;
        }
        session.phase = phase;
        session.activity = activity;
        attention
    }

    /// Called when a permission question is answered so the session leaves "needs you".
    pub fn permission_answered(&mut self, session_id: &str) {
        if let Some(s) = self.sessions.get_mut(session_id)
            && s.phase == Phase::NeedsYou
        {
            s.phase = Phase::Working;
            s.phase_since = Instant::now();
            s.activity = "Continuing…".into();
        }
    }

    fn live(&self, now: Instant) -> impl Iterator<Item = &Session> {
        self.sessions.values().filter(move |s| now.duration_since(s.last_event) < STALE_AFTER)
    }

    /// Glowby's mood: the most urgent mood of any live session.
    pub fn mood(&self, now: Instant) -> Mood {
        let moods: Vec<Mood> = self.live(now).map(|s| session_mood(s, now)).collect();
        for wanted in [Mood::Alert, Mood::Sick, Mood::Working, Mood::Happy] {
            if moods.contains(&wanted) {
                return wanted;
            }
        }
        if now.duration_since(self.last_any_event) > SLEEPY_AFTER {
            return Mood::Sleepy;
        }
        Mood::Idle
    }

    /// Your live Claude Code sessions (not Glowby's own chat), oldest first.
    pub fn live_sessions(&self, now: Instant) -> Vec<SessionInfo> {
        let mut list: Vec<SessionInfo> = self
            .sessions
            .iter()
            .filter(|(_, s)| !s.from_pet_chat && now.duration_since(s.last_event) < STALE_AFTER)
            .map(|(id, s)| SessionInfo {
                id: id.clone(),
                project: s.project.clone(),
                cwd: s.cwd.clone(),
                phase: s.phase,
                activity: s.activity.clone(),
                mood: session_mood(s, now),
                started: s.started,
                transcript_path: s.transcript_path.clone(),
            })
            .collect();
        list.sort_by(|a, b| a.started.cmp(&b.started).then_with(|| a.id.cmp(&b.id)));
        list
    }

    /// The next moment the mood could change on its own (e.g. "happy" fading to
    /// "idle"). The app sets ONE timer for that moment instead of polling.
    pub fn next_change(&self, now: Instant) -> Option<Instant> {
        let mut times = vec![self.last_any_event + SLEEPY_AFTER];
        for s in self.sessions.values() {
            times.push(s.last_event + STALE_AFTER);
            match s.phase {
                Phase::Done => times.push(s.phase_since + HAPPY_FOR),
                Phase::Failed => times.push(s.phase_since + SICK_FOR),
                _ => {}
            }
        }
        times.into_iter().filter(|t| *t > now).min()
    }

    /// Status of the most recently active session, for the bubble and status line.
    pub fn status(&self, now: Instant) -> Option<StatusView> {
        let live: Vec<&Session> = self.live(now).collect();
        let latest = live.iter().max_by_key(|s| s.last_event)?;
        Some(StatusView {
            agent: latest.agent,
            project: latest.project.clone(),
            activity: latest.activity.clone(),
            phase: latest.phase,
            from_pet_chat: latest.from_pet_chat,
            others: live.len().saturating_sub(1),
        })
    }

    /// Latest agent with real activity, excluding Glowby's own chat.
    pub fn latest_agent(&self) -> &'static str {
        self.sessions.values().filter(|s| !s.from_pet_chat && s.last_event.elapsed() < STALE_AFTER * 6).max_by_key(|s| s.last_event).map_or("claude", |s| s.agent)
    }

    pub fn session_agent(&self, id: &str) -> Option<&'static str> { self.sessions.get(id).map(|s| s.agent) }

    /// Log activity is a fallback only; recent hooks are authoritative.
    pub fn apply_log(&mut self, event: &str, payload: &Value) {
        let id = str_field(payload, "session_id").unwrap_or("");
        if self.sessions.get(id).and_then(|s| s.last_hook).is_some_and(|t| t.elapsed() < Duration::from_secs(15)) { return; }
        self.apply(event, payload, false);
        if let Some(s) = self.sessions.get_mut(id) { s.last_hook = None; }
    }

    pub fn tools_this_turn(&self, session_id: &str) -> u32 {
        self.sessions.get(session_id).map_or(0, |s| s.tools_this_turn)
    }

    /// Folder of your most recently active Claude Code session (not Glowby's own chat).
    pub fn latest_project_dir(&self) -> Option<String> {
        self.sessions
            .values()
            .filter(|s| !s.from_pet_chat && !s.cwd.is_empty())
            .max_by_key(|s| s.last_event)
            .map(|s| s.cwd.clone())
    }

    pub fn forget_stale(&mut self, now: Instant) {
        self.sessions.retain(|_, s| now.duration_since(s.last_event) < STALE_AFTER * 6);
    }
}

/// The mood one session would give Glowby on its own.
fn session_mood(s: &Session, now: Instant) -> Mood {
    match s.phase {
        Phase::NeedsYou => Mood::Alert,
        Phase::Failed if now.duration_since(s.phase_since) < SICK_FOR => Mood::Sick,
        Phase::Thinking | Phase::Working => Mood::Working,
        Phase::Done if now.duration_since(s.phase_since) < HAPPY_FOR => Mood::Happy,
        _ => Mood::Idle,
    }
}

/// "codex" or "claude": Codex events carry a `turn_id` and keep their
/// transcripts under ~/.codex; everything else is Claude Code.
pub fn agent_of(p: &Value) -> &'static str {
    if let Some(agent) = str_field(p, "_glowby_agent").filter(|a| *a == "codex" || *a == "claude") { return if agent == "codex" { "codex" } else { "claude" }; }
    let codex_log = str_field(p, "transcript_path").is_some_and(|t| {
        let t = t.to_ascii_lowercase().replace('/', "\\");
        t.contains("\\.codex\\")
    });
    if p.get("turn_id").is_some() || codex_log { "codex" } else { "claude" }
}

pub fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub fn project_name(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_string()
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn first_line(text: &str, max_chars: usize) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    shorten(line, max_chars)
}

pub fn shorten(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// "Edit" + {file_path: "src/main.rs"}  ->  "Editing main.rs"
pub fn describe_tool(p: &Value) -> String {
    let tool = str_field(p, "tool_name").unwrap_or("a tool");
    let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
    let path = ["file_path", "notebook_path", "path"]
        .iter()
        .find_map(|k| str_field(&input, k))
        .map(file_name)
        .unwrap_or("a file");
    match tool {
        "Read" | "NotebookRead" => format!("Reading {path}"),
        "Edit" | "MultiEdit" | "NotebookEdit" => format!("Editing {path}"),
        "Write" => format!("Writing {path}"),
        "Glob" | "Grep" | "LS" => "Searching the code".into(),
        "Bash" | "PowerShell" => {
            let cmd = str_field(&input, "command").unwrap_or("");
            format!("Running {}", first_line(cmd, 48))
        }
        "WebFetch" | "WebSearch" => "Browsing the web".into(),
        "Task" | "Agent" => "Working with a helper agent".into(),
        "TodoWrite" | "TaskCreate" | "TaskUpdate" => "Planning".into(),
        t if t.starts_with("mcp__") => {
            let server = t.split("__").nth(1).unwrap_or("an MCP");
            format!("Using {server} tools")
        }
        other => format!("Using {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(t: &mut Tracker, event: &str, extra: Value) -> Option<Attention> {
        let mut p = json!({ "session_id": "s1", "cwd": r"C:\code\my-app" });
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        t.apply(event, &p, false)
    }

    #[test]
    fn a_typical_turn_moves_through_the_moods() {
        let mut t = Tracker::new();
        let now = Instant::now();
        ev(&mut t, "SessionStart", json!({}));
        assert_eq!(t.mood(now), Mood::Idle);
        ev(&mut t, "UserPromptSubmit", json!({}));
        assert_eq!(t.mood(now), Mood::Working);
        ev(&mut t, "PreToolUse", json!({ "tool_name": "Edit", "tool_input": { "file_path": "src/main.rs" } }));
        assert_eq!(t.status(now).unwrap().activity, "Editing main.rs");
        assert_eq!(t.status(now).unwrap().project, "my-app");
        assert!(matches!(ev(&mut t, "Stop", json!({ "last_assistant_message": "Done!\nmore" })), Some(Attention::Done(s)) if s == "Done!"));
        assert_eq!(t.mood(Instant::now()), Mood::Happy);
        assert_eq!(t.mood(Instant::now() + HAPPY_FOR + Duration::from_secs(1)), Mood::Idle);
    }

    #[test]
    fn needs_you_wins_over_working() {
        let mut t = Tracker::new();
        ev(&mut t, "UserPromptSubmit", json!({}));
        ev(&mut t, "Notification", json!({ "notification_type": "permission_prompt", "message": "Bash needs permission" }));
        assert_eq!(t.mood(Instant::now()), Mood::Alert);
    }

    #[test]
    fn api_errors_make_glowby_sick_for_a_while() {
        let mut t = Tracker::new();
        ev(&mut t, "StopFailure", json!({ "error_type": "rate_limit" }));
        assert_eq!(t.mood(Instant::now()), Mood::Sick);
        assert!(t.next_change(Instant::now()).is_some());
    }

    #[test]
    fn live_sessions_lists_each_session_with_its_own_mood() {
        let mut t = Tracker::new();
        let start = |t: &mut Tracker, id: &str, cwd: &str| {
            t.apply("UserPromptSubmit", &json!({ "session_id": id, "cwd": cwd, "transcript_path": format!("C:\\t\\{id}.jsonl") }), false)
        };
        start(&mut t, "a", r"C:\code\one");
        std::thread::sleep(Duration::from_millis(5));
        start(&mut t, "b", r"C:\code\two");
        t.apply("Stop", &json!({ "session_id": "b", "cwd": r"C:\code\two" }), false);
        t.apply("UserPromptSubmit", &json!({ "session_id": "chat", "cwd": r"C:\code\one" }), true);
        let live = t.live_sessions(Instant::now());
        assert_eq!(live.len(), 2, "Glowby's own chat is not a squad member");
        assert_eq!((live[0].id.as_str(), live[0].mood), ("a", Mood::Working));
        assert_eq!((live[1].id.as_str(), live[1].mood), ("b", Mood::Happy));
        assert_eq!(live[1].project, "two");
        assert!(live[0].transcript_path.ends_with("a.jsonl"));
    }

    #[test]
    fn active_agent_switches_and_log_fallback_keeps_recent_hooks() {
        let mut t = Tracker::new();
        t.apply("UserPromptSubmit", &json!({"session_id":"codex-session","cwd":"C:/code","turn_id":"t"}), false);
        assert_eq!(t.latest_agent(), "codex");
        assert_eq!(t.status(Instant::now()).unwrap().agent, "codex");
        t.apply_log("Stop", &json!({"session_id":"codex-session","cwd":"C:/code","_glowby_agent":"codex"}));
        assert_eq!(t.status(Instant::now()).unwrap().phase, Phase::Thinking);
        t.apply("UserPromptSubmit", &json!({"session_id":"claude-session","cwd":"C:/code","_glowby_agent":"claude"}), false);
        assert_eq!(t.latest_agent(), "claude");
    }

    #[test]
    fn codex_and_claude_events_are_told_apart() {
        assert_eq!(agent_of(&json!({ "session_id": "a", "turn_id": "t1" })), "codex");
        assert_eq!(agent_of(&json!({ "transcript_path": r"C:\Users\me\.codex\sessions\2026\x.jsonl" })), "codex");
        assert_eq!(agent_of(&json!({ "transcript_path": "/home/me/.codex/sessions/x.jsonl" })), "codex");
        assert_eq!(agent_of(&json!({ "transcript_path": r"C:\Users\me\.claude\projects\x\y.jsonl" })), "claude");
    }

    #[test]
    fn session_end_forgets_the_session() {
        let mut t = Tracker::new();
        ev(&mut t, "UserPromptSubmit", json!({}));
        ev(&mut t, "SessionEnd", json!({}));
        assert!(t.status(Instant::now()).is_none());
    }
}
