//! Your recent projects: the git repositories your Claude Code and Codex sessions
//! ran in, plus the last folder any session worked in. Used by the chat (so you
//! don't have to pick a folder), the daily briefing and the GitHub CI check.
//! Saved in projects.json.

use crate::settings;
use crate::state::{AppState, lock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_PROJECTS: usize = 12;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KnownProject {
    /// Repository root, e.g. C:\code\my-app
    pub root: String,
    pub last_seen: String,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Projects {
    pub list: Vec<KnownProject>,
    /// The folder your most recent Claude Code / Codex session worked in.
    pub last_dir: String,
    /// Folder → repository root (None = not a git repo). Memory only.
    #[serde(skip)]
    roots: HashMap<String, Option<String>>,
}

impl Projects {
    fn touch(&mut self, root: &str) {
        let now = chrono::Local::now().to_rfc3339();
        match self.list.iter_mut().find(|p| p.root.eq_ignore_ascii_case(root)) {
            Some(p) => p.last_seen = now,
            None => self.list.push(KnownProject { root: root.into(), last_seen: now }),
        }
        self.list.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        self.list.truncate(MAX_PROJECTS);
    }

    /// Most recently used first.
    pub fn recent(&self, max: usize) -> Vec<String> {
        self.list.iter().take(max).map(|p| p.root.clone()).collect()
    }

    /// The best folder to work in when no session is running: the last folder a
    /// session used, else the most recent project. Only folders that still exist.
    pub fn remembered_dir(&self, exists: impl Fn(&str) -> bool) -> Option<String> {
        std::iter::once(self.last_dir.clone())
            .chain(self.list.iter().map(|p| p.root.clone()))
            .find(|d| !d.is_empty() && exists(d))
    }
}

/// Runs git without a shell or console window, with a timeout.
pub async fn git(dir: &str, args: &[&str]) -> Option<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .creation_flags(CREATE_NO_WINDOW);
    let out = tokio::time::timeout(Duration::from_secs(8), cmd.output()).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Remembers the repository a session's folder belongs to (git is asked only
/// the first time a folder is seen).
pub fn note_cwd(app: &AppHandle, cwd: &str) {
    if cwd.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    // remembered in memory now, written to projects.json at the end of each turn
    lock(&state.projects).last_dir = cwd.to_string();
    let known = lock(&state.projects).roots.get(cwd).cloned();
    match known {
        Some(Some(root)) => {
            lock(&state.projects).touch(&root);
        }
        Some(None) => {}
        None => {
            lock(&state.projects).roots.insert(cwd.to_string(), None);
            let app = app.clone();
            let cwd = cwd.to_string();
            tauri::async_runtime::spawn(async move {
                let root = git(&cwd, &["rev-parse", "--show-toplevel"]).await.map(|r| r.replace('/', "\\"));
                let state = app.state::<AppState>();
                {
                    let mut projects = lock(&state.projects);
                    projects.roots.insert(cwd, root.clone());
                    if let Some(root) = &root {
                        projects.touch(root);
                    }
                }
                save(&app);
            });
        }
    }
}

pub fn save(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snapshot = serde_json::to_value(&*lock(&state.projects)).ok();
    if let Some(v) = snapshot {
        let _ = settings::save_json(&state.paths.projects_file, &v);
    }
}

pub fn name(root: &str) -> String {
    crate::sessions::project_name(root)
}

/// "owner/repo" from a GitHub remote URL (https or ssh).
pub fn github_slug(remote: &str) -> Option<String> {
    let r = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    let rest = r
        .strip_prefix("https://github.com/")
        .or_else(|| r.strip_prefix("http://github.com/"))
        .or_else(|| r.strip_prefix("git@github.com:"))
        .or_else(|| r.strip_prefix("ssh://git@github.com/"))?;
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty() && parts.next().is_none()).then(|| format!("{owner}/{repo}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_remotes_are_recognised() {
        assert_eq!(github_slug("https://github.com/Shadow-PJ/glowby.git").as_deref(), Some("Shadow-PJ/glowby"));
        assert_eq!(github_slug("git@github.com:me/app.git").as_deref(), Some("me/app"));
        assert_eq!(github_slug("ssh://git@github.com/me/app").as_deref(), Some("me/app"));
        assert_eq!(github_slug("https://gitlab.com/me/app.git"), None);
        assert_eq!(github_slug("https://github.com/me"), None);
    }

    #[test]
    fn remembered_folder_prefers_the_last_session_and_skips_missing_ones() {
        let mut p = Projects::default();
        p.touch(r"C:\code\repo");
        p.last_dir = r"C:\code\notes".into();
        assert_eq!(p.remembered_dir(|_| true).as_deref(), Some(r"C:\code\notes"));
        assert_eq!(p.remembered_dir(|d| d != r"C:\code\notes").as_deref(), Some(r"C:\code\repo"));
        assert_eq!(p.remembered_dir(|_| false), None);
        assert_eq!(Projects::default().remembered_dir(|_| true), None);
    }

    #[test]
    fn recent_projects_are_ordered_and_capped() {
        let mut p = Projects::default();
        for i in 0..15 {
            p.touch(&format!("C:\\code\\p{i}"));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(p.list.len(), MAX_PROJECTS);
        assert_eq!(p.recent(1), vec!["C:\\code\\p14".to_string()]);
    }
}
