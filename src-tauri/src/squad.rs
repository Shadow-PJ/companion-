//! Squad mode (off by default): one small pet for each running Claude Code
//! session, sitting next to Glowby. Each pet earns XP from what its own session
//! does and levels up separately. Saved in squad.json, so a resumed session
//! (`claude --resume`) gets its pet back; pets of sessions you haven't used for
//! 30 days are forgotten.

use crate::sessions::{Mood, Phase, SessionInfo, project_name, str_field};
use crate::settings;
use crate::state::{self, AppState, lock};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

pub const TOOL_XP: u64 = 1;
pub const TASK_XP: u64 = 10;
pub const CHAT_XP: u64 = 2;
pub const COMMIT_XP: u64 = 8;
pub const FIX_XP: u64 = 20;
const FORGET_AFTER_DAYS: i64 = 30;
const MAX_REMEMBERED: usize = 200;

/// Names for squad pets (Glowby's own, not borrowed from anywhere).
const NAMES: [&str; 24] = [
    "Pip", "Nova", "Bloop", "Ziggy", "Mochi", "Puff", "Dot", "Echo", "Juno", "Kiwi", "Luma", "Orbit", "Pebble", "Sprite",
    "Tango", "Wisp", "Bubbles", "Comet", "Fizz", "Gizmo", "Hoot", "Jinx", "Nimbus", "Quill",
];
const COLORS: [&str; 6] = ["mint", "peach", "lilac", "rose", "aqua", "periwinkle"];

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Member {
    pub name: String,
    pub project: String,
    /// The folder the session was in when Glowby first saw it.
    pub cwd: String,
    /// Folder name under ~/.claude/projects where Claude Code keeps this conversation.
    pub transcript_dir: String,
    pub xp: u64,
    pub tasks: u32,
    pub tools: u32,
    pub first_seen: String,
    pub last_seen: String,
    /// An imported character's id, or "" for a little jellyfish.
    pub character: String,
    /// An anime pet ("neko" …), or "" for a little jellyfish (a character wins).
    pub species: String,
    pub color: String,
    /// The chat's own copy of this session (made with `--fork-session`), so
    /// follow-up questions continue there and never touch your running session.
    pub fork: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Squad {
    pub members: BTreeMap<String, Member>,
}

/// Total XP for a level: 2 at 20 XP, 3 at 60, 4 at 120, 5 at 200 … Faster than
/// Glowby's own curve, because one session is short.
pub fn threshold(level: u32) -> u64 {
    let l = level as u64;
    10 * l * l.saturating_sub(1)
}

pub fn level_for(xp: u64) -> u32 {
    let mut level = 1;
    while xp >= threshold(level + 1) {
        level += 1;
    }
    level
}

/// Little pets evolve too: at levels 4, 8 and 12.
pub fn stage_for(level: u32) -> usize {
    match level {
        0..=3 => 0,
        4..=7 => 1,
        8..=11 => 2,
        _ => 3,
    }
}

/// FNV-1a: a tiny, stable hash, so a session always gets the same name and colour.
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

impl Squad {
    fn new_member(&self, id: &str, cwd: &str, now: &str) -> Member {
        let taken: Vec<&str> = self.members.values().map(|m| m.name.as_str()).collect();
        let h = hash(id) as usize;
        let name = (0..NAMES.len())
            .map(|i| NAMES[(h + i) % NAMES.len()])
            .find(|n| !taken.contains(n))
            .map(String::from)
            .unwrap_or_else(|| format!("{} {}", NAMES[h % NAMES.len()], self.members.len() + 1));
        Member {
            name,
            project: project_name(cwd),
            cwd: cwd.to_string(),
            first_seen: now.to_string(),
            last_seen: now.to_string(),
            color: COLORS[h % COLORS.len()].to_string(),
            ..Default::default()
        }
    }

    /// The pet for a session, created the first time Glowby sees it.
    pub fn member_mut(&mut self, id: &str, cwd: &str, now: &str) -> &mut Member {
        if !self.members.contains_key(id) {
            let member = self.new_member(id, cwd, now);
            self.members.insert(id.to_string(), member);
        }
        let m = self.members.entry(id.to_string()).or_default();
        if m.cwd.is_empty() && !cwd.is_empty() {
            m.cwd = cwd.to_string();
            m.project = project_name(cwd);
        }
        m
    }

    /// Adds XP. Returns the new level if it went up.
    pub fn add_xp(&mut self, id: &str, amount: u64) -> Option<u32> {
        let m = self.members.get_mut(id)?;
        let before = level_for(m.xp);
        m.xp += amount;
        let after = level_for(m.xp);
        (after > before).then_some(after)
    }

    /// Forgets pets of sessions unused for a month (and keeps the file small).
    pub fn prune(&mut self, now: DateTime<Local>) {
        let age = |m: &Member| {
            DateTime::parse_from_rfc3339(&m.last_seen).map(|t| (now - t.with_timezone(&Local)).num_days()).unwrap_or(i64::MAX)
        };
        self.members.retain(|_, m| age(m) < FORGET_AFTER_DAYS);
        while self.members.len() > MAX_REMEMBERED {
            let Some(oldest) = self.members.iter().max_by_key(|(_, m)| age(m)).map(|(id, _)| id.clone()) else { break };
            self.members.remove(&oldest);
        }
    }
}

/// Claude Code keeps a conversation under ~/.claude/projects/<folder>, where
/// <folder> is the path the session started in with every character that isn't
/// a letter or digit replaced by "-". `--resume` only finds it from that same
/// folder, so walk up from the session's folder until the names match.
pub fn resume_dir(cwd: &str, transcript_dir: &str) -> String {
    let slug = |p: &str| p.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>();
    let mut dir = Some(Path::new(cwd));
    while let Some(d) = dir {
        if !transcript_dir.is_empty() && slug(&d.to_string_lossy()) == transcript_dir {
            return d.to_string_lossy().into_owned();
        }
        dir = d.parent();
    }
    cwd.to_string()
}

fn transcript_folder(transcript_path: &str) -> Option<String> {
    Path::new(transcript_path).parent()?.file_name().map(|n| n.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------- app glue

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SquadView {
    pub id: String,
    pub name: String,
    pub project: String,
    pub phase: Phase,
    pub activity: String,
    pub mood: Mood,
    pub level: u32,
    pub stage: usize,
    pub xp_into_level: u64,
    pub xp_for_level: u64,
    pub tasks: u32,
    pub tools: u32,
    /// Minutes since Glowby first saw this session today.
    pub minutes: u64,
    pub character: String,
    pub species: String,
    pub color: String,
    pub has_chat: bool,
}

#[derive(Serialize, Clone)]
struct LevelUp {
    id: String,
    text: String,
}

fn save(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut squad = lock(&state.squad);
    squad.prune(Local::now());
    if let Err(e) = settings::save_json(&state.paths.squad_file, &*squad) {
        crate::applog::line(format!("couldn't save squad.json: {e}"));
    }
}

/// Feeds a hook event to the session's squad pet (only while squad mode is on).
pub fn on_event(app: &AppHandle, event: &str, payload: &Value, from_pet_chat: bool, tools_this_turn: u32) {
    let state = app.state::<AppState>();
    if from_pet_chat || !lock(&state.settings).squad.enabled {
        return;
    }
    let Some(id) = str_field(payload, "session_id").filter(|s| !s.is_empty()) else { return };
    let cwd = str_field(payload, "cwd").unwrap_or("");
    let now = Local::now().to_rfc3339();
    let (level_up, persist) = {
        let mut squad = lock(&state.squad);
        let m = squad.member_mut(id, cwd, &now);
        m.last_seen = now;
        if m.transcript_dir.is_empty()
            && let Some(dir) = str_field(payload, "transcript_path").and_then(transcript_folder)
        {
            m.transcript_dir = dir;
        }
        let (xp, persist) = match event {
            "PostToolUse" => {
                m.tools += 1;
                let command = payload.pointer("/tool_input/command").and_then(Value::as_str).unwrap_or("");
                (TOOL_XP + if crate::actions::is_commit_command(command) { COMMIT_XP } else { 0 }, false)
            }
            "Stop" if tools_this_turn > 0 => {
                m.tasks += 1;
                (TASK_XP, true)
            }
            "Stop" => (CHAT_XP, true),
            "SessionStart" | "SessionEnd" => (0, true),
            _ => (0, false),
        };
        let up = if xp > 0 { squad.add_xp(id, xp) } else { None };
        (up, persist || up.is_some())
    };
    if let Some(level) = level_up {
        celebrate(app, id, level);
    }
    if persist {
        save(app);
    }
}

/// Tests or a build in this session pass again.
pub fn fixed(app: &AppHandle, session_id: &str) {
    let state = app.state::<AppState>();
    if !lock(&state.settings).squad.enabled {
        return;
    }
    let up = lock(&state.squad).add_xp(session_id, FIX_XP);
    if let Some(level) = up {
        celebrate(app, session_id, level);
    }
    save(app);
}

fn celebrate(app: &AppHandle, id: &str, level: u32) {
    let text = if stage_for(level) > stage_for(level - 1) { format!("Lv {level}! Evolved!") } else { format!("Lv {level}!") };
    let _ = app.emit_to(crate::pet_window::LABEL, "pet://squad-levelup", LevelUp { id: id.to_string(), text });
}

/// The pets to show: one per live session, oldest session first.
pub fn views(
    app: &AppHandle,
    live: &[SessionInfo],
    max: usize,
    known_character: impl Fn(&str) -> bool,
    pet_ok: impl Fn(&str) -> bool,
) -> Vec<SquadView> {
    let state = app.state::<AppState>();
    let now = Instant::now();
    let stamp = Local::now().to_rfc3339();
    let mut squad = lock(&state.squad);
    live.iter()
        .take(max)
        .map(|s| {
            let m = squad.member_mut(&s.id, &s.cwd, &stamp);
            if m.transcript_dir.is_empty()
                && let Some(dir) = transcript_folder(&s.transcript_path)
            {
                m.transcript_dir = dir;
            }
            let level = level_for(m.xp);
            SquadView {
                id: s.id.clone(),
                name: m.name.clone(),
                project: if s.project.is_empty() { m.project.clone() } else { s.project.clone() },
                phase: s.phase,
                activity: s.activity.clone(),
                mood: s.mood,
                level,
                stage: stage_for(level),
                xp_into_level: m.xp - threshold(level),
                xp_for_level: threshold(level + 1) - threshold(level),
                tasks: m.tasks,
                tools: m.tools,
                minutes: now.duration_since(s.started).as_secs() / 60,
                character: if known_character(&m.character) { m.character.clone() } else { String::new() },
                species: if pet_ok(&m.species) { m.species.clone() } else { String::new() },
                color: m.color.clone(),
                has_chat: !m.fork.is_empty(),
            }
        })
        .collect()
}

/// "Look" chips on a squad pet's card: a jellyfish, an anime pet you've
/// unlocked, or one of your characters.
pub fn set_look(app: &AppHandle, id: &str, character: &str, species: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    if !character.is_empty() && !lock(&state.characters).exists(character) {
        return Err("No such character.".into());
    }
    if !species.is_empty() && !crate::progress::pet_unlocked(&lock(&state.progress), species) {
        return Err("That pet isn't unlocked yet.".into());
    }
    {
        let mut squad = lock(&state.squad);
        let m = squad.members.get_mut(id).ok_or("That session is gone.")?;
        m.character = character.to_string();
        m.species = species.to_string();
    }
    save(app);
    state::publish(app);
    Ok(())
}

/// A character was deleted: its squad pets become jellyfish again.
pub fn forget_character(app: &AppHandle, character: &str) {
    let state = app.state::<AppState>();
    let changed = {
        let mut squad = lock(&state.squad);
        let mut changed = false;
        for m in squad.members.values_mut().filter(|m| m.character == character) {
            m.character.clear();
            changed = true;
        }
        changed
    };
    if changed {
        save(app);
    }
}

/// Where and how to chat with a squad pet's session.
pub struct ChatTarget {
    pub session_id: String,
    pub name: String,
    pub dir: String,
}

pub fn chat_target(app: &AppHandle, id: &str) -> Option<ChatTarget> {
    let state = app.state::<AppState>();
    let squad = lock(&state.squad);
    let m = squad.members.get(id)?;
    let dir = resume_dir(&m.cwd, &m.transcript_dir);
    Path::new(&dir).is_dir().then(|| ChatTarget { session_id: id.to_string(), name: m.name.clone(), dir })
}

/// True if Claude Code has this conversation on disk
/// (~/.claude/projects/<folder>/<id>.jsonl), so `--resume` can open it.
/// Codex sessions aren't there.
pub fn is_claude_session(id: &str) -> bool {
    let safe = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let Some(home) = std::env::var_os("USERPROFILE") else { return false };
    let projects = Path::new(&home).join(".claude").join("projects");
    safe && std::fs::read_dir(projects)
        .map(|dirs| dirs.flatten().any(|d| d.path().join(format!("{id}.jsonl")).is_file()))
        .unwrap_or(false)
}

/// The chat's own copy of this session, if one exists.
pub fn fork_of(app: &AppHandle, id: &str) -> String {
    lock(&app.state::<AppState>().squad).members.get(id).map(|m| m.fork.clone()).unwrap_or_default()
}

pub fn set_fork(app: &AppHandle, id: &str, fork: &str) {
    if let Some(m) = lock(&app.state::<AppState>().squad).members.get_mut(id) {
        m.fork = fork.to_string();
    }
    save(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squad_levels_come_quickly() {
        assert_eq!(level_for(0), 1);
        assert_eq!(level_for(19), 1);
        assert_eq!(level_for(20), 2);
        assert_eq!(level_for(60), 3);
        assert_eq!(level_for(threshold(4)), 4);
        assert_eq!(stage_for(3), 0);
        assert_eq!(stage_for(4), 1);
        assert_eq!(stage_for(12), 3);
    }

    #[test]
    fn each_session_gets_its_own_stable_name() {
        let mut squad = Squad::default();
        let now = Local::now().to_rfc3339();
        let a = squad.member_mut("session-a", r"C:\code\one", &now).name.clone();
        let b = squad.member_mut("session-b", r"C:\code\two", &now).name.clone();
        assert_ne!(a, b);
        assert_eq!(squad.member_mut("session-a", "", &now).name, a, "same session, same pet");
        assert_eq!(squad.members["session-b"].project, "two");
        for i in 0..40 {
            squad.member_mut(&format!("s{i}"), "", &now);
        }
        let mut names: Vec<&str> = squad.members.values().map(|m| m.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), squad.members.len(), "names stay unique even past the list");
    }

    #[test]
    fn xp_levels_each_pet_separately() {
        let mut squad = Squad::default();
        let now = Local::now().to_rfc3339();
        squad.member_mut("a", "", &now);
        squad.member_mut("b", "", &now);
        assert_eq!(squad.add_xp("a", 10), None);
        assert_eq!(squad.add_xp("a", 10), Some(2));
        assert_eq!(level_for(squad.members["b"].xp), 1);
        assert_eq!(squad.add_xp("missing", 50), None);
    }

    #[test]
    fn old_pets_are_forgotten() {
        let mut squad = Squad::default();
        let now = Local::now();
        squad.member_mut("old", "", &(now - chrono::Duration::days(40)).to_rfc3339());
        squad.member_mut("new", "", &now.to_rfc3339());
        squad.prune(now);
        assert!(squad.members.contains_key("new") && !squad.members.contains_key("old"));
    }

    #[test]
    fn resume_folder_is_found_from_the_transcript_folder_name() {
        let tmp = std::env::temp_dir().join("glowby squad (test)");
        let sub = tmp.join("src").join("deep");
        std::fs::create_dir_all(&sub).unwrap();
        let slug: String = tmp.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        assert_eq!(resume_dir(&sub.to_string_lossy(), &slug), tmp.to_string_lossy());
        assert_eq!(resume_dir(&sub.to_string_lossy(), ""), sub.to_string_lossy(), "unknown: use the session's folder");
        assert_eq!(transcript_folder(r"C:\Users\me\.claude\projects\C--code-app\abc.jsonl").as_deref(), Some("C--code-app"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
