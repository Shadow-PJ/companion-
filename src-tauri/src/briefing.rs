//! Daily briefing: shown on your first activity of the day.
//! "What you did yesterday, what's unfinished, one small next step",
//! worked out from git (run once a day, in the background).

use crate::projects::{self, git};
use crate::state::{self, AppState, lock};
use crate::quests;
use chrono::{Duration as ChronoDuration, Local, Timelike};
use serde::Serialize;
use tauri::{AppHandle, Manager};

const MAX_PROJECTS: usize = 5;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub text: String,
    #[serde(skip)]
    pub prompt: String,
    #[serde(skip)]
    pub dir: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BriefingView {
    pub greeting: String,
    /// "Yesterday" or "This week" (when yesterday had no commits).
    pub period: String,
    pub done: Vec<String>,
    pub unfinished: Vec<String>,
    pub suggestion: Option<Suggestion>,
}

struct ProjectFacts {
    root: String,
    name: String,
    commits: Vec<String>,
    uncommitted: usize,
    unpushed: usize,
    failing: Option<String>,
    todo: Option<String>,
    recent_file: Option<String>,
}

/// Called on every activity; starts the briefing once per day.
pub fn maybe_start_day(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.settings().briefing.enabled || Local::now().hour() < 4 {
        return;
    }
    let today = Local::now().format("%Y-%m-%d").to_string();
    let first_today = quests::with_book(app, |book| {
        let first = book.briefing_day != today;
        book.briefing_day = today.clone();
        first
    });
    if first_today {
        show(app.clone());
    }
}

/// Builds the briefing in the background, then shows it.
pub fn show(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let view = build(&app).await;
        let state = app.state::<AppState>();
        lock(&state.ui).briefing = Some(view);
        state::hold_out(&app, 60);
        crate::sounds::play(&app, crate::sounds::Sound::Notice);
        crate::pet_window::show(&app);
        state::publish(&app);
    });
}

fn greeting() -> String {
    match Local::now().hour() {
        4..=11 => "Good morning!",
        12..=17 => "Good afternoon!",
        _ => "Good evening!",
    }
    .to_string()
}

async fn facts_for(app: &AppHandle, root: &str, since: &str, until: &str, email: &str) -> ProjectFacts {
    let mut log_args = vec!["log", "--no-merges", "--pretty=format:%s"];
    let since_arg = format!("--since={since}");
    let until_arg = format!("--until={until}");
    let author_arg = format!("--author={email}");
    log_args.push(&since_arg);
    log_args.push(&until_arg);
    if !email.is_empty() {
        log_args.push(&author_arg);
    }
    let commits: Vec<String> = git(root, &log_args).await.unwrap_or_default().lines().map(str::to_string).filter(|l| !l.is_empty()).collect();
    let uncommitted = git(root, &["status", "--porcelain"]).await.map(|s| s.lines().count()).unwrap_or(0);
    let unpushed = git(root, &["rev-list", "--count", "@{u}..HEAD"]).await.and_then(|s| s.parse().ok()).unwrap_or(0);
    let failing = lock(&app.state::<AppState>().health)
        .failures
        .values()
        .find(|f| f.project_dir.eq_ignore_ascii_case(root) || f.project_dir.to_lowercase().starts_with(&root.to_lowercase()))
        .map(|f| f.kind.noun().to_string());
    // Files from the latest commit: look for a TODO there, or suggest a test.
    let changed: Vec<String> = git(root, &["show", "--name-only", "--pretty=format:", "HEAD"])
        .await
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .filter(|l| !l.is_empty())
        .take(20)
        .collect();
    let mut todo = None;
    if !changed.is_empty() {
        let mut args = vec!["grep", "-n", "-I", "-E", "TODO|FIXME", "--"];
        args.extend(changed.iter().map(String::as_str));
        todo = git(root, &args).await.and_then(|out| out.lines().next().map(|l| l.splitn(3, ':').take(2).collect::<Vec<_>>().join(":")));
    }
    let recent_file = changed.into_iter().find(|f| !f.to_lowercase().contains("test") && !f.ends_with(".md") && !f.ends_with(".json") && !f.ends_with(".lock"));
    ProjectFacts { root: root.into(), name: projects::name(root), commits, uncommitted, unpushed, failing, todo, recent_file }
}

pub async fn build(app: &AppHandle) -> BriefingView {
    let state = app.state::<AppState>();
    let roots = lock(&state.projects).recent(MAX_PROJECTS);
    let now = Local::now();
    let today = now.date_naive().and_hms_opt(0, 0, 0).unwrap_or_default();
    let yesterday = today - ChronoDuration::days(1);
    let fmt = |d: chrono::NaiveDateTime| d.format("%Y-%m-%d %H:%M").to_string();
    let email = match roots.first() {
        Some(r) => git(r, &["config", "user.email"]).await.unwrap_or_default(),
        None => String::new(),
    };

    let mut facts = Vec::new();
    for root in &roots {
        facts.push(facts_for(app, root, &fmt(yesterday), &fmt(today), &email).await);
    }
    let mut period = "Yesterday".to_string();
    if facts.iter().all(|f| f.commits.is_empty()) {
        // Nothing yesterday (weekend?): look at the whole past week instead.
        period = "This week".into();
        let week = fmt(today - ChronoDuration::days(7));
        let until = fmt(now.naive_local());
        for f in facts.iter_mut() {
            let mut args = vec!["log".to_string(), "--no-merges".into(), "--pretty=format:%s".into(), format!("--since={week}"), format!("--until={until}")];
            if !email.is_empty() {
                args.push(format!("--author={email}"));
            }
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            f.commits = git(&f.root, &refs).await.unwrap_or_default().lines().map(str::to_string).filter(|l| !l.is_empty()).collect();
        }
    }

    let mut done = Vec::new();
    for f in facts.iter().filter(|f| !f.commits.is_empty()) {
        let shown: Vec<&str> = f.commits.iter().take(3).map(String::as_str).collect();
        let more = if f.commits.len() > 3 { format!(" (+{} more)", f.commits.len() - 3) } else { String::new() };
        done.push(format!("{}: {}{more}", f.name, shown.join("; ")));
    }
    let mut unfinished = Vec::new();
    for f in &facts {
        if let Some(kind) = &f.failing {
            unfinished.push(format!("{}: the {kind} are failing", f.name));
        }
        if f.uncommitted > 0 {
            unfinished.push(format!("{}: {} uncommitted file{}", f.name, f.uncommitted, if f.uncommitted == 1 { "" } else { "s" }));
        }
        if f.unpushed > 0 {
            unfinished.push(format!("{}: {} unpushed commit{}", f.name, f.unpushed, if f.unpushed == 1 { "" } else { "s" }));
        }
    }
    BriefingView { greeting: greeting(), period, done, unfinished, suggestion: suggest(&facts) }
}

/// One small next step, by priority.
fn suggest(facts: &[ProjectFacts]) -> Option<Suggestion> {
    let s = |text: String, prompt: String, f: &ProjectFacts| Some(Suggestion { text, prompt, dir: f.root.clone() });
    if let Some(f) = facts.iter().find(|f| f.failing.is_some()) {
        let kind = f.failing.clone().unwrap_or_default();
        return s(
            format!("Get the {kind} passing in {}.", f.name),
            format!("My {kind} are failing in this project. Run them, find out why, fix it with the smallest change, and run them again to confirm."),
            f,
        );
    }
    if let Some(f) = facts.iter().find(|f| f.uncommitted > 0) {
        return s(
            format!("Commit your {} changed file{} in {}.", f.uncommitted, if f.uncommitted == 1 { "" } else { "s" }, f.name),
            "Look at my uncommitted changes (git status and git diff). Commit them with a clear message: a short summary line, then what changed and why. Don't push.".into(),
            f,
        );
    }
    if let Some(f) = facts.iter().find(|f| f.unpushed > 0) {
        return s(
            format!("Push your {} commit{} in {}.", f.unpushed, if f.unpushed == 1 { "" } else { "s" }, f.name),
            "Push my local commits to the remote branch. If the push is rejected, explain why instead of forcing it.".into(),
            f,
        );
    }
    if let Some(f) = facts.iter().find(|f| f.todo.is_some()) {
        let at = f.todo.clone().unwrap_or_default();
        return s(
            format!("Tackle the TODO at {at} in {}.", f.name),
            format!("Look at the TODO/FIXME at {at}. If it's small, do it and explain the change; otherwise tell me what it would take."),
            f,
        );
    }
    if let Some(f) = facts.iter().find(|f| f.recent_file.is_some()) {
        let file = f.recent_file.clone().unwrap_or_default();
        return s(
            format!("Write one small test for {file} in {}.", f.name),
            format!("Write one small, focused test for {file}, following this project's existing test style, and run it."),
            f,
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(name: &str) -> ProjectFacts {
        ProjectFacts {
            root: format!("C:\\code\\{name}"),
            name: name.into(),
            commits: vec![],
            uncommitted: 0,
            unpushed: 0,
            failing: None,
            todo: None,
            recent_file: Some("src/lib.rs".into()),
        }
    }

    #[test]
    fn failing_checks_come_first_then_commits_then_pushes() {
        let mut a = facts("a");
        a.uncommitted = 3;
        let mut b = facts("b");
        b.failing = Some("tests".into());
        let s = suggest(&[a, b]).unwrap();
        assert!(s.text.contains("tests passing in b"));
        assert_eq!(s.dir, "C:\\code\\b");

        let mut c = facts("c");
        c.unpushed = 2;
        let mut d = facts("d");
        d.uncommitted = 1;
        assert!(suggest(&[c, d]).unwrap().text.starts_with("Commit your 1 changed file"));
    }

    #[test]
    fn falls_back_to_a_small_test() {
        assert!(suggest(&[facts("x")]).unwrap().text.contains("test for src/lib.rs"));
        let mut none = facts("y");
        none.recent_file = None;
        assert!(suggest(&[none]).is_none());
    }
}
