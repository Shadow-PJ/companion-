//! Installs Glowby's hooks for Codex in ~/.codex/hooks.json.
//!
//! The activity hooks run in the background: Glowby receives activity but
//! they never slow Codex down. One hook, PermissionRequest, waits for an
//! answer, so you can Allow / Deny Codex's questions on Glowby or let
//! auto-allow answer them. It fails open: if Glowby is closed, slow, or
//! doesn't answer, Codex shows its normal approval prompt.

use crate::hooks_installer::{self, DiffLine, HooksStatus, Preview};
use crate::settings;
use crate::state::AppState;
use serde_json::{Map, Value, json};
use similar::{ChangeTag, TextDiff};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "SessionEnd",
    "Interrupt",
];

/// Waits for Glowby's answer (same answer format as Claude Code's hook).
const ANSWER_EVENTS: &[&str] = &["PermissionRequest"];

pub fn settings_path(app: &AppHandle) -> PathBuf {
    app.path()
        .home_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".codex")
        .join("hooks.json")
}

fn command_for(hook: &Path, event: &str) -> String {
    // Codex's command hook uses a command string. Quoting the executable path
    // keeps installations working when the Windows user profile contains spaces.
    format!("\"{}\" {event}", hook.display())
}

fn is_ours(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| {
            command
                .to_ascii_lowercase()
                .contains(hooks_installer::HOOK_EXE)
        })
}

fn read(path: &Path) -> Result<(String, Value), String> {
    if !path.exists() {
        return Ok((String::new(), json!({})));
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok((raw, json!({})));
    }
    let value: Value = serde_json::from_str(&raw).map_err(|e| {
        format!(
            "{} isn't valid JSON, so Glowby won't touch it ({e}).",
            path.display()
        )
    })?;
    if !value.is_object() {
        return Err(format!(
            "{} doesn't contain a JSON object, so Glowby won't touch it.",
            path.display()
        ));
    }
    Ok((raw, value))
}

fn token_of(raw: &str) -> String {
    let mut hash = DefaultHasher::new();
    raw.hash(&mut hash);
    format!("{:016x}", hash.finish())
}

fn strip_ours(root: &mut Value, drop_empty: bool) {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };
    for groups in hooks.values_mut() {
        if let Some(groups) = groups.as_array_mut() {
            for group in groups.iter_mut() {
                if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                    list.retain(|entry| !is_ours(entry));
                }
            }
            groups.retain(|group| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_none_or(|list| !list.is_empty())
            });
        }
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
    if hooks.is_empty() && drop_empty {
        root.as_object_mut()
            .map(|object| object.shift_remove("hooks"));
    }
}

fn add_ours(root: &mut Value, hook: &Path) {
    let object = root.as_object_mut().expect("settings root is an object");
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks) = hooks.as_object_mut() else {
        return;
    };
    for event in EVENTS {
        let groups = hooks
            .entry((*event).to_string())
            .or_insert_with(|| json!([]));
        if let Some(groups) = groups.as_array_mut() {
            groups.push(json!({
                "hooks": [{
                    "type": "command",
                    "command": command_for(hook, event),
                    "async": true,
                    "timeout": 10,
                    "statusMessage": "Glowby is updating your pet"
                }]
            }));
        }
    }
    for event in ANSWER_EVENTS {
        let groups = hooks
            .entry((*event).to_string())
            .or_insert_with(|| json!([]));
        if let Some(groups) = groups.as_array_mut() {
            groups.push(json!({
                "hooks": [{
                    "type": "command",
                    "command": command_for(hook, event),
                    "timeout": 600,
                    "statusMessage": "Waiting for your answer on Glowby"
                }]
            }));
        }
    }
}

fn pretty(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}

fn transform(app: &AppHandle, install: bool) -> Result<(String, Value), String> {
    let path = settings_path(app);
    let (raw, before) = read(&path)?;
    let mut after = before;
    strip_ours(&mut after, !install);
    if install {
        add_ours(&mut after, &hooks_installer::installed_hook_path(app));
    }
    Ok((raw, after))
}

pub fn preview(app: &AppHandle, install: bool) -> Result<Preview, String> {
    let path = settings_path(app);
    let (raw, after) = transform(app, install)?;
    let (_, before) = read(&path)?;
    let before_text = if raw.trim().is_empty() {
        String::new()
    } else {
        pretty(&before)
    };
    let after_text = pretty(&after);
    let diff = TextDiff::from_lines(&before_text, &after_text);
    let mut lines = Vec::new();
    for (index, group) in diff.grouped_ops(3).iter().enumerate() {
        if index > 0 {
            lines.push(DiffLine {
                tag: "gap",
                text: "…".into(),
            });
        }
        for operation in group {
            for change in diff.iter_changes(operation) {
                let tag = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                lines.push(DiffLine {
                    tag,
                    text: change.value().trim_end_matches(['\r', '\n']).to_string(),
                });
            }
        }
    }
    Ok(Preview {
        install,
        changed: before_text != after_text,
        diff: lines,
        token: token_of(&raw),
        settings_path: path.display().to_string(),
        reformatted: !raw.trim().is_empty() && raw.replace("\r\n", "\n") != before_text,
    })
}

pub fn apply(app: &AppHandle, install: bool, token: &str) -> Result<String, String> {
    let path = settings_path(app);
    let (raw, after) = transform(app, install)?;
    if token_of(&raw) != token {
        return Err(
            "hooks.json changed since you reviewed the diff. Review it again before applying."
                .into(),
        );
    }
    if install {
        hooks_installer::ensure_hook_binary(app)?;
    }
    let state = app.state::<AppState>();
    let mut backup = String::new();
    if path.exists() {
        let dir = &state.paths.backups_dir;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let target = dir.join(format!(
            "codex-hooks-{}.json",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        std::fs::copy(&path, &target)
            .map_err(|e| format!("Backup failed, nothing was changed: {e}"))?;
        backup = target.display().to_string();
    }
    settings::save_json(&path, &after).map_err(|e| format!("Couldn't write hooks.json: {e}"))?;
    let (_, check) = read(&path)?;
    if check != after {
        return Err(format!(
            "hooks.json didn't read back as expected. Your backup: {backup}"
        ));
    }
    Ok(backup)
}

pub fn status(app: &AppHandle) -> HooksStatus {
    let state = app.state::<AppState>();
    let path = settings_path(app);
    let hook = hooks_installer::installed_hook_path(app);
    let mut output = HooksStatus {
        state: "notInstalled",
        detail: None,
        settings_path: path.display().to_string(),
        hook_path: hook.display().to_string(),
        backups_dir: state.paths.backups_dir.display().to_string(),
    };
    let root = match read(&path) {
        Ok((_, root)) => root,
        Err(error) => {
            output.state = "error";
            output.detail = Some(error);
            return output;
        }
    };
    let complete = EVENTS.iter().chain(ANSWER_EVENTS).all(|event| {
        root.pointer(&format!("/hooks/{event}"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|group| group.get("hooks").and_then(Value::as_array))
            .flatten()
            .any(is_ours)
    });
    if complete && hook.is_file() {
        output.state = "installed";
    } else if EVENTS.iter().any(|event| {
        root.pointer(&format!("/hooks/{event}"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|group| group.get("hooks").and_then(Value::as_array))
            .flatten()
            .any(is_ours)
    }) {
        output.state = "outdated";
        output.detail = Some(
            "Some Glowby Codex hooks are missing (new: answering Codex's permission questions) or point to an old location. Update them, then trust them in Codex with /hooks.".into(),
        );
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn install_then_remove_keeps_other_codex_hooks() {
        let original = json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "my-stop.ps1" }] }] } });
        let mut value = original.clone();
        add_ours(
            &mut value,
            Path::new(r"C:\\Users\\me\\AppData\\Local\\Glowby\\bin\\glowby-hook.exe"),
        );
        strip_ours(&mut value, true);
        assert_eq!(value, original);
    }
    #[test]
    fn only_the_permission_hook_waits() {
        let mut value = json!({});
        add_ours(&mut value, Path::new(r"C:\\Glowby\\glowby-hook.exe"));
        for (event, groups) in value["hooks"].as_object().unwrap() {
            let entry = &groups[0]["hooks"][0];
            if event == "PermissionRequest" {
                assert!(entry.get("async").is_none(), "Codex must wait for the answer");
                assert_eq!(entry["timeout"], 600);
            } else {
                assert_eq!(entry["async"], true, "{event} runs in the background");
            }
        }
    }
}
