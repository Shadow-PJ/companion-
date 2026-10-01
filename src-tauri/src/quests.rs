//! Daily quests: three small goals a day ("Fix 1 bug", "Code for 30 minutes"…)
//! with XP rewards. Saved in quests.json. The day's quests are picked with the
//! date as the random seed, so restarting Glowby never rerolls them.

use crate::settings::{self, Difficulty, QuestSettings};
use crate::state::{self, AppState, lock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

pub const ALL_KINDS: &[&str] = &["fix", "test", "minutes", "tasks", "commit", "learn", "break"];
const ALL_DONE_BONUS: u32 = 20;
/// Gaps longer than this between events don't count as coding time.
const MAX_TICK_GAP_SECS: i64 = 5 * 60;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Quest {
    pub kind: String,
    pub target: u32,
    pub progress: u32,
    pub done: bool,
    pub xp: u32,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct QuestBook {
    pub day: String,
    pub quests: Vec<Quest>,
    pub coding_secs: u64,
    pub last_tick: String,
    /// Test files already counted today (one file = one test written).
    pub tested_files: Vec<String>,
    pub bonus_given: bool,
    /// The day the daily briefing was last shown.
    pub briefing_day: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestView {
    pub kind: String,
    pub label: String,
    pub progress: u32,
    pub target: u32,
    pub done: bool,
    pub xp: u32,
}

pub fn target_for(kind: &str, d: Difficulty) -> u32 {
    let (easy, normal, hard) = match kind {
        "fix" => (1, 1, 2),
        "test" => (1, 2, 3),
        "minutes" => (20, 30, 60),
        "tasks" => (2, 4, 8),
        "commit" => (1, 2, 4),
        "learn" => (1, 2, 3),
        "break" => (1, 1, 2),
        _ => (1, 1, 1),
    };
    match d {
        Difficulty::Easy => easy,
        Difficulty::Normal => normal,
        Difficulty::Hard => hard,
    }
}

pub fn xp_for(d: Difficulty) -> u32 {
    match d {
        Difficulty::Easy => 15,
        Difficulty::Normal => 25,
        Difficulty::Hard => 40,
    }
}

pub fn label(kind: &str, n: u32) -> String {
    let s = if n == 1 { "" } else { "s" };
    match kind {
        "fix" => format!("Fix {n} bug{s}"),
        "test" => format!("Write {n} test{s}"),
        "minutes" => format!("Code for {n} minutes"),
        "tasks" => format!("Finish {n} task{s} with Claude"),
        "commit" => format!("Make {n} commit{s}"),
        "learn" => format!("Answer {n} learn-mode question{s} right"),
        "break" => format!("Take {n} break{s}"),
        other => other.to_string(),
    }
}

/// Deterministic shuffle: same day + same settings = same quests.
fn pick(kinds: &[String], n: usize, day: &str) -> Vec<String> {
    let mut seed: u64 = day.bytes().fold(1469598103934665603, |h, b| (h ^ b as u64).wrapping_mul(1099511628211));
    let mut pool: Vec<String> = kinds.to_vec();
    let mut chosen = Vec::new();
    while !pool.is_empty() && chosen.len() < n {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let i = (seed >> 33) as usize % pool.len();
        chosen.push(pool.remove(i));
    }
    chosen
}

impl QuestBook {
    /// Starts a new day if needed. `available` = quest kinds that make sense now
    /// (e.g. no learn quests while learn mode is off).
    pub fn ensure_day(&mut self, today: &str, s: &QuestSettings, available: &[String]) {
        if self.day == today {
            return;
        }
        let briefing_day = std::mem::take(&mut self.briefing_day);
        *self = QuestBook { day: today.into(), briefing_day, ..Default::default() };
        let kinds: Vec<String> = s.kinds.iter().filter(|k| available.contains(k)).cloned().collect();
        self.quests = pick(&kinds, s.per_day as usize, today)
            .into_iter()
            .map(|kind| Quest { target: target_for(&kind, s.difficulty), progress: 0, done: false, xp: xp_for(s.difficulty), kind })
            .collect();
    }

    /// Adds progress; returns quests that just got completed.
    pub fn bump(&mut self, kind: &str, amount: u32) -> Vec<Quest> {
        self.update(kind, |p| p + amount)
    }

    pub fn set(&mut self, kind: &str, value: u32) -> Vec<Quest> {
        self.update(kind, |_| value)
    }

    fn update(&mut self, kind: &str, f: impl Fn(u32) -> u32) -> Vec<Quest> {
        let mut finished = Vec::new();
        for q in self.quests.iter_mut().filter(|q| q.kind == kind && !q.done) {
            q.progress = f(q.progress).min(q.target);
            if q.progress >= q.target {
                q.done = true;
                finished.push(q.clone());
            }
        }
        finished
    }

    pub fn all_done(&self) -> bool {
        !self.quests.is_empty() && self.quests.iter().all(|q| q.done)
    }

    pub fn views(&self) -> Vec<QuestView> {
        self.quests
            .iter()
            .map(|q| QuestView { kind: q.kind.clone(), label: label(&q.kind, q.target), progress: q.progress, target: q.target, done: q.done, xp: q.xp })
            .collect()
    }
}

/// A file looks like a test file and the new code contains a test.
pub fn written_test_file(payload: &Value) -> Option<String> {
    let tool = payload.get("tool_name")?.as_str()?;
    if !matches!(tool, "Edit" | "MultiEdit" | "Write") {
        return None;
    }
    let input = payload.get("tool_input")?;
    let path = input.get("file_path")?.as_str()?;
    let lower = path.to_lowercase().replace('\\', "/");
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let looks_like_test_file = name.contains("test") || name.contains("spec") || lower.contains("/tests/") || lower.contains("/__tests__/");
    let mut new_code = String::new();
    for key in ["new_string", "content"] {
        if let Some(s) = input.get(key).and_then(Value::as_str) {
            new_code.push_str(s);
        }
    }
    if let Some(edits) = input.get("edits").and_then(Value::as_array) {
        for e in edits {
            new_code.push_str(e.get("new_string").and_then(Value::as_str).unwrap_or(""));
        }
    }
    const MARKERS: &[&str] = &["#[test]", "def test_", "it(", "test(", "describe(", "@Test", "func Test", "[Fact]", "[Test]", "TEST_CASE("];
    let has_test = MARKERS.iter().any(|m| new_code.contains(m));
    // Rust keeps unit tests inside normal files, so a #[test] counts anywhere.
    (has_test && (looks_like_test_file || new_code.contains("#[test]"))).then(|| path.to_string())
}

// ---------------------------------------------------------------- app glue

fn available_kinds(app: &AppHandle) -> Vec<String> {
    let s = app.state::<AppState>().settings();
    ALL_KINDS
        .iter()
        .filter(|k| match **k {
            "learn" => s.learn.enabled,
            "break" => s.breaks.enabled,
            _ => true,
        })
        .map(|k| k.to_string())
        .collect()
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Runs `f` on today's quest book (creating today's quests first), then saves.
pub fn with_book<T>(app: &AppHandle, f: impl FnOnce(&mut QuestBook) -> T) -> T {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let available = available_kinds(app);
    let (result, snapshot) = {
        let mut book = lock(&state.quests);
        book.ensure_day(&today(), &settings.quests, &available);
        let r = f(&mut book);
        (r, serde_json::to_value(&*book).ok())
    };
    if let Some(v) = snapshot {
        let _ = settings::save_json(&state.paths.quests_file, &v);
    }
    result
}

pub fn views(app: &AppHandle) -> Vec<QuestView> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    if !settings.quests.enabled {
        return Vec::new();
    }
    let available = available_kinds(app);
    let mut book = lock(&state.quests);
    book.ensure_day(&today(), &settings.quests, &available);
    book.views()
}

/// Something happened that counts towards a quest.
pub fn progress(app: &AppHandle, kind: &str, amount: u32) {
    if !app.state::<AppState>().settings().quests.enabled {
        return;
    }
    let (finished, bonus) = with_book(app, |book| {
        let finished = book.bump(kind, amount);
        let bonus = !finished.is_empty() && book.all_done() && !std::mem::replace(&mut book.bonus_given, true);
        (finished, bonus)
    });
    celebrate(app, finished, bonus);
}

fn celebrate(app: &AppHandle, finished: Vec<Quest>, bonus: bool) {
    for q in &finished {
        state::toast(app, "levelup", format!("Quest complete: {} (+{} XP)", label(&q.kind, q.target), q.xp), String::new(), 7);
        crate::progress::award(app, q.xp, "quest");
    }
    if bonus {
        state::toast(app, "levelup", format!("All of today's quests done! (+{ALL_DONE_BONUS} XP bonus)"), String::new(), 8);
        crate::progress::award(app, ALL_DONE_BONUS, "all quests");
    }
    if !finished.is_empty() {
        crate::sounds::play(app, crate::sounds::Sound::LevelUp);
        crate::pet_window::show(app);
        state::publish(app);
    }
}

/// Coding time: called on every hook event of your sessions. Time between
/// events counts if the gap is short (so lunch breaks don't count).
pub fn tick(app: &AppHandle) {
    if !app.state::<AppState>().settings().quests.enabled {
        return;
    }
    let now = chrono::Local::now();
    let state = app.state::<AppState>();
    let settings = state.settings();
    let available = available_kinds(app);
    let (finished, bonus, minute_changed) = {
        let mut book = lock(&state.quests);
        book.ensure_day(&today(), &settings.quests, &available);
        let gap = chrono::DateTime::parse_from_rfc3339(&book.last_tick).map(|t| (now - t.with_timezone(&chrono::Local)).num_seconds()).unwrap_or(i64::MAX);
        book.last_tick = now.to_rfc3339();
        let before = book.coding_secs / 60;
        if (0..=MAX_TICK_GAP_SECS).contains(&gap) {
            book.coding_secs += gap as u64;
        }
        let minutes = (book.coding_secs / 60) as u32;
        let finished = book.set("minutes", minutes);
        let bonus = !finished.is_empty() && book.all_done() && !std::mem::replace(&mut book.bonus_given, true);
        (finished, bonus, minutes as u64 != before)
    };
    if minute_changed || !finished.is_empty() {
        with_book(app, |_| ()); // save
    }
    celebrate(app, finished, bonus);
}

/// A test file was written: counts once per file per day.
pub fn test_written(app: &AppHandle, file: String) {
    let new = with_book(app, |book| {
        let key = file.to_lowercase();
        if book.tested_files.contains(&key) {
            false
        } else {
            book.tested_files.push(key);
            true
        }
    });
    if new {
        progress(app, "test", 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings() -> QuestSettings {
        QuestSettings::default()
    }

    #[test]
    fn same_day_same_quests_new_day_new_quests() {
        let all: Vec<String> = ALL_KINDS.iter().map(|s| s.to_string()).collect();
        let mut a = QuestBook::default();
        a.ensure_day("2026-10-01", &settings(), &all);
        let mut b = QuestBook::default();
        b.ensure_day("2026-10-01", &settings(), &all);
        assert_eq!(a.quests, b.quests);
        assert_eq!(a.quests.len(), 3);
        let first: Vec<String> = a.quests.iter().map(|q| q.kind.clone()).collect();
        let mut kinds_vary = false;
        for day in ["2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05"] {
            let mut c = QuestBook::default();
            c.ensure_day(day, &settings(), &all);
            kinds_vary |= c.quests.iter().map(|q| q.kind.clone()).collect::<Vec<_>>() != first;
        }
        assert!(kinds_vary);
    }

    #[test]
    fn progress_completes_quests_once() {
        let mut book = QuestBook::default();
        book.ensure_day("2026-10-01", &settings(), &["commit".to_string()]);
        assert_eq!(book.quests.len(), 1, "only available kinds are picked");
        assert!(book.bump("commit", 1).is_empty());
        assert_eq!(book.bump("commit", 1).len(), 1);
        assert!(book.bump("commit", 1).is_empty(), "already done");
        assert!(book.all_done());
    }

    #[test]
    fn detects_written_tests() {
        let py = json!({ "tool_name": "Write", "tool_input": { "file_path": "C:\\app\\tests\\test_math.py", "content": "def test_add():\n    assert add(1, 2) == 3" } });
        assert!(written_test_file(&py).is_some());
        let rust = json!({ "tool_name": "Edit", "tool_input": { "file_path": "src/math.rs", "old_string": "x", "new_string": "#[test]\nfn adds() {}" } });
        assert!(written_test_file(&rust).is_some());
        let not_test = json!({ "tool_name": "Edit", "tool_input": { "file_path": "src/app.ts", "old_string": "a", "new_string": "const x = test(1)" } });
        assert!(written_test_file(&not_test).is_none());
    }

    #[test]
    fn labels_read_naturally() {
        assert_eq!(label("fix", 1), "Fix 1 bug");
        assert_eq!(label("test", 2), "Write 2 tests");
        assert_eq!(label("minutes", 30), "Code for 30 minutes");
    }
}
