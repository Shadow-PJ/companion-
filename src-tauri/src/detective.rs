//! The token detective inside Glowby:
//! * the weekly **case report**: what ate your Claude limits this week, from
//!   your local logs (the analysis itself lives in crates/glowby-detective);
//! * the live checks that act for you (cold-chat guard, cache reminder, big
//!   chat helper) are in guard.rs.
//!
//! Only numbers, times, tool names and file paths are read; nothing is sent anywhere.

use crate::settings;
use crate::state::{self, AppState, lock};
use glowby_detective::Report;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const WEEK_SECS: i64 = 7 * 86_400;

fn now_secs() -> i64 {
    chrono::Local::now().timestamp()
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Detective {
    pub last_report_at: i64,
    pub last_text: String,
    pub last: Option<Report>,
    #[serde(skip)]
    pub running: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FindingView {
    pub title: String,
    pub detail: String,
    pub tip: String,
    pub share: String,
    /// Offer "Ask Claude how to split it" (a file that keeps being re-read).
    pub can_fix: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CaseView {
    pub running: bool,
    pub period: String,
    pub summary: String,
    pub findings: Vec<FindingView>,
    pub footnote: String,
}

// ---------------------------------------------------------------- the case report

fn save(app: &AppHandle) {
    let state = app.state::<AppState>();
    let d = lock(&state.detective);
    if let Err(e) = settings::save_json(&state.paths.detective_file, &*d) {
        crate::applog::line(format!("couldn't save detective.json: {e}"));
    }
}

/// Makes a case report for the last 7 days (in the background). `show` opens it on Glowby.
pub fn run(app: &AppHandle, show: bool) {
    let state = app.state::<AppState>();
    {
        let mut d = lock(&state.detective);
        if d.running {
            return;
        }
        d.running = true;
    }
    if show {
        lock(&state.ui).case_open = true;
        crate::pet_window::show(app);
    }
    state::publish(app);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let now = now_secs();
        let report = glowby_detective::claude_projects_dir().map(|root| glowby_detective::analyze_dir(&root, now - WEEK_SECS, now));
        let state = app.state::<AppState>();
        {
            let mut d = lock(&state.detective);
            d.running = false;
            if let Some(r) = report {
                d.last_text = glowby_detective::render_text(&r);
                d.last = Some(r);
                d.last_report_at = now;
            }
        }
        save(&app);
        let first = {
            let mut p = lock(&state.progress);
            p.stats.case_reports += 1;
            p.stats.case_reports == 1
        };
        crate::progress::save_now(&app);
        if first {
            state::toast(&app, "levelup", "Case closed! Unlocked: Detective hat.".into(), String::new(), 8);
        }
        if show {
            state::hold_out(&app, 60);
            crate::sounds::play(&app, crate::sounds::Sound::Notice);
        }
        crate::applog::line("detective: case report made");
        state::publish(&app);
    });
}

/// On your first activity of a new week: a fresh case report.
pub fn maybe_weekly(app: &AppHandle) {
    let state = app.state::<AppState>();
    let s = state.settings();
    if !s.detective.enabled || !s.detective.weekly || lock(&state.ui).game_active {
        return;
    }
    let due = now_secs() - lock(&state.detective).last_report_at >= WEEK_SECS;
    if due {
        run(app, true);
    }
}

pub fn close(app: &AppHandle) {
    lock(&app.state::<AppState>().ui).case_open = false;
    state::publish(app);
}

/// "Ask Claude how to split it" on a re-read file.
pub fn fix(app: &AppHandle, index: usize) -> Result<(), String> {
    let state = app.state::<AppState>();
    let finding = lock(&state.detective).last.as_ref().and_then(|r| r.findings.get(index).cloned()).ok_or("That finding is gone.")?;
    let (Some(file), Some(dir)) = (finding.file, finding.project_dir) else {
        return Err("Nothing to fix automatically for this one; the tip says what helps.".into());
    };
    let template = format!(
        "Claude keeps re-reading {file} in full. Look at it and suggest how to split it into smaller, focused files, or what to note in CLAUDE.md so it doesn't need to be re-read. Don't change anything yet; just explain the plan briefly."
    );
    lock(&state.ui).case_open = false;
    crate::actions::run_prompt_in(app, Some(dir), "Plan: split a big file".into(), &template, true);
    Ok(())
}

pub fn view(app: &AppHandle) -> Option<CaseView> {
    let state = app.state::<AppState>();
    if !lock(&state.ui).case_open {
        return None;
    }
    let d = lock(&state.detective);
    let date = |t: i64| chrono::DateTime::from_timestamp(t, 0).map(|x| x.with_timezone(&chrono::Local).format("%a %d %b").to_string()).unwrap_or_default();
    let Some(r) = d.last.as_ref() else {
        return Some(CaseView { running: true, period: String::new(), summary: "Investigating your logs…".into(), findings: Vec::new(), footnote: String::new() });
    };
    Some(CaseView {
        running: d.running,
        period: format!("{} – {}", date(r.since), date(r.until)),
        summary: format!("{} sessions in {} projects, {} replies.", r.sessions, r.projects, r.replies),
        findings: r
            .findings
            .iter()
            .map(|f| FindingView {
                title: f.title.clone(),
                detail: f.detail.clone(),
                tip: f.tip.clone(),
                share: format!("≈ {:.0}% of your week", f.share),
                can_fix: f.file.is_some() && f.project_dir.is_some(),
            })
            .collect(),
        footnote: format!(
            "Not counted: {} normal cache rebuilds and {} caused by Claude Code itself. Estimates from your local logs; nothing was sent anywhere.",
            r.cache.normal_rebuilds, r.cache.other_rebuilds
        ),
    })
}
