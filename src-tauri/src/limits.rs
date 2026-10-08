//! AI limit forecast: how much of your Claude and Codex usage limits you've
//! used, when they reset, and a rough "you may run low in …" estimate.
//!
//! Where the numbers come from (all on this PC, nothing is sent anywhere):
//! * Claude: Claude Code hands its status line the exact percentages of your
//!   5-hour and weekly limits (Pro/Max). Glowby's status line passes them on.
//!   The Claude app doesn't run a status line, so there Glowby shows the tokens
//!   in your local Claude Code transcripts (no percentage: claude.ai chats use
//!   the same limit and aren't in those logs). When Claude says you've hit a
//!   limit, its message has the exact reset time: Glowby tells you at once and
//!   again when it's back.
//! * Codex: Codex saves its exact limit percentages and reset times in its
//!   session logs (~/.codex/sessions); Glowby reads the newest numbers.
//!
//! The "runs low in" time is a straight-line estimate from your recent pace.
//! It can be wrong (you may slow down or speed up) and every number shows how
//! old it is.

use crate::settings;
use crate::state::{self, AppState, lock};
use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Numbers older than this are shown as stale.
const STALE_SECS: i64 = 30 * 60;
/// Codex logs are re-read at most this often.
const CODEX_READ_EVERY_SECS: i64 = 60;
/// The Claude transcript estimate is recomputed at most this often.
const CLAUDE_ESTIMATE_EVERY_SECS: i64 = 5 * 60;
const CLAUDE_BLOCK_SECS: i64 = 5 * 3600;
/// Only the end of each log is read (recent numbers are at the end).
const TAIL_BYTES: u64 = 8 * 1024 * 1024;
const MAX_HISTORY: usize = 48;

fn now_secs() -> i64 {
    Local::now().timestamp()
}

// ---------------------------------------------------------------- model

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowState {
    /// "five_hour", "seven_day", "primary" …
    pub key: String,
    pub label: String,
    /// 0..100, or None when only a token count is known (Claude estimate before calibration).
    pub used: Option<f64>,
    /// Tokens used in this window (estimate only).
    pub tokens: Option<u64>,
    /// Unix seconds.
    pub resets_at: i64,
    pub observed_at: i64,
    /// true = the agent's own numbers; false = Glowby's estimate.
    pub exact: bool,
    /// (unix seconds, percent used) in this window, oldest first.
    pub history: Vec<(i64, f64)>,
}

impl WindowState {
    /// Records a new reading; a new reset time means a fresh window.
    fn record(&mut self, used: Option<f64>, tokens: Option<u64>, resets_at: i64, at: i64, exact: bool) {
        if (self.resets_at - resets_at).abs() > 120 {
            self.history.clear();
        }
        self.used = used;
        self.tokens = tokens;
        self.resets_at = resets_at;
        self.observed_at = at;
        self.exact = exact;
        if let Some(u) = used {
            let changed = self.history.last().is_none_or(|(t, v)| (v - u).abs() >= 0.5 || at - t >= 600);
            if changed {
                self.history.push((at, u));
                let excess = self.history.len().saturating_sub(MAX_HISTORY);
                self.history.drain(..excess);
            }
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Limits {
    pub claude: Vec<WindowState>,
    pub codex: Vec<WindowState>,
    pub codex_plan: String,
    /// Warnings shown in this app run. A restarted app must recreate pending alerts,
    /// including alerts that were queued quietly during a fullscreen game.
    #[serde(skip)]
    pub warned: BTreeSet<String>,
    /// Claude said its limit is used up until then (unix seconds; 0 = not out).
    pub claude_out_until: i64,
    #[serde(skip)]
    last_codex_read: i64,
    #[serde(skip)]
    last_claude_estimate: i64,
    #[serde(skip)]
    last_save: i64,
}

fn window<'a>(list: &'a mut Vec<WindowState>, key: &str, label: &str) -> &'a mut WindowState {
    if let Some(i) = list.iter().position(|w| w.key == key) {
        return &mut list[i];
    }
    list.push(WindowState { key: key.into(), label: label.into(), ..Default::default() });
    list.last_mut().expect("just pushed")
}

/// Minutes until `percent` reaches 100 at the recent pace (None = not enough data
/// or not rising). Uses the readings from the last 90 minutes.
pub fn minutes_to_full(history: &[(i64, f64)], now: i64) -> Option<f64> {
    let recent: Vec<&(i64, f64)> = history.iter().filter(|(t, _)| now - t <= 90 * 60).collect();
    let (first, last) = (recent.first()?, recent.last()?);
    let minutes = (last.0 - first.0) as f64 / 60.0;
    if minutes < 5.0 {
        return None;
    }
    let per_minute = (last.1 - first.1) / minutes;
    (per_minute > 0.005).then(|| ((100.0 - last.1).max(0.0) / per_minute).max(0.0))
}

pub fn window_label(minutes: i64) -> String {
    match minutes {
        300 => "5-hour".into(),
        10080 => "Weekly".into(),
        m if m % 1440 == 0 => format!("{}-day", m / 1440),
        m if m % 60 == 0 => format!("{}-hour", m / 60),
        m => format!("{m}-minute"),
    }
}

// ---------------------------------------------------------------- Claude: status line

/// Claude Code's status line data (via glowby-hook StatusLine).
pub fn on_claude_status(app: &AppHandle, payload: &Value) {
    let Some(rl) = payload.get("rate_limits").filter(|v| v.is_object()) else { return };
    let at = now_secs();
    {
        let state = app.state::<AppState>();
        let mut limits = lock(&state.limits);
        for (key, label) in [("five_hour", "5-hour"), ("seven_day", "Weekly")] {
            let Some(w) = rl.get(key) else { continue };
            let used = w.get("used_percentage").and_then(Value::as_f64);
            let resets_at = w.get("resets_at").and_then(Value::as_i64).unwrap_or(0);
            if used.is_some() {
                window(&mut limits.claude, key, label).record(used, None, resets_at, at, true);
            }
        }
    }
    after_update(app);
}

// ---------------------------------------------------------------- Claude: estimate from transcripts

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

fn tail(path: &Path) -> String {
    tail_of(path, TAIL_BYTES)
}

/// Reads the last `max` bytes of a file (whole lines only).
pub fn tail_of(path: &Path, max: u64) -> String {
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    let _ = f.take(max).read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 { text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default() } else { text }
}

/// (unix seconds, tokens) of every Claude reply in the last 10 hours, from the
/// usage numbers in your local transcripts (no message text is kept).
fn claude_usage_points(now: i64) -> Vec<(i64, u64)> {
    let since = now - 2 * CLAUDE_BLOCK_SECS;
    let mut points = Vec::new();
    // one reply is written as several lines with the same id: count it once
    let mut seen = std::collections::HashSet::new();
    for root in crate::agent_watch::claude_roots() {
        let Ok(projects) = std::fs::read_dir(root) else { continue };
        for dir in projects.flatten() {
            let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
            for file in files.flatten() {
                let path = file.path();
                let recent = file
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64 >= since).unwrap_or(false));
                if !recent || path.extension().is_none_or(|e| e != "jsonl") {
                    continue;
                }
                for line in tail(&path).lines().filter(|l| l.contains("\"usage\"") && l.contains("\"assistant\"")) {
                    let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
                    let Some(at) = v.get("timestamp").and_then(Value::as_str).and_then(|t| DateTime::parse_from_rfc3339(t).ok()) else { continue };
                    if let Some(id) = v.pointer("/message/id").and_then(Value::as_str)
                        && !seen.insert(id.to_string())
                    {
                        continue;
                    }
                    let u = v.pointer("/message/usage").cloned().unwrap_or(Value::Null);
                    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                    // cache reads are cheap for limits, so they're left out
                    let tokens = n("input_tokens") + n("output_tokens") + n("cache_creation_input_tokens");
                    if at.timestamp() >= since && tokens > 0 {
                        points.push((at.timestamp(), tokens));
                    }
                }
            }
        }
    }
    points.sort();
    points
}

/// A 5-hour Claude window seen in your transcripts.
pub struct Block {
    pub start: i64,
    pub tokens: u64,
}

/// The current 5-hour block: it starts at the first message after the previous
/// block ended, rounded down to the hour (how Claude's windows are commonly counted).
pub fn current_block(points: &[(i64, u64)], now: i64) -> Option<Block> {
    let mut start: Option<i64> = None;
    let mut total = 0u64;
    for &(t, tokens) in points {
        if start.is_none_or(|s| t >= s + CLAUDE_BLOCK_SECS) {
            start = Some(t - t.rem_euclid(3600));
            total = 0;
        }
        total += tokens;
    }
    let start = start?;
    (now < start + CLAUDE_BLOCK_SECS).then_some(Block { start, tokens: total })
}

fn refresh_claude_estimate(app: &AppHandle, force: bool) {
    let state = app.state::<AppState>();
    let now = now_secs();
    {
        let mut limits = lock(&state.limits);
        let fresh_exact = limits.claude.iter().any(|w| w.exact && w.resets_at > now && now - w.observed_at < 6 * 3600);
        if fresh_exact || (!force && now - limits.last_claude_estimate < CLAUDE_ESTIMATE_EVERY_SECS) {
            return;
        }
        limits.last_claude_estimate = now;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let points = claude_usage_points(now);
        let state = app.state::<AppState>();
        {
            let mut limits = lock(&state.limits);
            match current_block(&points, now) {
                Some(Block { start, tokens, .. }) => {
                    let w = window(&mut limits.claude, "five_hour", "5-hour");
                    if w.exact && w.resets_at > now {
                        // Claude's own numbers (status line or a limit hit) are better than a token count
                    } else {
                        w.history.clear();
                        w.record(None, Some(tokens), start + CLAUDE_BLOCK_SECS, now, false);
                    }
                }
                None => limits.claude.retain(|w| w.exact),
            }
        }
        after_update(&app);
    });
}

/// Claude said it hit a usage limit (its own message in the transcript, with
/// the exact reset time): tell you right away, and again when it's back.
pub fn claude_limit_hit(app: &AppHandle, hit: &glowby_detective::live::LimitHit) {
    if hit.resets_at <= now_secs() {
        return;
    }
    let (key, label) = match hit.kind.as_str() {
        "seven_day" | "seven_day_opus" | "seven_day_sonnet" => ("seven_day", "Weekly"),
        _ => ("five_hour", "5-hour"),
    };
    let state = app.state::<AppState>();
    let codex_left = {
        let mut limits = lock(&state.limits);
        if !limits.warned.insert(format!("Claude:hit:{}", hit.resets_at)) {
            return; // already told you about this one
        }
        // this alert says it all: no "Claude is at 100%" warning on top
        for level in ["95", "high", "pace"] {
            limits.warned.insert(format!("Claude:{key}:{}:{level}", hit.resets_at));
        }
        window(&mut limits.claude, key, label).record(Some(100.0), None, hit.resets_at, hit.at, true);
        limits.claude_out_until = hit.resets_at;
        limits.last_save = 0; // save right away
        let now = now_secs();
        limits.codex.iter().filter(|w| w.resets_at > now).filter_map(|w| w.used).fold(None::<f64>, |acc, u| Some(acc.map_or(u, |a| a.max(u)))).map(|worst| 100.0 - worst)
    };
    crate::applog::line(format!("limits: Claude hit its {label} limit, back at {}", clock(hit.resets_at, now_secs())));
    let back = format!("{} (in {})", clock(hit.resets_at, now_secs()), duration_text((hit.resets_at - now_secs()) as f64 / 60.0));
    let codex = match codex_left {
        Some(left) if left > 20.0 => format!(" Codex still has about {left:.0}% left if you want to keep going there."),
        _ => String::new(),
    };
    let offer = crate::actions::Offer {
        kind: "limitHit",
        title: format!("Claude is out of usage until {}", clock(hit.resets_at, now_secs())),
        detail: format!("Claude says its {} limit is used up. It's back at {back}, and I'll tell you the moment it is.{codex}", label.to_lowercase()),
        project: String::new(),
        url: None,
        dir: None,
    };
    let _ = crate::alerts::raise(
        app,
        offer,
        std::time::Duration::from_secs(3600),
        Some((format!("Claude is out of usage until {}", clock(hit.resets_at, now_secs())), "Glowby will tell you when it's back.".into())),
    );
    after_update(app);
    schedule_back(app, hit.resets_at);
}

/// "Claude is back!" when the limit resets.
fn schedule_back(app: &AppHandle, at: i64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let wait = (at - now_secs()).max(0) as u64 + 5;
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        let state = app.state::<AppState>();
        {
            let mut limits = lock(&state.limits);
            if limits.claude_out_until != at {
                return; // a newer limit hit replaced this one
            }
            limits.claude_out_until = 0;
            limits.last_save = 0;
        }
        crate::applog::line("limits: Claude is back");
        let offer = crate::actions::Offer {
            kind: "claudeBack",
            title: "Claude is back!".into(),
            detail: format!("Your usage limit reset at {}. Ready when you are.", clock(at, now_secs())),
            project: String::new(),
            url: None,
            dir: None,
        };
        let _ = crate::alerts::raise(&app, offer, std::time::Duration::from_secs(30 * 60), Some(("Claude is back!".into(), "Your usage limit reset. Ready when you are.".into())));
        after_update(&app);
    });
}

/// At start: if Claude is still out, the "back" alert is scheduled again.
pub fn resume(app: &AppHandle) {
    let until = lock(&app.state::<AppState>().limits).claude_out_until;
    if until > now_secs() {
        schedule_back(app, until);
    }
}

// ---------------------------------------------------------------- Codex: session logs

/// Codex's logs, newest first (only the last few days).
fn recent_codex_logs() -> Vec<PathBuf> {
    let Some(root) = home().map(|h| h.join(".codex").join("sessions")) else { return Vec::new() };
    // sessions/YYYY/MM/DD/rollout-*.jsonl: walk the newest folders first
    let newest_dir = |dir: &Path| -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        dirs.reverse();
        dirs
    };
    let mut logs = Vec::new();
    for year in newest_dir(&root).into_iter().take(2) {
        for month in newest_dir(&year).into_iter().take(2) {
            for day in newest_dir(&month).into_iter().take(4) {
                let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&day)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
                    .filter_map(|e| Some((e.metadata().and_then(|m| m.modified()).ok()?, e.path())))
                    .collect();
                files.sort();
                logs.extend(files.into_iter().rev().map(|(_, p)| p));
                if logs.len() >= 20 {
                    return logs;
                }
            }
        }
    }
    logs
}

/// The newest real limit numbers. Some logs have none (imported chats, a
/// session that ended before its first reply), so older logs are tried too.
fn newest_codex_numbers() -> Option<(Value, i64)> {
    recent_codex_logs().iter().filter_map(|p| parse_codex_limits(&tail(p))).max_by_key(|(_, at)| *at)
}

/// The newest rate-limit numbers in a Codex log ("token_count" events).
pub fn parse_codex_limits(log: &str) -> Option<(Value, i64)> {
    log.lines().rev().filter(|l| l.contains("\"rate_limits\"")).find_map(|line| {
        let v: Value = serde_json::from_str(line).ok()?;
        let rl = v.pointer("/payload/rate_limits").or_else(|| v.pointer("/payload/info/rate_limits")).filter(|r| r.is_object())?.clone();
        rl.get("primary").filter(|p| p.is_object())?;
        let at = v.get("timestamp").and_then(Value::as_str).and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp())?;
        Some((rl, at))
    })
}

pub fn refresh_from_claude_log(app: &AppHandle) { refresh_claude_estimate(app, false); }

pub fn on_codex_log(app: &AppHandle, text: &str) {
    let Some((rl, at)) = parse_codex_limits(text) else { return };
    record_codex(app, rl, at);
}

fn record_codex(app: &AppHandle, rl: Value, at: i64) {
    let state = app.state::<AppState>();
    {
        let mut limits = lock(&state.limits);
        limits.codex_plan = rl.get("plan_type").and_then(Value::as_str).unwrap_or("").to_string();
        for key in ["primary", "secondary"] {
            let Some(w) = rl.get(key).filter(|w| w.is_object()) else { continue };
            if limits.codex.iter().any(|x| x.key == key && x.observed_at > at) { continue; }
            let used = w.get("used_percent").and_then(Value::as_f64);
            let minutes = w.get("window_minutes").and_then(Value::as_i64).unwrap_or(0);
            let resets_at = w.get("resets_at").and_then(Value::as_i64).or_else(|| w.get("resets_in_seconds").and_then(Value::as_i64).map(|s| at+s)).unwrap_or(0);
            window(&mut limits.codex, key, &window_label(minutes)).record(used, None, resets_at, at, true);
        }
    }
    after_update(app);
}

fn refresh_codex(app: &AppHandle, force: bool) {
    let state = app.state::<AppState>();
    let now = now_secs();
    {
        let mut limits = lock(&state.limits);
        if !force && now - limits.last_codex_read < CODEX_READ_EVERY_SECS {
            return;
        }
        limits.last_codex_read = now;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some((rl, at)) = newest_codex_numbers() else { return };
        record_codex(&app, rl, at);
    });
}

// ---------------------------------------------------------------- triggers

/// Every hook event: Codex numbers after a Codex turn, Claude's estimate after a
/// Claude turn (each at most once a minute or every few minutes).
pub fn on_event(app: &AppHandle, event: &str, payload: &Value) {
    if !lock(&app.state::<AppState>().settings).limits.enabled {
        return;
    }
    let codex = crate::sessions::agent_of(payload) == "codex";
    match event {
        "Stop" | "PostToolUse" | "SessionStart" | "UserPromptSubmit" if codex => refresh_codex(app, false),
        "Stop" if !codex => refresh_claude_estimate(app, false),
        _ => {}
    }
}

/// Opening the limits card or Settings: read fresh numbers now.
pub fn refresh_now(app: &AppHandle) {
    refresh_codex(app, true);
    refresh_claude_estimate(app, true);
}

/// Checks for warnings, saves (at most every minute, or now after a warning), updates Glowby.
fn after_update(app: &AppHandle) {
    maybe_warn(app);
    let state = app.state::<AppState>();
    let now = now_secs();
    {
        let mut limits = lock(&state.limits);
        if now - limits.last_save >= 60 {
            limits.last_save = now;
            if let Err(e) = settings::save_json(&state.paths.limits_file, &*limits) {
                crate::applog::line(format!("couldn't save limits.json: {e}"));
            }
        }
    }
    state::publish(app);
}

// ---------------------------------------------------------------- the view

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WindowView {
    pub label: String,
    pub used: Option<f64>,
    /// "62%" or "≈ 62%" or "≈ 1.2M tokens"
    pub used_text: String,
    /// "resets 16:40 (in 2 h 5 min)"
    pub resets_text: String,
    /// "≈ may run low in 45 min" / "should last until it resets" / "pace unknown yet"
    pub forecast: String,
    /// "as of 14:32 (3 min ago)"
    pub as_of: String,
    pub stale: bool,
    pub exact: bool,
    /// The estimate says you'll run out before the reset.
    pub tight: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub agent: String,
    pub plan: String,
    pub windows: Vec<WindowView>,
    /// Shown when there are no numbers yet.
    pub empty_hint: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LimitsView {
    pub agents: Vec<AgentView>,
}

pub fn duration_text(minutes: f64) -> String {
    let m = minutes.round().max(0.0) as i64;
    match m {
        0 => "under a minute".into(),
        1..=59 => format!("{m} min"),
        60..=1439 => {
            let (h, rest) = (m / 60, m % 60);
            if rest == 0 { format!("{h} h") } else { format!("{h} h {rest} min") }
        }
        _ => {
            let days = (m as f64 / 1440.0).round() as i64;
            format!("{days} day{}", if days == 1 { "" } else { "s" })
        }
    }
}

fn clock(t: i64, now: i64) -> String {
    let Some(dt) = Local.timestamp_opt(t, 0).single() else { return "?".into() };
    if (t - now).abs() < 20 * 3600 { dt.format("%H:%M").to_string() } else { dt.format("%a %d %b, %H:%M").to_string() }
}

pub fn window_view(w: &WindowState, now: i64) -> WindowView {
    let mins_to_reset = ((w.resets_at - now) as f64 / 60.0).max(0.0);
    let approx = if w.exact { "" } else { "≈ " };
    let used_text = match (w.used, w.tokens) {
        (Some(u), _) => format!("{approx}{u:.0}%"),
        (None, Some(t)) if t >= 1_000_000 => format!("≈ {:.1}M tokens", t as f64 / 1e6),
        (None, Some(t)) => format!("≈ {}k tokens", t / 1000),
        _ => "?".into(),
    };
    let resets_text = if w.resets_at > now {
        format!("resets {} (in {})", clock(w.resets_at, now), duration_text(mins_to_reset))
    } else {
        "may have reset already".into()
    };
    let eta = minutes_to_full(&w.history, now);
    let full_now = w.used.is_some_and(|u| u >= 99.5);
    let (forecast, tight) = match (eta, w.used) {
        _ if full_now => (format!("limit reached, back {}", if w.resets_at > now { format!("in {}", duration_text(mins_to_reset)) } else { "soon".into() }), true),
        (Some(e), _) if e < mins_to_reset => (format!("≈ may run low in {}", duration_text(e)), true),
        (Some(_), _) => ("≈ should last until it resets".into(), false),
        (None, Some(_)) => ("pace unknown yet (needs a few readings)".into(), false),
        (None, None) => ("Claude Code chats only; claude.ai chats use the same limit".into(), false),
    };
    let age = now - w.observed_at;
    WindowView {
        label: w.label.clone(),
        used: w.used,
        used_text,
        resets_text,
        forecast,
        as_of: format!("as of {} ({} ago)", clock(w.observed_at, now), duration_text(age as f64 / 60.0)),
        stale: age > STALE_SECS,
        exact: w.exact,
        tight,
    }
}

pub fn view(app: &AppHandle) -> Option<LimitsView> {
    let state = app.state::<AppState>();
    if !lock(&state.settings).limits.enabled {
        return None;
    }
    let now = now_secs();
    let limits = lock(&state.limits);
    let live = |list: &Vec<WindowState>| list.iter().filter(|w| w.resets_at > now - 3600).map(|w| window_view(w, now)).collect::<Vec<_>>();
    Some(LimitsView {
        agents: vec![
            AgentView {
                agent: "Claude".into(),
                plan: String::new(),
                windows: live(&limits.claude),
                empty_hint: "No numbers yet. Exact percentages come only from terminal sessions (Glowby's status line); the Claude app doesn't share them. Glowby still tells you the moment Claude hits a limit, and when it's back.".into(),
            },
            AgentView {
                agent: "Codex".into(),
                plan: limits.codex_plan.clone(),
                windows: live(&limits.codex),
                empty_hint: "No numbers yet. They appear after your next Codex reply.".into(),
            },
        ],
    })
}

// ---------------------------------------------------------------- warnings

/// When a limit gets tight: an offer to save a handoff note, mentioning the
/// other agent if it has more room. Once per window and level.
pub fn maybe_warn(app: &AppHandle) {
    let state = app.state::<AppState>();
    let s = state.settings();
    if !s.limits.enabled || !s.limits.warn {
        return;
    }
    let now = now_secs();
    let mut warning = None;
    let mut reserved_key = String::new();
    {
        let mut limits = lock(&state.limits);
        let best_left = |list: &[WindowState]| list.iter().filter(|w| w.resets_at > now && w.exact && now - w.observed_at <= STALE_SECS).filter_map(|w| w.used).fold(None::<f64>, |acc, u| Some(acc.map_or(u, |a| a.max(u)))).map(|worst| 100.0 - worst);
        let claude_left = best_left(&limits.claude);
        let codex_left = best_left(&limits.codex);
        for (agent, list, other, other_left) in [("Claude", limits.claude.clone(), "Codex", codex_left), ("Codex", limits.codex.clone(), "Claude", claude_left)] {
            // Within a window usage only goes up, so an older reading is still a
            // safe minimum and is warned about too. Only the pace needs fresh numbers.
            for w in list.iter().filter(|w| w.resets_at > now) {
                let v = window_view(w, now);
                let Some(used) = w.used else { continue };
                let fresh = now - w.observed_at <= STALE_SECS;
                let level = if used >= 95.0 { "95" } else if used >= s.limits.warn_percent as f64 { "high" } else if v.tight && fresh { "pace" } else { continue };
                let key = format!("{agent}:{}:{}:{level}", w.key, w.resets_at);
                if !limits.warned.insert(key.clone()) {
                    continue;
                }
                reserved_key = key;
                limits.last_save = 0; // save after a warning is actually shown
                let tip = match other_left {
                    Some(left) if left > 30.0 => format!(" {other} still has about {left:.0}% left, so you could continue there."),
                    _ => String::new(),
                };
                let age = if fresh { "These numbers can be a few minutes old." } else { "This is the latest reading I have; it may be stale." };
                warning = Some((
                    format!("{agent} is at {} of its {} limit", v.used_text, w.label.to_lowercase()),
                    format!("{}, {}. {}.{tip} {age}", v.forecast, v.resets_text, v.as_of),
                ));
                break;
            }
            if warning.is_some() {
                break;
            }
        }
        if limits.warned.len() > 200 {
            let keep: BTreeSet<String> = limits.warned.iter().rev().take(100).cloned().collect();
            limits.warned = keep;
        }
    }
    let Some((title, detail)) = warning else { return };
    let notification = (title.clone(), "Glowby has a handoff note ready, to continue later or with the other agent.".to_string());
    let offer = crate::actions::Offer { kind: "limits", title, detail, project: String::new(), url: None, dir: None };
    if !crate::alerts::raise(app, offer, std::time::Duration::from_secs(3600), Some(notification)) {
        lock(&state.limits).warned.remove(&reserved_key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn restart_can_recreate_an_alert_instead_of_remembering_an_invisible_card() {
        let limits: Limits = serde_json::from_value(json!({"warned":["Codex:primary:123:high"]})).unwrap();
        assert!(limits.warned.is_empty());
        assert!(serde_json::to_value(limits).unwrap().get("warned").is_none());
    }

    #[test]
    fn pace_gives_a_rough_time_to_the_limit() {
        let now = 100_000;
        // 40% → 60% in 40 minutes = 0.5% per minute → 40% left = 80 minutes
        let history = vec![(now - 2400, 40.0), (now - 1200, 50.0), (now, 60.0)];
        let eta = minutes_to_full(&history, now).unwrap();
        assert!((eta - 80.0).abs() < 1.0, "eta {eta}");
        assert!(minutes_to_full(&[(now, 60.0)], now).is_none(), "one reading is not a pace");
        assert!(minutes_to_full(&[(now - 3000, 60.0), (now, 60.0)], now).is_none(), "flat = no estimate");
        assert!(minutes_to_full(&[(now - 9000, 10.0), (now - 8000, 90.0)], now).is_none(), "old readings are ignored");
    }

    #[test]
    fn a_new_reset_time_starts_a_fresh_window() {
        let mut w = WindowState::default();
        w.record(Some(80.0), None, 1000, 10, true);
        w.record(Some(85.0), None, 1000, 700, true);
        assert_eq!(w.history.len(), 2);
        w.record(Some(3.0), None, 20_000, 800, true);
        assert_eq!(w.history, vec![(800, 3.0)]);
    }

    #[test]
    fn codex_limits_are_read_from_its_log() {
        let log = [
            json!({ "timestamp": "2026-10-03T10:00:00Z", "type": "event_msg", "payload": { "type": "token_count", "rate_limits": { "primary": { "used_percent": 50.0, "window_minutes": 300, "resets_at": 1793615689 } } } }),
            json!({ "timestamp": "2026-10-03T10:05:00Z", "type": "response_item", "payload": { "type": "message" } }),
            json!({ "timestamp": "2026-10-03T11:06:17.318Z", "type": "event_msg", "payload": { "type": "token_count", "rate_limits": { "primary": { "used_percent": 91.0, "window_minutes": 43200, "resets_at": 1793615689 }, "secondary": null, "plan_type": "free" } } }),
        ]
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let (rl, at) = parse_codex_limits(&log).unwrap();
        assert_eq!(rl["primary"]["used_percent"], 91.0);
        assert_eq!(rl["plan_type"], "free");
        assert_eq!(at, DateTime::parse_from_rfc3339("2026-10-03T11:06:17.318Z").unwrap().timestamp());
        assert_eq!(window_label(43200), "30-day");
        assert_eq!(window_label(300), "5-hour");
        assert_eq!(window_label(10080), "Weekly");
    }

    #[test]
    fn claude_blocks_start_on_the_hour_and_last_five_hours() {
        let h = 3600;
        let base = 1_800_000_000 - 1_800_000_000 % h; // on the hour
        let points = vec![(base - 7 * h, 500), (base + 600, 1000), (base + 2 * h, 2000)];
        let b = current_block(&points, base + 3 * h).unwrap();
        assert_eq!(b.start, base, "the new block starts at the hour of its first message");
        assert_eq!(b.tokens, 3000);
        assert!(current_block(&points, base + 6 * h).is_none(), "that block has ended");
    }

    #[test]
    fn the_view_is_honest_about_estimates_and_age() {
        let now = 1_800_000_000;
        let mut w = WindowState { key: "five_hour".into(), label: "5-hour".into(), ..Default::default() };
        w.record(Some(70.0), None, now + 3600, now - 40 * 60, false);
        w.record(Some(80.0), None, now + 3600, now - 20 * 60, false);
        w.record(Some(90.0), None, now + 3600, now, false);
        let v = window_view(&w, now);
        assert_eq!(v.used_text, "≈ 90%");
        assert!(v.tight && v.forecast.starts_with("≈ may run low in 20 min"), "{}", v.forecast);
        assert!(v.as_of.starts_with("as of") && !v.stale);
        assert_eq!(duration_text(125.0), "2 h 5 min");
        assert_eq!(duration_text(3.0 * 1440.0), "3 days");
    }
}
