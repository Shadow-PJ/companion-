//! Optional GitHub CI check (off by default).
//!
//! * The token is stored in Windows Credential Manager (CredWrite/CredRead),
//!   never in a file. Settings never shows it again after you save it.
//! * Only while turned on: every N minutes (and a few minutes after a
//!   `git push`) Glowby asks api.github.com for the latest Actions run of the
//!   current branch in your recent GitHub projects.
//! * A failed run: Glowby gets sick and offers Open on GitHub / Fix it.

use crate::actions::Offer;
use crate::projects::{self, git};
use crate::state::{self, AppState, lock};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
};

const TARGET: &str = "Glowby/GitHubToken";
const MAX_REPOS: usize = 5;
const CI_OFFER_STAYS: Duration = Duration::from_secs(6 * 3600);

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    pub repo: String,
    pub branch: String,
    /// "success", "failure", "running", "none" or an error message.
    pub state: String,
    pub url: String,
    pub checked: String,
}

#[derive(Default)]
pub struct Github {
    notified: HashSet<u64>,
    failing: HashMap<String, u64>,
    pub last: Vec<RepoStatus>,
    pub wake: Arc<Notify>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatus {
    pub has_token: bool,
    pub repos: Vec<RepoStatus>,
}

// ---------------------------------------------------------------- Credential Manager

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn save_token(token: &str) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("The token is empty.".into());
    }
    write_secret(TARGET, token)
}

pub fn load_token() -> Option<String> {
    read_secret(TARGET)
}

pub fn delete_token() {
    delete_secret(TARGET);
}

fn write_secret(name: &str, secret: &str) -> Result<(), String> {
    let target = wide(name);
    let user = wide("glowby");
    let blob = secret.as_bytes();
    let mut cred: CREDENTIALW = unsafe { std::mem::zeroed() };
    cred.Type = CRED_TYPE_GENERIC;
    cred.TargetName = target.as_ptr() as *mut u16;
    cred.UserName = user.as_ptr() as *mut u16;
    cred.CredentialBlobSize = blob.len() as u32;
    cred.CredentialBlob = blob.as_ptr() as *mut u8;
    cred.Persist = CRED_PERSIST_LOCAL_MACHINE;
    if unsafe { CredWriteW(&cred, 0) } == 0 {
        return Err("Windows Credential Manager refused to store the token.".into());
    }
    Ok(())
}

fn read_secret(name: &str) -> Option<String> {
    let target = wide(name);
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) == 0 || cred.is_null() {
            return None;
        }
        let c = &*cred;
        let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec();
        CredFree(cred as *const _);
        String::from_utf8(bytes).ok().filter(|t| !t.is_empty())
    }
}

fn delete_secret(name: &str) {
    let target = wide(name);
    unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
}

// ---------------------------------------------------------------- checking

/// Starts the background loop. It sleeps when the check is off.
pub fn spawn(app: AppHandle) {
    let wake = lock(&app.state::<AppState>().github).wake.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let settings = app.state::<AppState>().settings();
            let active = settings.github.enabled && load_token().is_some();
            if active {
                check(&app).await;
                let every = Duration::from_secs(settings.github.every_mins as u64 * 60);
                tokio::select! {
                    _ = tokio::time::sleep(every) => {}
                    _ = wake.notified() => {}
                }
            } else {
                wake.notified().await; // nothing to do until you turn it on
            }
        }
    });
}

/// Checks now (Settings button, settings changed).
pub fn wake(app: &AppHandle) {
    lock(&app.state::<AppState>().github).wake.notify_one();
}

/// CI usually starts right after a push: look again in 2 and 6 minutes.
pub fn after_push(app: &AppHandle) {
    if !app.state::<AppState>().settings().github.enabled {
        return;
    }
    for mins in [2u64, 6] {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(mins * 60)).await;
            wake(&app);
        });
    }
}

struct RepoInfo {
    slug: String,
    branch: String,
    root: String,
}

async fn repos(app: &AppHandle) -> Vec<RepoInfo> {
    let roots = lock(&app.state::<AppState>().projects).recent(12);
    let mut out: Vec<RepoInfo> = Vec::new();
    for root in roots {
        let Some(remote) = git(&root, &["remote", "get-url", "origin"]).await else { continue };
        let Some(slug) = projects::github_slug(&remote) else { continue };
        if out.iter().any(|r| r.slug.eq_ignore_ascii_case(&slug)) {
            continue;
        }
        let branch = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).await.unwrap_or_else(|| "main".into());
        out.push(RepoInfo { slug, branch, root });
        if out.len() >= MAX_REPOS {
            break;
        }
    }
    out
}

/// One HTTPS request to the GitHub API (blocking, so it runs on a worker thread).
fn latest_run(token: &str, slug: &str, branch: &str) -> Result<Option<Value>, String> {
    let url = format!("https://api.github.com/repos/{slug}/actions/runs");
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
    let mut response = agent
        .get(&url)
        // .query() percent-encodes, so branch names with & or # stay intact
        .query("branch", branch)
        .query("per_page", "1")
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "Glowby")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(401) => "the token was rejected (401)".to_string(),
            ureq::Error::StatusCode(403) => "no permission or rate limited (403)".to_string(),
            ureq::Error::StatusCode(404) => "repo not found or no access (404)".to_string(),
            other => other.to_string(),
        })?;
    let body: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    Ok(body.pointer("/workflow_runs/0").cloned())
}

pub async fn check(app: &AppHandle) {
    let Some(token) = load_token() else { return };
    let now = chrono::Local::now().format("%H:%M").to_string();
    let mut statuses = Vec::new();
    for repo in repos(app).await {
        let (token, slug, branch) = (token.clone(), repo.slug.clone(), repo.branch.clone());
        let result = tauri::async_runtime::spawn_blocking(move || latest_run(&token, &slug, &branch)).await.unwrap_or_else(|e| Err(e.to_string()));
        let mut status = RepoStatus { repo: repo.slug.clone(), branch: repo.branch.clone(), state: "none".into(), url: String::new(), checked: now.clone() };
        match result {
            Err(e) => status.state = e,
            Ok(None) => {}
            Ok(Some(run)) => {
                let field = |k: &str| run.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                status.url = field("html_url");
                let id = run.get("id").and_then(Value::as_u64).unwrap_or(0);
                status.state = match (field("status").as_str(), field("conclusion").as_str()) {
                    ("completed", "success") => "success".into(),
                    ("completed", "failure") | ("completed", "timed_out") => "failure".into(),
                    ("completed", other) => other.to_string(),
                    _ => "running".into(),
                };
                on_result(app, &repo, &status, id, &field("name"), &field("display_title"));
            }
        }
        statuses.push(status);
    }
    lock(&app.state::<AppState>().github).last = statuses;
    state::publish(app);
}

fn on_result(app: &AppHandle, repo: &RepoInfo, status: &RepoStatus, run_id: u64, workflow: &str, title: &str) {
    let state = app.state::<AppState>();
    match status.state.as_str() {
        "failure" => {
            let first_time = {
                let mut gh = lock(&state.github);
                gh.failing.insert(repo.slug.clone(), run_id);
                gh.notified.insert(run_id)
            };
            if first_time {
                let offer = Offer {
                    kind: "ci",
                    title: format!("CI failed on {}", repo.slug),
                    detail: format!("{workflow} · {} · {title}", repo.branch),
                    project: projects::name(&repo.root),
                    url: Some(status.url.clone()),
                    dir: Some(repo.root.clone()),
                };
                lock(&state.ui).offer = Some((offer, Instant::now() + CI_OFFER_STAYS));
                state::hold_out(app, 60);
                crate::sounds::play(app, crate::sounds::Sound::Error);
                crate::pet_window::show(app);
            }
        }
        "success" => {
            let was_failing = lock(&state.github).failing.remove(&repo.slug).is_some();
            if was_failing {
                {
                    let mut ui = lock(&state.ui);
                    if ui.offer.as_ref().is_some_and(|(o, _)| o.kind == "ci" && o.title.ends_with(&repo.slug)) {
                        ui.offer = None;
                    }
                }
                state::toast(app, "done", format!("CI is green again on {}.", repo.slug), projects::name(&repo.root), 6);
                crate::progress::award(app, crate::progress::FIX_XP, "CI fixed");
            }
        }
        _ => {}
    }
}

pub fn status(app: &AppHandle) -> GithubStatus {
    GithubStatus { has_token: load_token().is_some(), repos: lock(&app.state::<AppState>().github).last.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses its own throwaway entry, never the real token.
    #[test]
    fn credential_manager_round_trip() {
        let name = "Glowby/SelfTest";
        delete_secret(name);
        assert!(read_secret(name).is_none());
        write_secret(name, "not-a-real-token-123").unwrap();
        assert_eq!(read_secret(name).as_deref(), Some("not-a-real-token-123"));
        delete_secret(name);
        assert!(read_secret(name).is_none());
    }
}
