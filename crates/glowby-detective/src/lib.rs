//! The token detective: finds what eats your Claude usage limits, from your
//! local Claude Code logs (~/.claude/projects/<project>/<session>.jsonl).
//!
//! It reads numbers, timestamps, tool names and file paths, never the text of
//! your messages or files (only the *length* of a file Claude read). The log
//! format isn't an official API, so every line is parsed tolerantly: anything
//! unexpected is skipped, never fatal.
//!
//! Three detectors:
//! * **Cache rebuilds after breaks**: Claude Code caches your conversation; after
//!   a pause longer than the cache's lifetime (5 minutes or 1 hour, read from the
//!   logs, not assumed) the whole context is sent again. Normal rebuilds are not
//!   counted: a session's first reply, right after /compact, or after a model switch.
//! * **Re-read files**: a file read in full again in the same session, although
//!   it didn't change and no /compact happened in between.
//! * **Long sessions**: how much more each step costs after two hours.

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub mod live;

/// A rebuild only counts if it re-sends at least this many tokens…
const MIN_REBUILD_TOKENS: u64 = 10_000;
/// …and at least this share of the context.
const REBUILD_SHARE: f64 = 0.5;
/// Default cache lifetime when a log doesn't say.
const DEFAULT_TTL_SECS: i64 = 300;
/// A file needs this many wasted re-reads in the period to become a finding.
const MIN_REREADS: u32 = 3;
const EARLY_SECS: i64 = 3600;
const LATE_SECS: i64 = 2 * 3600;

pub fn claude_projects_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|h| PathBuf::from(h).join(".claude").join("projects"))
}

// ---------------------------------------------------------------- parsing

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Call {
    pub t: i64,
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub create: u64,
    pub read: u64,
    pub create_1h: u64,
    pub create_5m: u64,
}

impl Call {
    fn context(&self) -> u64 {
        self.input + self.create + self.read
    }
    /// Rough "cost" of one reply: cache reads are about 10× cheaper.
    pub fn weighted(&self) -> u64 {
        self.input + self.create + self.output + self.read / 10
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Read { tool_id: String, path: String },
    Edit { path: String },
    Compact,
}

#[derive(Clone, Debug, Default)]
pub struct ReadResult {
    pub full: bool,
    pub total_lines: u64,
    pub chars: u64,
}

#[derive(Default, Debug)]
pub struct Session {
    pub id: String,
    pub cwd: String,
    /// Main conversation only (subagents have their own context).
    pub calls: Vec<Call>,
    pub steps: Vec<(i64, Step)>,
    pub reads: HashMap<String, ReadResult>,
}

fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn time_of(v: &Value) -> Option<i64> {
    v.get("timestamp").and_then(Value::as_str).and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp())
}

const EDIT_TOOLS: &[&str] = &["Edit", "MultiEdit", "Write", "NotebookEdit"];

/// Reads the lines of one or more transcripts into sessions.
pub fn parse_lines<I: IntoIterator<Item = String>>(lines: I, sessions: &mut BTreeMap<String, Session>) {
    let mut seen_replies: HashSet<String> = HashSet::new();
    for line in lines {
        // cheap filter before parsing: only the line kinds we use
        if !(line.contains("\"assistant\"") || line.contains("tool_result") || line.contains("ompact")) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(sid) = v.get("sessionId").and_then(Value::as_str) else { continue };
        let side = v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false);
        let t = time_of(&v);
        let s = sessions.entry(sid.to_string()).or_insert_with(|| Session { id: sid.into(), ..Default::default() });
        if s.cwd.is_empty()
            && let Some(cwd) = v.get("cwd").and_then(Value::as_str)
        {
            s.cwd = cwd.into();
        }
        if side {
            continue; // subagents: their own context, not this conversation's
        }
        match v.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                let Some(msg) = v.get("message") else { continue };
                let Some(t) = t else { continue };
                // one reply is written as several lines (thinking, text, tool use)
                // with the same id and the same usage: count it once
                let id = msg.get("id").or_else(|| v.get("requestId")).and_then(Value::as_str).unwrap_or("");
                if let Some(u) = msg.get("usage").filter(|u| u.is_object())
                    && (id.is_empty() || seen_replies.insert(id.to_string()))
                {
                    let cc = u.get("cache_creation").cloned().unwrap_or(Value::Null);
                    s.calls.push(Call {
                        t,
                        model: msg.get("model").and_then(Value::as_str).unwrap_or("").to_string(),
                        input: num(u, "input_tokens"),
                        output: num(u, "output_tokens"),
                        create: num(u, "cache_creation_input_tokens"),
                        read: num(u, "cache_read_input_tokens"),
                        create_1h: num(&cc, "ephemeral_1h_input_tokens"),
                        create_5m: num(&cc, "ephemeral_5m_input_tokens"),
                    });
                }
                for block in msg.get("content").and_then(Value::as_array).into_iter().flatten() {
                    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                        continue;
                    }
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    let path = input.get("file_path").or_else(|| input.get("notebook_path")).and_then(Value::as_str).unwrap_or("");
                    if path.is_empty() {
                        continue;
                    }
                    if name == "Read" {
                        let tool_id = block.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                        s.steps.push((t, Step::Read { tool_id, path: path.into() }));
                    } else if EDIT_TOOLS.contains(&name) {
                        s.steps.push((t, Step::Edit { path: path.into() }));
                    }
                }
            }
            Some("user") => {
                if v.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
                    && let Some(t) = t
                {
                    s.steps.push((t, Step::Compact));
                }
                // a Read's result: was the whole file read, and how big is it?
                let Some(file) = v.pointer("/toolUseResult/file").filter(|f| f.is_object()) else { continue };
                let blocks = v.pointer("/message/content").and_then(Value::as_array);
                let Some(tool_id) = blocks.into_iter().flatten().find_map(|b| b.get("tool_use_id").and_then(Value::as_str)) else { continue };
                let (start, lines, total) = (num(file, "startLine"), num(file, "numLines"), num(file, "totalLines"));
                let chars = file.get("content").and_then(Value::as_str).map_or(0, |c| c.chars().count() as u64);
                s.reads.insert(tool_id.into(), ReadResult { full: start <= 1 && total > 0 && lines >= total, total_lines: total, chars });
            }
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") => {
                if let Some(t) = t {
                    s.steps.push((t, Step::Compact));
                }
            }
            _ => {}
        }
    }
}

/// Parses every transcript changed since `since` (one level of project folders).
pub fn load_sessions(root: &Path, since: i64) -> BTreeMap<String, Session> {
    let mut sessions = BTreeMap::new();
    let Ok(projects) = std::fs::read_dir(root) else { return sessions };
    for dir in projects.flatten().filter(|d| d.path().is_dir()) {
        let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
        for file in files.flatten() {
            let path = file.path();
            let fresh = file
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .is_some_and(|d| d.as_secs() as i64 >= since);
            if !fresh || path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Ok(f) = std::fs::File::open(&path) else { continue };
            let lines = BufReader::new(f).lines().map_while(Result::ok);
            parse_lines(lines, &mut sessions);
        }
    }
    sessions
}

// ---------------------------------------------------------------- analysis

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct GapBucket {
    pub label: String,
    pub replies: u32,
    pub rebuilds: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CacheFindings {
    /// Rebuilds after a break longer than the cache's lifetime.
    pub break_rebuilds: u32,
    pub break_tokens: u64,
    /// Typical break length that caused them (minutes, 25th–75th percentile).
    pub typical_break: Option<(i64, i64)>,
    /// Cache lifetimes seen in the logs, e.g. ["5 min", "1 hour"].
    pub lifetimes: Vec<String>,
    /// Normal rebuilds (first reply, after /compact, model switch): not counted.
    pub normal_rebuilds: u32,
    /// Rebuilds without a break (Claude Code changed tools/settings, server side).
    pub other_rebuilds: u32,
    /// Rebuild rate by idle gap: lets the data show where the cache expires.
    pub by_gap: Vec<GapBucket>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct RereadFile {
    pub path: String,
    pub project: String,
    pub project_dir: String,
    pub rereads: u32,
    pub sessions: u32,
    pub total_lines: u64,
    pub tokens: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct LongSessions {
    pub sessions: u32,
    /// Average weighted tokens per reply in the first hour vs after two hours.
    pub early_avg: u64,
    pub late_avg: u64,
    pub late_replies: u32,
    /// Extra tokens the late replies cost compared with the early pace.
    pub extra_tokens: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Finding {
    /// "cache", "reread", "long"
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub tip: String,
    pub tokens: u64,
    /// Share of all (weighted) tokens in the period, 0..100.
    pub share: f64,
    /// For re-read files: the file and its project folder (for a one-click fix).
    pub file: Option<String>,
    pub project_dir: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Report {
    pub since: i64,
    pub until: i64,
    pub sessions: u32,
    pub projects: u32,
    pub replies: u32,
    pub weighted_tokens: u64,
    pub cache: CacheFindings,
    pub rereads: Vec<RereadFile>,
    pub long: LongSessions,
    /// The findings, biggest saving first.
    pub findings: Vec<Finding>,
}

fn project_name(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

fn norm(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

/// Cache lifetime the previous replies used (from what they wrote to the cache).
fn ttl_before(calls: &[Call]) -> i64 {
    calls
        .iter()
        .rev()
        .find_map(|c| if c.create_1h > 0 { Some(3600) } else if c.create_5m > 0 { Some(300) } else { None })
        .unwrap_or(DEFAULT_TTL_SECS)
}

const GAP_EDGES: [(i64, &str); 6] = [(4, "under 4 min"), (6, "4–6 min"), (10, "6–10 min"), (30, "10–30 min"), (60, "30–60 min"), (i64::MAX, "over 1 hour")];

pub fn analyze(sessions: &BTreeMap<String, Session>, since: i64, until: i64) -> Report {
    let mut r = Report { since, until, ..Default::default() };
    r.cache.by_gap = GAP_EDGES.iter().map(|(_, l)| GapBucket { label: (*l).into(), ..Default::default() }).collect();
    let mut break_gaps: Vec<i64> = Vec::new();
    let mut lifetimes: HashSet<&str> = HashSet::new();
    let mut projects: HashSet<String> = HashSet::new();
    let mut rereads: HashMap<String, RereadFile> = HashMap::new();
    let mut reread_sessions: HashMap<String, HashSet<String>> = HashMap::new();
    let (mut early, mut late): (Vec<u64>, Vec<u64>) = (Vec::new(), Vec::new());
    let mut long_sessions = 0u32;

    for s in sessions.values() {
        let mut calls: Vec<Call> = s.calls.iter().filter(|c| c.t >= since && c.t <= until).cloned().collect();
        calls.sort_by_key(|c| c.t);
        if calls.is_empty() {
            continue;
        }
        let mut steps: Vec<&(i64, Step)> = s.steps.iter().collect();
        steps.sort_by_key(|(t, _)| *t);
        let compactions: Vec<i64> = steps.iter().filter(|(_, st)| *st == Step::Compact).map(|(t, _)| *t).collect();
        let compacted_between = |a: i64, b: i64| compactions.iter().any(|t| *t >= a && *t <= b);
        r.sessions += 1;
        r.replies += calls.len() as u32;
        projects.insert(project_name(&s.cwd));
        for c in &calls {
            if c.create_1h > 0 {
                lifetimes.insert("1 hour");
            }
            if c.create_5m > 0 {
                lifetimes.insert("5 min");
            }
            r.weighted_tokens += c.weighted();
        }

        // --- cache rebuilds
        for i in 0..calls.len() {
            let c = &calls[i];
            let rebuilt = c.create >= MIN_REBUILD_TOKENS && c.create as f64 >= REBUILD_SHARE * c.context() as f64;
            if i == 0 {
                r.cache.normal_rebuilds += rebuilt as u32;
                continue;
            }
            let prev = &calls[i - 1];
            let gap = c.t - prev.t;
            let normal = compacted_between(prev.t, c.t) || (!prev.model.is_empty() && c.model != prev.model);
            if !normal {
                let bucket = GAP_EDGES.iter().position(|(edge, _)| gap < edge * 60).unwrap_or(GAP_EDGES.len() - 1);
                r.cache.by_gap[bucket].replies += 1;
                r.cache.by_gap[bucket].rebuilds += rebuilt as u32;
            }
            if !rebuilt {
                continue;
            }
            if normal {
                r.cache.normal_rebuilds += 1;
            } else if gap > ttl_before(&calls[..i]) + 5 {
                r.cache.break_rebuilds += 1;
                r.cache.break_tokens += c.create;
                break_gaps.push(gap);
            } else {
                r.cache.other_rebuilds += 1;
            }
        }

        // --- re-reads of unchanged files (state resets at /compact)
        let mut known_unchanged: HashMap<String, bool> = HashMap::new();
        for (t, step) in &steps {
            if *t < since || *t > until {
                // still track state, but only count inside the period
                match step {
                    Step::Compact => known_unchanged.clear(),
                    Step::Edit { path } => {
                        known_unchanged.insert(norm(path), false);
                    }
                    Step::Read { tool_id, path } => {
                        if s.reads.get(tool_id).is_some_and(|r| r.full) {
                            known_unchanged.insert(norm(path), true);
                        }
                    }
                }
                continue;
            }
            match step {
                Step::Compact => known_unchanged.clear(),
                Step::Edit { path } => {
                    known_unchanged.insert(norm(path), false);
                }
                Step::Read { tool_id, path } => {
                    let Some(res) = s.reads.get(tool_id).filter(|r| r.full) else { continue };
                    let key = norm(path);
                    if known_unchanged.get(&key) == Some(&true) {
                        let f = rereads.entry(key.clone()).or_insert_with(|| RereadFile {
                            path: path.clone(),
                            project: project_name(&s.cwd),
                            project_dir: s.cwd.clone(),
                            ..Default::default()
                        });
                        f.rereads += 1;
                        f.total_lines = f.total_lines.max(res.total_lines);
                        f.tokens += res.chars / 4; // ≈ 4 characters per token
                        reread_sessions.entry(key.clone()).or_default().insert(s.id.clone());
                    }
                    known_unchanged.insert(key, true);
                }
            }
        }

        // --- long sessions: cost per reply by age (age restarts at /compact)
        let mut start = calls[0].t;
        let mut went_long = false;
        for (i, c) in calls.iter().enumerate() {
            if i > 0 && compacted_between(calls[i - 1].t, c.t) {
                start = c.t;
            }
            let age = c.t - start;
            if age < EARLY_SECS {
                early.push(c.weighted());
            } else if age >= LATE_SECS {
                late.push(c.weighted());
                went_long = true;
            }
        }
        long_sessions += went_long as u32;
    }

    r.projects = projects.len() as u32;
    let mut lt: Vec<String> = lifetimes.into_iter().map(String::from).collect();
    lt.sort();
    r.cache.lifetimes = lt;
    break_gaps.sort();
    if !break_gaps.is_empty() {
        let at = |q: f64| break_gaps[((break_gaps.len() - 1) as f64 * q).round() as usize] / 60;
        r.cache.typical_break = Some((at(0.25), at(0.75)));
    }
    for (key, f) in rereads.iter_mut() {
        f.sessions = reread_sessions.get(key).map_or(0, |s| s.len() as u32);
    }
    let mut files: Vec<RereadFile> = rereads.into_values().filter(|f| f.rereads >= MIN_REREADS).collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.tokens));
    r.rereads = files;
    let avg = |v: &[u64]| if v.is_empty() { 0 } else { v.iter().sum::<u64>() / v.len() as u64 };
    r.long = LongSessions {
        sessions: long_sessions,
        early_avg: avg(&early),
        late_avg: avg(&late),
        late_replies: late.len() as u32,
        extra_tokens: late.iter().map(|x| x.saturating_sub(avg(&early))).sum(),
    };
    r.findings = findings(&r);
    r
}

fn thousands(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.0}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

fn findings(r: &Report) -> Vec<Finding> {
    let share = |t: u64| if r.weighted_tokens == 0 { 0.0 } else { t as f64 * 100.0 / r.weighted_tokens as f64 };
    let mut out = Vec::new();
    let c = &r.cache;
    if c.break_rebuilds >= 3 {
        let breaks = match c.typical_break {
            Some((a, b)) if a == b => format!("breaks of about {a} minutes"),
            Some((a, b)) => format!("breaks of {a}–{b} minutes"),
            None => "breaks".into(),
        };
        let life = if c.lifetimes.is_empty() { "a few minutes".to_string() } else { c.lifetimes.join(" or ") };
        out.push(Finding {
            kind: "cache".into(),
            title: format!("{} cache rebuilds after breaks", c.break_rebuilds),
            detail: format!("Your {breaks} let Claude's cache expire (it lasts {life} here), so the whole conversation was sent again: ≈ {} tokens.", thousands(c.break_tokens)),
            tip: "Before a break, finish the thought. After a long break, start with a short summary (/compact) instead of carrying on.".into(),
            tokens: c.break_tokens,
            share: share(c.break_tokens),
            file: None,
            project_dir: None,
        });
    }
    for f in r.rereads.iter().take(3) {
        let size = if f.total_lines > 0 { format!(" ({} lines)", f.total_lines) } else { String::new() };
        let big = f.total_lines >= 800;
        out.push(Finding {
            kind: "reread".into(),
            title: format!("{}{size} re-read {} times", file_name(&f.path), f.rereads),
            detail: format!(
                "Claude read the whole file again {} times without it changing, in {} session{} of {}: ≈ {} tokens.",
                f.rereads,
                f.sessions,
                if f.sessions == 1 { "" } else { "s" },
                f.project,
                thousands(f.tokens)
            ),
            tip: if big {
                "Split it into smaller files, or tell Claude which part to look at.".into()
            } else {
                "Note in CLAUDE.md what's in this file, so Claude doesn't need to re-check it.".into()
            },
            tokens: f.tokens,
            share: share(f.tokens),
            file: Some(f.path.clone()),
            project_dir: Some(f.project_dir.clone()),
        });
    }
    let l = &r.long;
    if l.late_replies >= 20 && l.early_avg > 0 && l.late_avg >= 2 * l.early_avg {
        out.push(Finding {
            kind: "long".into(),
            title: format!("Long sessions cost {:.1}× more per step", l.late_avg as f64 / l.early_avg as f64),
            detail: format!(
                "After two hours without /compact, each reply carried ≈ {} tokens instead of ≈ {} in the first hour ({} session{}): ≈ {} extra tokens.",
                thousands(l.late_avg),
                thousands(l.early_avg),
                l.sessions,
                if l.sessions == 1 { "" } else { "s" },
                thousands(l.extra_tokens)
            ),
            tip: "Use /compact when you switch tasks, or start a fresh session with a short summary.".into(),
            tokens: l.extra_tokens,
            share: share(l.extra_tokens),
            file: None,
            project_dir: None,
        });
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.tokens));
    out
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The weekly "case report" as plain text.
pub fn render_text(r: &Report) -> String {
    let date = |t: i64| chrono::DateTime::from_timestamp(t, 0).map(|d| d.with_timezone(&chrono::Local).format("%a %d %b").to_string()).unwrap_or_default();
    let mut out = format!("Glowby's case report · {} – {}\n", date(r.since), date(r.until));
    out += &format!(
        "Looked at {} session{} in {} project{}: {} replies, ≈ {} weighted tokens (cache reads count 1/10).\n\n",
        r.sessions,
        if r.sessions == 1 { "" } else { "s" },
        r.projects,
        if r.projects == 1 { "" } else { "s" },
        r.replies,
        thousands(r.weighted_tokens)
    );
    if r.findings.is_empty() {
        out += "No big leaks found. Nice!\n";
    }
    for (i, f) in r.findings.iter().enumerate() {
        out += &format!("{}. {} (≈ {:.0}% of your week)\n   {}\n   Tip: {}\n", i + 1, f.title, f.share, f.detail, f.tip);
    }
    let c = &r.cache;
    out += &format!(
        "\nNot counted as waste: {} normal cache rebuilds (first reply, after /compact or a model switch) and {} without a break (Claude Code changed tools or settings, or server side).\n",
        c.normal_rebuilds, c.other_rebuilds
    );
    let gaps: Vec<String> = c
        .by_gap
        .iter()
        .filter(|b| b.replies > 0)
        .map(|b| format!("{} {}% ({}/{})", b.label, b.rebuilds * 100 / b.replies.max(1), b.rebuilds, b.replies))
        .collect();
    if !gaps.is_empty() {
        out += &format!("Cache rebuilds by pause before the reply: {}\n", gaps.join(" · "));
    }
    out += "All numbers are estimates from your local Claude Code logs; nothing was sent anywhere.\n";
    out
}

pub fn analyze_dir(root: &Path, since: i64, until: i64) -> Report {
    analyze(&load_sessions(root, since), since, until)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T0: i64 = 1_790_000_000;

    fn ts(t: i64) -> String {
        chrono::DateTime::from_timestamp(t, 0).unwrap().to_rfc3339()
    }

    fn reply(id: &str, t: i64, create: u64, read: u64, ttl_1h: bool) -> String {
        let cc = if ttl_1h { json!({ "ephemeral_1h_input_tokens": create, "ephemeral_5m_input_tokens": 0 }) } else { json!({ "ephemeral_1h_input_tokens": 0, "ephemeral_5m_input_tokens": create }) };
        json!({ "type": "assistant", "sessionId": "s1", "cwd": r"C:\code\app", "timestamp": ts(t), "isSidechain": false,
            "message": { "id": id, "model": "opus", "content": [{ "type": "text", "text": "secret words" }],
                "usage": { "input_tokens": 10, "output_tokens": 500, "cache_creation_input_tokens": create, "cache_read_input_tokens": read, "cache_creation": cc } } })
        .to_string()
    }

    fn read_call(id: &str, tool: &str, t: i64, path: &str) -> String {
        json!({ "type": "assistant", "sessionId": "s1", "timestamp": ts(t),
            "message": { "id": id, "model": "opus", "content": [{ "type": "tool_use", "id": tool, "name": "Read", "input": { "file_path": path } }],
                "usage": { "input_tokens": 5, "output_tokens": 50, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 90_000 } } })
        .to_string()
    }

    fn read_result(tool: &str, t: i64, lines: u64, total: u64) -> String {
        json!({ "type": "user", "sessionId": "s1", "timestamp": ts(t),
            "message": { "content": [{ "type": "tool_result", "tool_use_id": tool, "content": "…" }] },
            "toolUseResult": { "type": "text", "file": { "filePath": "x", "content": "x".repeat(4000), "numLines": lines, "startLine": 1, "totalLines": total } } })
        .to_string()
    }

    fn run(lines: Vec<String>) -> Report {
        let mut sessions = BTreeMap::new();
        parse_lines(lines, &mut sessions);
        analyze(&sessions, T0 - 10, T0 + 100_000)
    }

    #[test]
    fn a_reply_split_over_several_lines_counts_once_and_unknown_lines_are_skipped() {
        let line = reply("m1", T0, 50_000, 0, false);
        let r = run(vec![line.clone(), line, "not json".into(), json!({ "type": "brand-new-kind" }).to_string()]);
        assert_eq!(r.replies, 1);
    }

    #[test]
    fn breaks_longer_than_the_cache_lifetime_are_found_but_normal_rebuilds_are_not() {
        let mut lines = vec![reply("a", T0, 60_000, 0, false)]; // first reply: normal
        lines.push(reply("b", T0 + 60, 200, 60_000, false)); // cache hit
        lines.push(reply("c", T0 + 60 + 8 * 60, 61_000, 0, false)); // 8-min break, 5-min cache: rebuild
        lines.push(reply("d", T0 + 60 + 9 * 60, 300, 61_000, false));
        lines.push(json!({ "type": "system", "subtype": "compact_boundary", "sessionId": "s1", "timestamp": ts(T0 + 60 + 20 * 60) }).to_string());
        lines.push(reply("e", T0 + 60 + 30 * 60, 20_000, 0, false)); // after /compact: normal
        let r = run(lines);
        assert_eq!(r.cache.break_rebuilds, 1);
        assert_eq!(r.cache.break_tokens, 61_000);
        assert_eq!(r.cache.normal_rebuilds, 2);
        assert_eq!(r.cache.lifetimes, vec!["5 min".to_string()]);
    }

    #[test]
    fn a_one_hour_cache_survives_a_ten_minute_break() {
        let lines = vec![reply("a", T0, 60_000, 0, true), reply("b", T0 + 600, 61_000, 0, true)];
        let r = run(lines);
        assert_eq!(r.cache.break_rebuilds, 0);
        assert_eq!(r.cache.other_rebuilds, 1, "a rebuild without a long enough break isn't the user's fault");
    }

    #[test]
    fn only_full_rereads_of_unchanged_files_count() {
        let p = r"C:\code\app\src\big.rs";
        let mut lines = Vec::new();
        let mut t = T0;
        for i in 0..5 {
            lines.push(read_call(&format!("r{i}"), &format!("t{i}"), t, p));
            lines.push(read_result(&format!("t{i}"), t + 1, 2000, 2000));
            t += 60;
        }
        // an edit, then a read: legitimate
        lines.push(json!({ "type": "assistant", "sessionId": "s1", "timestamp": ts(t), "message": { "id": "e1", "content": [{ "type": "tool_use", "id": "te", "name": "Edit", "input": { "file_path": p } }] } }).to_string());
        lines.push(read_call("r9", "t9", t + 10, p));
        lines.push(read_result("t9", t + 11, 2000, 2000));
        // a partial read never counts
        lines.push(read_call("r10", "t10", t + 20, p));
        lines.push(read_result("t10", t + 21, 100, 2000));
        let r = run(lines);
        assert_eq!(r.rereads.len(), 1);
        assert_eq!(r.rereads[0].rereads, 4, "5 full reads without changes = 4 wasted re-reads");
        assert_eq!(r.rereads[0].total_lines, 2000);
        assert_eq!(r.rereads[0].tokens, 4 * 1000);
    }

    #[test]
    fn compact_resets_what_claude_has_seen() {
        let p = "src/a.rs";
        let mut lines = Vec::new();
        for i in 0..4 {
            let t = T0 + i * 100;
            if i == 2 {
                lines.push(json!({ "type": "user", "sessionId": "s1", "timestamp": ts(t - 5), "isCompactSummary": true, "message": { "content": "summary" } }).to_string());
            }
            lines.push(read_call(&format!("r{i}"), &format!("t{i}"), t, p));
            lines.push(read_result(&format!("t{i}"), t + 1, 50, 50));
        }
        let mut sessions = BTreeMap::new();
        parse_lines(lines, &mut sessions);
        let r = analyze(&sessions, T0 - 10, T0 + 100_000);
        // reads 0,1 | compact | 2,3  →  wasted: 1 and 3 = 2 (below the 3-read threshold)
        assert!(r.rereads.is_empty());
    }

    #[test]
    fn report_text_explains_itself() {
        let mut lines = vec![reply("a", T0, 60_000, 0, false)];
        for i in 1..=4 {
            lines.push(reply(&format!("b{i}"), T0 + i * 900, 60_000, 0, false));
        }
        let r = run(lines);
        assert!(r.findings.iter().any(|f| f.kind == "cache"));
        let text = render_text(&r);
        assert!(text.contains("cache rebuilds after breaks") && text.contains("Not counted as waste"));
        assert!(!text.contains("secret words"), "message text never appears");
    }
}
