//! Installs / uninstalls Glowby's hooks in ~/.claude/settings.json, safely:
//!
//! 1. preview: build the new file in memory and show you a diff
//! 2. apply: only if the file is unchanged since the preview (token check),
//!    back it up, write atomically, read it back to verify.
//!
//! Glowby's entries are recognised by their command (…\glowby-hook.exe), so
//! uninstall removes only ours and never touches hooks you added yourself.

use crate::settings;
use crate::state::{AppState, lock};
use serde::Serialize;
use serde_json::{Map, Value, json};
use similar::{ChangeTag, TextDiff};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub const HOOK_EXE: &str = "glowby-hook.exe";

/// Events Glowby only listens to. `async: true` means Claude Code doesn't even
/// wait for the hook: zero delay for you.
const LISTEN_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
];
/// Events where Claude Code waits for Glowby's answer (Allow / Deny).
const ANSWER_EVENTS: &[&str] = &["PermissionRequest"];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksStatus {
    /// "installed" | "outdated" | "notInstalled" | "error"
    pub state: &'static str,
    pub detail: Option<String>,
    pub settings_path: String,
    pub hook_path: String,
    pub backups_dir: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub tag: &'static str,
    pub text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub install: bool,
    pub changed: bool,
    pub diff: Vec<DiffLine>,
    pub token: String,
    pub settings_path: String,
    pub reformatted: bool,
}

pub fn claude_settings_path(app: &AppHandle) -> PathBuf {
    let home = app.path().home_dir().unwrap_or_else(|_| PathBuf::from("."));
    home.join(".claude").join("settings.json")
}

/// Stable location the hooks point to, so dev and release builds share one path.
pub fn installed_hook_path(app: &AppHandle) -> PathBuf {
    let base = app.path().local_data_dir().unwrap_or_else(|_| PathBuf::from("."));
    base.join("Glowby").join("bin").join(HOOK_EXE)
}

/// A copy of glowby-hook.exe built into glowby.exe (see build.rs; empty if the
/// hook wasn't built first).
const EMBEDDED_HOOK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/glowby-hook.bin"));

fn bundled_hook_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join(HOOK_EXE);
    candidate.is_file().then_some(candidate)
}

/// The hook program to install: the glowby-hook.exe next to Glowby (fresh in
/// development), else the copy built into glowby.exe (so a lone glowby.exe works).
fn hook_program() -> Option<Vec<u8>> {
    if let Some(bytes) = bundled_hook_path().and_then(|p| std::fs::read(p).ok()) {
        return Some(bytes);
    }
    (!EMBEDDED_HOOK.is_empty()).then(|| EMBEDDED_HOOK.to_vec())
}

/// Installs the hook program into %LOCALAPPDATA%\Glowby\bin (if missing or changed).
pub fn ensure_hook_binary(app: &AppHandle) -> Result<PathBuf, String> {
    let target = installed_hook_path(app);
    if let Some(dir) = target.parent() {
        remove_old_copies(dir);
    }
    let Some(program) = hook_program() else {
        return if target.is_file() {
            Ok(target)
        } else {
            Err(format!("{HOOK_EXE} wasn't found next to Glowby. Build it with `npm run hook`."))
        };
    };
    if std::fs::read(&target).is_ok_and(|current| current == program) {
        return Ok(target);
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = target.with_extension("exe.new");
    std::fs::write(&tmp, &program).map_err(|e| format!("Couldn't copy the hook program: {e}"))?;
    if std::fs::rename(&tmp, &target).is_ok() {
        return Ok(target);
    }
    // Claude Code is probably running a hook right now, so the file is in use and
    // can't be overwritten. Windows does allow RENAMING a running program, so we
    // move the old copy aside (deleted on a later start) and put the new one in place.
    let aside = target.with_file_name(format!("glowby-hook.old-{}.exe", chrono::Local::now().format("%H%M%S%3f")));
    if std::fs::rename(&target, &aside).is_ok() && std::fs::rename(&tmp, &target).is_ok() {
        crate::applog::line("hook program updated (old copy was in use and moved aside)");
        return Ok(target);
    }
    let _ = std::fs::remove_file(&tmp);
    if target.is_file() {
        crate::applog::line("couldn't update the hook program now; the previous copy keeps working");
        Ok(target)
    } else {
        Err("Couldn't install the hook program.".into())
    }
}

/// Deletes copies moved aside during earlier updates (skips any still running).
fn remove_old_copies(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("glowby-hook.old-") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn read_settings(path: &Path) -> Result<(String, Value), String> {
    if !path.exists() {
        return Ok((String::new(), json!({})));
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok((raw, json!({})));
    }
    let value: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("{} isn't valid JSON, so Glowby won't touch it ({e}).", path.display()))?;
    if !value.is_object() {
        return Err(format!("{} doesn't contain a JSON object, so Glowby won't touch it.", path.display()));
    }
    Ok((raw, value))
}

fn token_of(raw: &str) -> String {
    let mut h = DefaultHasher::new();
    raw.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn is_ours(entry: &Value) -> bool {
    entry.get("command").and_then(Value::as_str).is_some_and(|c| c.to_ascii_lowercase().ends_with(HOOK_EXE))
}

/// Removes every Glowby hook entry, then tidies up any empty containers it leaves.
/// `drop_empty_hooks`: also remove a now-empty "hooks" key (uninstall). When
/// reinstalling we keep it, so the key stays where it was in your file.
fn strip_ours(root: &mut Value, drop_empty_hooks: bool) {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else { return };
    for groups in hooks.values_mut() {
        if let Some(groups) = groups.as_array_mut() {
            for group in groups.iter_mut() {
                if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                    list.retain(|entry| !is_ours(entry));
                }
            }
            groups.retain(|group| group.get("hooks").and_then(Value::as_array).is_none_or(|l| !l.is_empty()));
        }
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|g| !g.is_empty()));
    if hooks.is_empty() && drop_empty_hooks {
        // shift_remove keeps the order of your other settings (remove() would swap them)
        root.as_object_mut().map(|o| o.shift_remove("hooks"));
    }
}

fn add_ours(root: &mut Value, hook: &str) {
    let obj = root.as_object_mut().expect("settings root is an object");
    let hooks = obj.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks) = hooks.as_object_mut() else { return };
    let mut add = |event: &str, entry: Value| {
        let groups = hooks.entry(event.to_string()).or_insert_with(|| json!([]));
        if let Some(groups) = groups.as_array_mut() {
            groups.push(json!({ "hooks": [entry] }));
        }
    };
    for event in LISTEN_EVENTS {
        add(event, json!({ "type": "command", "command": hook, "args": [event], "async": true, "timeout": 10 }));
    }
    for event in ANSWER_EVENTS {
        add(event, json!({ "type": "command", "command": hook, "args": [event], "timeout": 600 }));
    }
}

fn pretty(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}

fn transform(app: &AppHandle, install: bool) -> Result<(String, Value, Value), String> {
    let path = claude_settings_path(app);
    let (raw, before) = read_settings(&path)?;
    let mut after = before.clone();
    strip_ours(&mut after, !install);
    if install {
        add_ours(&mut after, &installed_hook_path(app).to_string_lossy());
    }
    Ok((raw, before, after))
}

pub fn preview(app: &AppHandle, install: bool) -> Result<Preview, String> {
    let path = claude_settings_path(app);
    let (raw, before, after) = transform(app, install)?;
    let before_text = if raw.trim().is_empty() { String::new() } else { pretty(&before) };
    let after_text = pretty(&after);
    let diff = TextDiff::from_lines(&before_text, &after_text);
    let mut lines = Vec::new();
    for (i, group) in diff.grouped_ops(3).iter().enumerate() {
        if i > 0 {
            lines.push(DiffLine { tag: "gap", text: "…".into() });
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let tag = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                lines.push(DiffLine { tag, text: change.value().trim_end_matches(['\r', '\n']).to_string() });
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

/// Applies the change you reviewed. Returns the backup path ("" if there was no file).
pub fn apply(app: &AppHandle, install: bool, token: &str) -> Result<String, String> {
    let path = claude_settings_path(app);
    let (raw, _before, after) = transform(app, install)?;
    if token_of(&raw) != token {
        return Err("settings.json changed since you reviewed the diff. Review it again before applying.".into());
    }
    if install {
        ensure_hook_binary(app)?;
    }

    let state = app.state::<AppState>();
    let mut backup = String::new();
    if path.exists() {
        let dir = &state.paths.backups_dir;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let target = dir.join(format!("settings-{stamp}.json"));
        std::fs::copy(&path, &target).map_err(|e| format!("Backup failed, nothing was changed: {e}"))?;
        backup = target.display().to_string();
    }

    settings::save_json(&path, &after).map_err(|e| format!("Couldn't write settings.json: {e}"))?;
    let (_, check) = read_settings(&path)?;
    if check != after {
        return Err(format!("settings.json didn't read back as expected. Your backup: {backup}"));
    }
    lock(&state.ui).hooks_installed = install;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOOK: &str = r"C:\Users\me\AppData\Local\Glowby\bin\glowby-hook.exe";

    #[test]
    fn install_then_uninstall_leaves_your_own_hooks_untouched() {
        let original = json!({
            "theme": "dark",
            "hooks": { "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-check.sh" }] }] }
        });
        let mut v = original.clone();
        add_ours(&mut v, HOOK);
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert!(v.pointer("/hooks/PermissionRequest/0/hooks/0/command").is_some());
        strip_ours(&mut v, true);
        assert_eq!(v, original);
    }

    #[test]
    fn uninstall_removes_the_hooks_key_if_glowby_added_it() {
        let original = json!({ "theme": "dark", "model": "x" });
        let mut v = original.clone();
        add_ours(&mut v, HOOK);
        strip_ours(&mut v, true);
        assert_eq!(v, original);
        assert_eq!(v.to_string(), original.to_string(), "and in the same order");
    }

    #[test]
    fn reinstall_keeps_the_order_of_your_settings() {
        // Map equality ignores order, so compare the text Claude Code would see.
        let mut v = json!({ "hooks": {}, "model": "x", "theme": "dark" });
        add_ours(&mut v, HOOK);
        let installed = v.to_string();
        strip_ours(&mut v, false);
        add_ours(&mut v, HOOK);
        assert_eq!(v.to_string(), installed);
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["hooks", "model", "theme"]);
    }

    #[test]
    fn reinstall_does_not_duplicate() {
        let mut v = json!({});
        add_ours(&mut v, HOOK);
        strip_ours(&mut v, false);
        add_ours(&mut v, HOOK);
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn only_the_permission_hook_makes_claude_wait() {
        let mut v = json!({});
        add_ours(&mut v, HOOK);
        for (event, groups) in v["hooks"].as_object().unwrap() {
            let entry = &groups[0]["hooks"][0];
            assert_eq!(entry["args"][0], event.as_str());
            let waits = entry.get("async").and_then(Value::as_bool) != Some(true);
            assert_eq!(waits, event == "PermissionRequest", "{event}");
        }
    }
}

pub fn status(app: &AppHandle) -> HooksStatus {
    let state = app.state::<AppState>();
    let path = claude_settings_path(app);
    let hook = installed_hook_path(app);
    let mut out = HooksStatus {
        state: "notInstalled",
        detail: None,
        settings_path: path.display().to_string(),
        hook_path: hook.display().to_string(),
        backups_dir: state.paths.backups_dir.display().to_string(),
    };
    let root = match read_settings(&path) {
        Ok((_, root)) => root,
        Err(e) => {
            out.state = "error";
            out.detail = Some(e);
            return out;
        }
    };
    let hook_str = hook.to_string_lossy().to_ascii_lowercase();
    let ours_for = |event: &str| -> (bool, bool) {
        let entries: Vec<&Value> = root
            .pointer(&format!("/hooks/{event}"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|g| g.get("hooks").and_then(Value::as_array))
            .flatten()
            .filter(|e| is_ours(e))
            .collect();
        let current = entries.iter().any(|e| {
            e.get("command").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() == Some(hook_str.as_str())
        });
        (!entries.is_empty(), current)
    };
    let all: Vec<(bool, bool)> = LISTEN_EVENTS.iter().chain(ANSWER_EVENTS).map(|e| ours_for(e)).collect();
    if all.iter().all(|(_, current)| *current) && hook.is_file() {
        out.state = "installed";
    } else if all.iter().any(|(any, _)| *any) {
        out.state = "outdated";
        out.detail = Some("Some Glowby hooks are missing or point to an old location. Update them.".into());
    }
    out
}
