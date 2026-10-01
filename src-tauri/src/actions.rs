//! Quick actions (right-click menu), the last error Glowby noticed, and
//! "offers" like "That looks like an error. Fix it?".

use crate::chat::{self, ChatRequest};
use crate::state::{self, AppState, lock};
use crate::{health, pet_window, settings};
use serde::Serialize;
use tauri::{AppHandle, Manager};

/// The most recent error Glowby noticed (failed command, failing tests, or a
/// copied error). Kept in memory only, never written to disk.
#[derive(Clone, Debug)]
pub struct LastError {
    pub text: String,
    pub source: String,
}

/// A suggestion with buttons (Fix it / Explain / Dismiss).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    /// "clipboardError" or "failingChecks"
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
    pub project: String,
}

const MAX_ERROR_CHARS: usize = 6000;

const FIX_TEMPLATE: &str = "Fix this error in my project. Find the cause, make the smallest change that fixes it, verify it if you can, and tell me what you changed.\n\n{last_error}";
const EXPLAIN_TEMPLATE: &str = "Explain this error in plain language: what went wrong, why, and the smallest fix. Don't change any files.\n\n{last_error}";

pub fn remember_error(app: &AppHandle, text: &str, source: String) {
    let text = crate::sessions::shorten(text.trim(), MAX_ERROR_CHARS);
    if !text.is_empty() {
        *lock(&app.state::<AppState>().last_error) = Some(LastError { text, source });
    }
}

/// Fills in a prompt template's placeholders.
pub fn expand(template: &str, last_error: Option<&LastError>, project: &str, today: &str) -> String {
    let error_block = match last_error {
        Some(e) => format!("Here is the most recent error Glowby saw (from {}):\n```\n{}\n```", e.source, e.text),
        None => "Glowby hasn't seen an error yet, so find the most recent one yourself (recent test or build output, logs).".to_string(),
    };
    template
        .replace("{last_error}", &error_block)
        .replace("{project}", if project.is_empty() { "this project" } else { project })
        .replace("{today}", today)
}

pub fn run_action(app: &AppHandle, id: &str) -> Result<(), String> {
    let settings = app.state::<AppState>().settings();
    if !settings.quick_actions.enabled {
        return Err("Quick actions are turned off in Settings.".into());
    }
    let action = settings
        .quick_actions
        .actions
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or("That quick action no longer exists.")?;
    run_prompt(app, action.label, &action.prompt, action.read_only);
    Ok(())
}

/// Opens the chat and sends a filled-in prompt to Claude Code.
pub fn run_prompt(app: &AppHandle, title: String, template: &str, read_only: bool) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let (dir, _) = chat::effective_dir(&state, &settings);
    let last = lock(&state.last_error).clone();
    let today = chrono::Local::now().format("%A %Y-%m-%d").to_string();
    let message = expand(template, last.as_ref(), &crate::sessions::project_name(&dir), &today);
    lock(&state.ui).chat_open = true;
    pet_window::show(app);
    pet_window::focus_for_typing(app);
    state::publish(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = chat::send(app, ChatRequest { message, title, read_only }).await;
    });
}

/// A button on an offer was clicked.
pub fn offer_choice(app: &AppHandle, choice: &str) {
    let state = app.state::<AppState>();
    let clipboard_offer = lock(&state.ui).offer.take().map(|(offer, _)| offer);
    let failing = if clipboard_offer.is_none() { lock(&state.health).latest().cloned() } else { None };
    match (choice, failing) {
        ("dismiss", Some(_)) => {
            lock(&state.health).clear();
            save_health(app);
        }
        ("fix", Some(f)) => {
            let template = format!(
                "My {} fail when I run `{}` in this project. Run it, find out why, fix it with the smallest change, and run it again to confirm.\n\n{{last_error}}",
                f.kind.noun(),
                f.command
            );
            run_prompt(app, format!("Fix failing {}", f.kind.noun()), &template, false);
        }
        ("explain", Some(f)) => {
            let template = format!(
                "My {} fail when I run `{}`. Explain in plain language why, and what the smallest fix would be. Don't change any files.\n\n{{last_error}}",
                f.kind.noun(),
                f.command
            );
            run_prompt(app, format!("Why the {} fail", f.kind.noun()), &template, true);
        }
        ("fix", None) => run_prompt(app, "Fix this error".into(), FIX_TEMPLATE, false),
        ("explain", None) => run_prompt(app, "Explain this error".into(), EXPLAIN_TEMPLATE, true),
        _ => {}
    }
    state::publish(app);
}

pub fn save_health(app: &AppHandle) {
    let state = app.state::<AppState>();
    let health = lock(&state.health);
    if let Err(e) = settings::save_json(&state.paths.health_file, &*health) {
        crate::applog::line(format!("couldn't save health.json: {e}"));
    }
}

/// Feeds a PostToolUse / PostToolUseFailure event into the error memory and
/// the test/build health. Returns a toast to show, if the state changed.
pub fn on_tool_result(app: &AppHandle, event: &str, payload: &serde_json::Value) -> Option<(&'static str, String)> {
    let state = app.state::<AppState>();
    let field = |k: &str| payload.get(k).and_then(serde_json::Value::as_str).unwrap_or("");
    if event == "PostToolUseFailure" && !payload.get("is_interrupt").and_then(serde_json::Value::as_bool).unwrap_or(false) {
        let what = match payload.pointer("/tool_input/command").and_then(serde_json::Value::as_str) {
            Some(cmd) => format!("`{}`", crate::sessions::shorten(cmd, 80)),
            None => format!("the {} tool", field("tool_name")),
        };
        remember_error(app, field("error"), format!("running {what}"));
    }
    if !lock(&state.settings).health {
        return None;
    }
    let check = health::evaluate(event, payload)?;
    let project_dir = field("cwd").to_string();
    let change = lock(&state.health).record(&project_dir, &check);
    if !check.passed
        && let Some(output) = &check.output
    {
        remember_error(app, output, format!("failing {}: `{}`", check.kind.noun(), crate::sessions::shorten(&check.command, 80)));
    }
    let project = crate::sessions::project_name(&project_dir);
    let toast = match change? {
        health::Change::NowFailing(f) => ("failed", format!("The {} are failing in {project}.", if f.kind == health::CheckKind::Tests { "tests" } else { "build steps" })),
        health::Change::NowPassing(kind) => ("done", format!("{} pass again in {project}. Feeling better!", if kind == health::CheckKind::Tests { "Tests" } else { "The build steps" })),
    };
    save_health(app);
    Some(toast)
}

/// Shell commands Glowby's chat may run without asking, because they only read.
/// Anything with chaining, redirection or substitution is never auto-allowed.
pub fn is_safe_read_command(command: &str) -> bool {
    let c = command.trim();
    if c.is_empty() || c.contains(['\n', ';', '&', '|', '>', '<', '`', '$']) || c.contains("--output") {
        return false;
    }
    const SAFE: &[&str] = &[
        "git status", "git diff", "git log", "git show", "git rev-parse", "git ls-files", "git blame",
        "git branch --show-current", "git remote -v", "ls", "dir", "pwd", "Get-ChildItem", "Get-Location",
    ];
    SAFE.iter().any(|p| c == *p || c.starts_with(&format!("{p} ")))
        || c == "git branch"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_filled() {
        let e = LastError { text: "boom".into(), source: "running `cargo test`".into() };
        let out = expand("Explain {last_error} in {project} on {today}", Some(&e), "glowby", "Monday");
        assert!(out.contains("boom") && out.contains("cargo test") && out.contains("glowby") && out.contains("Monday"));
        let none = expand("{last_error}", None, "", "x");
        assert!(none.contains("hasn't seen an error"));
    }

    #[test]
    fn only_plain_read_commands_are_auto_allowed() {
        assert!(is_safe_read_command("git log --since=midnight --oneline"));
        assert!(is_safe_read_command("git status"));
        assert!(is_safe_read_command("git diff --stat"));
        assert!(!is_safe_read_command("git log; rm -rf /"));
        assert!(!is_safe_read_command("git diff > out.txt"));
        assert!(!is_safe_read_command("git diff --output=x"));
        assert!(!is_safe_read_command("git branch -D main"));
        assert!(!is_safe_read_command("git commit -m x"));
        assert!(!is_safe_read_command("lsof"));
        assert!(!is_safe_read_command("git log $(whoami)"));
    }

    #[test]
    fn default_actions_are_valid() {
        let actions = settings::default_quick_actions();
        assert_eq!(actions.len(), 4);
        assert!(actions.iter().all(|a| !a.id.is_empty() && !a.label.is_empty() && !a.prompt.is_empty()));
    }
}
