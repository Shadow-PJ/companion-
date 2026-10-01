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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct PetSettings {
    /// Monitor name as Windows reports it; empty = primary monitor.
    pub monitor: String,
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
        }
    }
}

impl Default for PetSettings {
    fn default() -> Self {
        Self {
            monitor: String::new(),
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
