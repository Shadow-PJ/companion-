//! Glowby's own settings, stored as JSON in %APPDATA%\dev.glowby.app\settings.json.
//! Every feature has its own on/off switch here.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub pet: PetSettings,
    pub permissions: PermissionSettings,
    pub chat: ChatSettings,
    /// Hide the pet while a fullscreen app or game runs.
    pub game_mode: bool,
    /// Right-click menu with your quick actions.
    pub quick_actions: QuickActionSettings,
    /// Drop files on Glowby to send them to Claude Code.
    pub drop_files: bool,
    /// Opt-in: notice errors you copy to the clipboard (processed locally only).
    pub error_watcher: bool,
    /// Look sick while tests or builds fail.
    pub health: bool,
    /// XP, levels, evolution, cosmetics, streak.
    pub progression: ProgressionSettings,
    pub breaks: BreakSettings,
    pub sounds: SoundSettings,
    pub briefing: BriefingSettings,
    pub learn: LearnSettings,
    pub quests: QuestSettings,
    pub github: GithubSettings,
    pub squad: SquadSettings,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct SquadSettings {
    /// Off by default: one small pet per running Claude Code session.
    pub enabled: bool,
    /// How many squad pets fit next to Glowby (1–6).
    pub max_shown: u32,
}

impl Default for SquadSettings {
    fn default() -> Self {
        Self { enabled: false, max_shown: 6 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct BriefingSettings {
    pub enabled: bool,
}

impl Default for BriefingSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct LearnSettings {
    pub enabled: bool,
    /// At most one question this often (minutes).
    pub every_mins: u32,
}

impl Default for LearnSettings {
    fn default() -> Self {
        Self { enabled: true, every_mins: 30 }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct QuestSettings {
    pub enabled: bool,
    pub difficulty: Difficulty,
    pub per_day: u32,
    /// Quest types that may be picked (see quests.rs).
    pub kinds: Vec<String>,
}

impl Default for QuestSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            difficulty: Difficulty::Normal,
            per_day: 3,
            kinds: ["fix", "test", "minutes", "tasks", "commit", "learn", "break", "pet"].map(String::from).to_vec(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct GithubSettings {
    /// Off by default. The token itself lives in Windows Credential Manager.
    pub enabled: bool,
    pub every_mins: u32,
}

impl Default for GithubSettings {
    fn default() -> Self {
        Self { enabled: false, every_mins: 15 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct ProgressionSettings {
    /// Earn XP and level up.
    pub enabled: bool,
    /// Get sleepy and pale when ignored for days (never dies).
    pub neglect: bool,
    /// Equipped hat id ("" = none), colour id and aura id ("" = none).
    pub hat: String,
    pub color: String,
    pub aura: String,
    /// Anime pet ("neko", "kitsune" …; "" = Glowby the jellyfish).
    pub pet: String,
}

impl Default for ProgressionSettings {
    fn default() -> Self {
        Self { enabled: true, neglect: true, hat: String::new(), color: "periwinkle".into(), aura: String::new(), pet: String::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct BreakSettings {
    pub enabled: bool,
    /// Remind after this many minutes of continuous coding.
    pub interval_mins: u32,
}

impl Default for BreakSettings {
    fn default() -> Self {
        Self { enabled: true, interval_mins: 60 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct SoundSettings {
    pub enabled: bool,
    /// 0–100
    pub volume: u32,
    pub task_done: bool,
    pub needs_you: bool,
    pub problems: bool,
    pub level_up: bool,
    pub breaks: bool,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self { enabled: true, volume: 40, task_done: true, needs_you: true, problems: true, level_up: true, breaks: true }
    }
}

/// One entry in the right-click menu. The prompt may use placeholders:
/// {last_error}, {project}, {today}.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct QuickAction {
    pub id: String,
    pub label: String,
    pub prompt: String,
    /// Only read and explain: Glowby blocks file edits for this action.
    pub read_only: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct QuickActionSettings {
    pub enabled: bool,
    pub actions: Vec<QuickAction>,
}

impl Default for QuickActionSettings {
    fn default() -> Self {
        Self { enabled: true, actions: default_quick_actions() }
    }
}

pub fn default_quick_actions() -> Vec<QuickAction> {
    let action = |id: &str, label: &str, read_only: bool, prompt: &str| QuickAction {
        id: id.into(),
        label: label.into(),
        prompt: prompt.into(),
        read_only,
    };
    vec![
        action(
            "explain-error",
            "Explain the last error",
            true,
            "Explain the last error in this project in plain language: what went wrong, why it happened, and the smallest fix. Don't change any files.\n\n{last_error}",
        ),
        action(
            "run-and-fix",
            "Run my project and fix what breaks",
            false,
            "Find out how this project is built and run (README, package.json, Cargo.toml, Makefile, …). Run it. If something breaks, fix it with the smallest change that works, run it again to confirm, then summarize what you changed.",
        ),
        action(
            "commit",
            "Commit my work with a good message",
            false,
            "Look at my uncommitted changes (git status and git diff). Commit them with a clear message: a short summary line, then a few lines on what changed and why. Don't push, and don't commit secrets or build output.",
        ),
        action(
            "today",
            "What did I change today?",
            true,
            "Summarize what changed in this project today ({today}): commits since midnight (git log) plus any uncommitted changes. Group related changes and keep it short.",
        ),
    ]
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct PetSettings {
    /// Monitor name as Windows reports it; empty = primary monitor.
    pub monitor: String,
    /// An imported character to show instead of the jellyfish ("" = Glowby himself).
    pub character: String,
    /// Horizontal position along the top edge: 0.0 = far left, 1.0 = far right.
    pub position: f64,
    /// Thin coloured line at the top edge while Glowby is hidden (blue = working, amber = needs you).
    pub status_line: bool,
    pub follow_mouse: bool,
    pub show_on_permission: bool,
    pub show_on_done: bool,
    pub show_on_attention: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct PermissionSettings {
    /// Show Claude Code's permission questions on Glowby (Allow / Deny).
    pub enabled: bool,
    /// After this many seconds without an answer, the question goes back to the terminal.
    pub timeout_secs: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChatMode {
    /// Edits and commands ask you on Glowby first.
    Ask,
    /// Claude may only read (plan mode).
    ReadOnly,
    /// File edits are auto-accepted; commands still ask you.
    AcceptEdits,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct ChatSettings {
    pub enabled: bool,
    pub project_dir: String,
    pub mode: ChatMode,
    /// Continue one conversation per project folder (vs. a fresh start every message).
    pub keep_conversation: bool,
    /// Override for claude.exe; empty = find it automatically.
    pub claude_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pet: PetSettings::default(),
            permissions: PermissionSettings::default(),
            chat: ChatSettings::default(),
            game_mode: true,
            quick_actions: QuickActionSettings::default(),
            drop_files: true,
            error_watcher: false,
            health: true,
            progression: ProgressionSettings::default(),
            breaks: BreakSettings::default(),
            sounds: SoundSettings::default(),
            briefing: BriefingSettings::default(),
            learn: LearnSettings::default(),
            quests: QuestSettings::default(),
            github: GithubSettings::default(),
            squad: SquadSettings::default(),
        }
    }
}

impl Default for PetSettings {
    fn default() -> Self {
        Self {
            monitor: String::new(),
            character: String::new(),
            position: 0.5,
            status_line: true,
            follow_mouse: true,
            show_on_permission: true,
            show_on_done: true,
            show_on_attention: true,
        }
    }
}

impl Default for PermissionSettings {
    fn default() -> Self {
        Self { enabled: true, timeout_secs: 60 }
    }
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            project_dir: String::new(),
            mode: ChatMode::Ask,
            keep_conversation: true,
            claude_path: String::new(),
        }
    }
}

impl Settings {
    /// Clamp values a hand-edited file could break.
    pub fn sanitized(mut self) -> Self {
        self.pet.position = self.pet.position.clamp(0.0, 1.0);
        self.permissions.timeout_secs = self.permissions.timeout_secs.clamp(5, 540);
        self.breaks.interval_mins = self.breaks.interval_mins.clamp(15, 240);
        self.sounds.volume = self.sounds.volume.min(100);
        self.learn.every_mins = self.learn.every_mins.clamp(5, 240);
        self.quests.per_day = self.quests.per_day.clamp(1, 5);
        self.github.every_mins = self.github.every_mins.clamp(5, 180);
        self.squad.max_shown = self.squad.max_shown.clamp(1, 6);
        let mut seen = std::collections::HashSet::new();
        self.quick_actions.actions.truncate(20);
        for (i, action) in self.quick_actions.actions.iter_mut().enumerate() {
            action.label = action.label.trim().chars().take(60).collect();
            action.prompt = action.prompt.chars().take(4000).collect();
            if action.id.trim().is_empty() || !seen.insert(action.id.clone()) {
                action.id = format!("action-{i}-{}", seen.len());
                seen.insert(action.id.clone());
            }
        }
        self
    }
}

/// Reads a JSON file; a missing or broken file gives the default value.
pub fn load_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Atomic save: write a temp file, then rename it over the real one. If the PC
/// loses power mid-write, you keep either the old file or the new one, never half.
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Remembers the chat session id per project folder, so conversations continue.
pub type ChatSessions = HashMap<String, String>;
