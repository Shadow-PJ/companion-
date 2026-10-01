//! Glowby's progression: XP, levels, evolution stages, cosmetics, daily
//! streak and energy. Saved in %APPDATA%\dev.glowby.app\progress.json.
//!
//! The first half is pure logic (easy to test); the second half connects it
//! to the app (toasts, sounds, emotes).

use crate::settings::{self, Settings};
use crate::sounds::{self, Sound};
use crate::state::{self, AppState, lock};
use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tauri::{AppHandle, Emitter, Manager};

// ---------------------------------------------------------------- XP amounts

pub const TASK_XP: u32 = 10;
pub const CHAT_XP: u32 = 2;
pub const FIX_XP: u32 = 30;
pub const TESTS_PASSED_XP: u32 = 5;
pub const COMMIT_XP: u32 = 8;
pub const BREAK_XP: u32 = 5;
/// Passing tests give XP at most this often per project (no farming by re-running).
const TEST_XP_COOLDOWN_MINS: i64 = 10;

// ---------------------------------------------------------------- model

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Progress {
    pub xp: u64,
    pub streak: u32,
    pub best_streak: u32,
    /// Last coding day, YYYY-MM-DD (local time).
    pub last_day: String,
    /// Last time you coded or played with Glowby (RFC 3339).
    pub last_seen: String,
    pub stats: Stats,
    /// Project folder → when tests-passed XP was last given.
    pub last_test_xp: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Stats {
    pub tasks: u32,
    pub fixes: u32,
    pub tests_passed: u32,
    pub commits: u32,
    pub breaks: u32,
}

/// Evolution stages: (first level, name).
pub const STAGES: [(u32, &str); 4] = [
    (1, "Little Glowby"),
    (6, "Lantern Glowby"),
    (15, "Starlit Glowby"),
    (30, "Aurora Glowby"),
];

/// Total XP needed to reach `level` (level 1 = 0 XP, 2 = 50, 3 = 150, 4 = 300 …).
pub fn threshold(level: u32) -> u64 {
    let l = level as u64;
    25 * l * l.saturating_sub(1)
}

pub fn level_for(xp: u64) -> u32 {
    let mut level = 1;
    while xp >= threshold(level + 1) {
        level += 1;
    }
    level
}

/// 0-based stage index for a level.
pub fn stage_for(level: u32) -> usize {
    STAGES.iter().rposition(|(min, _)| level >= *min).unwrap_or(0)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Hat,
    Color,
    Emote,
}

#[derive(Clone, Copy, Debug)]
pub enum Needs {
    Level(u32),
    Streak(u32),
}

pub struct Cosmetic {
    pub id: &'static str,
    pub kind: Kind,
    pub name: &'static str,
    pub needs: Needs,
}

const fn c(id: &'static str, kind: Kind, name: &'static str, needs: Needs) -> Cosmetic {
    Cosmetic { id, kind, name, needs }
}

pub const COSMETICS: &[Cosmetic] = &[
    c("wave", Kind::Emote, "Wave", Needs::Level(1)),
    c("heart", Kind::Emote, "Hearts", Needs::Level(3)),
    c("spin", Kind::Emote, "Spin", Needs::Level(6)),
    c("dance", Kind::Emote, "Dance", Needs::Level(9)),
    c("fireworks", Kind::Emote, "Fireworks", Needs::Level(18)),
    c("sprout", Kind::Hat, "Sprout", Needs::Level(2)),
    c("party", Kind::Hat, "Party hat", Needs::Level(4)),
    c("beanie", Kind::Hat, "Cozy beanie", Needs::Streak(3)),
    c("headphones", Kind::Hat, "Headphones", Needs::Level(8)),
    c("crown", Kind::Hat, "Tiny crown", Needs::Level(12)),
    c("wizard", Kind::Hat, "Wizard hat", Needs::Level(20)),
    c("gradcap", Kind::Hat, "Graduation cap", Needs::Level(25)),
    c("periwinkle", Kind::Color, "Periwinkle", Needs::Level(1)),
    c("mint", Kind::Color, "Mint", Needs::Level(3)),
    c("peach", Kind::Color, "Peach", Needs::Level(7)),
    c("lilac", Kind::Color, "Lilac", Needs::Level(10)),
    c("rose", Kind::Color, "Rose", Needs::Streak(7)),
    c("aqua", Kind::Color, "Aqua", Needs::Level(16)),
];

fn is_unlocked(cosmetic: &Cosmetic, level: u32, best_streak: u32) -> bool {
    match cosmetic.needs {
        Needs::Level(l) => level >= l,
        Needs::Streak(s) => best_streak >= s,
    }
}

fn unlocked_names(level: u32, best_streak: u32) -> Vec<&'static str> {
    COSMETICS.iter().filter(|c| is_unlocked(c, level, best_streak)).map(|c| c.name).collect()
}

pub fn requirement_text(needs: Needs) -> String {
    match needs {
        Needs::Level(l) => format!("Level {l}"),
        Needs::Streak(s) => format!("{s}-day streak"),
    }
}

/// What changed after earning XP or extending the streak.
#[derive(Default, Debug)]
pub struct Award {
    pub new_level: Option<u32>,
    pub evolved_into: Option<&'static str>,
    pub unlocked: Vec<&'static str>,
}

impl Progress {
    pub fn level(&self) -> u32 {
        level_for(self.xp)
    }

    fn diff(&self, level_before: u32, best_before: u32) -> Award {
        let level = self.level();
        let before = unlocked_names(level_before, best_before);
        let unlocked = unlocked_names(level, self.best_streak).into_iter().filter(|n| !before.contains(n)).collect();
        Award {
            new_level: (level > level_before).then_some(level),
            evolved_into: (stage_for(level) > stage_for(level_before)).then(|| STAGES[stage_for(level)].1),
            unlocked,
        }
    }

    pub fn add_xp(&mut self, amount: u32) -> Award {
        let (level_before, best_before) = (self.level(), self.best_streak);
        self.xp += amount as u64;
        self.diff(level_before, best_before)
    }

    /// Records a coding day. Returns the new streak length if today is a new day.
    pub fn touch_day(&mut self, today: NaiveDate) -> Option<(u32, Award)> {
        let last = NaiveDate::parse_from_str(&self.last_day, "%Y-%m-%d").ok();
        if last == Some(today) {
            return None;
        }
        let (level_before, best_before) = (self.level(), self.best_streak);
        self.streak = if last.and_then(|d| d.succ_opt()) == Some(today) { self.streak + 1 } else { 1 };
        self.best_streak = self.best_streak.max(self.streak);
        self.last_day = today.format("%Y-%m-%d").to_string();
        Some((self.streak, self.diff(level_before, best_before)))
    }

    /// Whole days since you last coded or played with Glowby.
    pub fn days_away(&self, now: DateTime<Local>) -> i64 {
        DateTime::parse_from_rfc3339(&self.last_seen).map(|t| (now - t.with_timezone(&Local)).num_days().max(0)).unwrap_or(0)
    }

    /// 100 = full of energy. Drops 30 per day you stay away, but never below 10:
    /// Glowby gets weak and sleepy, he never dies.
    pub fn energy(&self, now: DateTime<Local>) -> u8 {
        (100 - 30 * self.days_away(now)).clamp(10, 100) as u8
    }
}

// ---------------------------------------------------------------- app glue

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProgressView {
    pub level: u32,
    pub stage: usize,
    pub stage_name: &'static str,
    pub xp: u64,
    pub xp_into_level: u64,
    pub xp_for_level: u64,
    pub streak: u32,
    pub energy: u8,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Look {
    pub stage: usize,
    pub hat: String,
    pub color: String,
    /// Ignored for days: paler and slower.
    pub weak: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmeticView {
    pub id: &'static str,
    pub kind: Kind,
    pub name: &'static str,
    pub unlocked: bool,
    pub requirement: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressInfo {
    pub view: ProgressView,
    pub look: Look,
    pub stats: Stats,
    pub best_streak: u32,
    pub stages: Vec<(u32, &'static str)>,
    pub cosmetics: Vec<CosmeticView>,
}

#[derive(Serialize, Clone)]
struct XpMsg {
    amount: u32,
    reason: String,
}

pub fn view(p: &Progress, now: DateTime<Local>) -> ProgressView {
    let level = p.level();
    let stage = stage_for(level);
    ProgressView {
        level,
        stage,
        stage_name: STAGES[stage].1,
        xp: p.xp,
        xp_into_level: p.xp - threshold(level),
        xp_for_level: threshold(level + 1) - threshold(level),
        streak: p.streak,
        energy: p.energy(now),
    }
}

/// Equipped cosmetics, but only ones that are actually unlocked.
pub fn look(p: &Progress, settings: &Settings, now: DateTime<Local>) -> Look {
    let level = p.level();
    let ok = |id: &str, kind: Kind| COSMETICS.iter().any(|c| c.id == id && c.kind == kind && is_unlocked(c, level, p.best_streak));
    Look {
        stage: stage_for(level),
        hat: if ok(&settings.progression.hat, Kind::Hat) { settings.progression.hat.clone() } else { String::new() },
        color: if ok(&settings.progression.color, Kind::Color) { settings.progression.color.clone() } else { "periwinkle".into() },
        weak: settings.progression.neglect && p.energy(now) <= 40,
    }
}

pub fn unlocked_emotes(p: &Progress) -> Vec<(&'static str, &'static str)> {
    let level = p.level();
    COSMETICS
        .iter()
        .filter(|c| c.kind == Kind::Emote && is_unlocked(c, level, p.best_streak))
        .map(|c| (c.id, c.name))
        .collect()
}

pub fn info(app: &AppHandle) -> ProgressInfo {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let p = lock(&state.progress).clone();
    let now = Local::now();
    let level = p.level();
    ProgressInfo {
        view: view(&p, now),
        look: look(&p, &settings, now),
        stats: p.stats.clone(),
        best_streak: p.best_streak,
        stages: STAGES.to_vec(),
        cosmetics: COSMETICS
            .iter()
            .map(|c| CosmeticView {
                id: c.id,
                kind: c.kind,
                name: c.name,
                unlocked: is_unlocked(c, level, p.best_streak),
                requirement: requirement_text(c.needs),
            })
            .collect(),
    }
}

fn save(app: &AppHandle) {
    let state = app.state::<AppState>();
    let p = lock(&state.progress).clone();
    if let Err(e) = settings::save_json(&state.paths.progress_file, &p) {
        crate::applog::line(format!("couldn't save progress: {e}"));
    }
}

/// Earn XP (if progression is on). Celebrates level-ups and evolutions.
pub fn award(app: &AppHandle, amount: u32, reason: &str) {
    let state = app.state::<AppState>();
    if !lock(&state.settings).progression.enabled || amount == 0 {
        return;
    }
    let result = lock(&state.progress).add_xp(amount);
    save(app);
    let _ = app.emit_to(crate::pet_window::LABEL, "pet://xp", XpMsg { amount, reason: reason.into() });
    celebrate(app, result);
}

fn celebrate(app: &AppHandle, award: Award) {
    let Some(level) = award.new_level else {
        if !award.unlocked.is_empty() {
            state::toast(app, "levelup", format!("Unlocked: {}!", award.unlocked.join(", ")), String::new(), 8);
            state::publish(app);
        }
        return;
    };
    let mut text = match award.evolved_into {
        Some(stage) => format!("Level {level}! Glowby evolved into {stage}!"),
        None => format!("Level {level}!"),
    };
    if !award.unlocked.is_empty() {
        text.push_str(&format!(" Unlocked: {}.", award.unlocked.join(", ")));
    }
    state::toast(app, "levelup", text, String::new(), 9);
    sounds::play(app, if award.evolved_into.is_some() { Sound::Evolve } else { Sound::LevelUp });
    crate::pet_window::show(app);
    let _ = app.emit_to(crate::pet_window::LABEL, "pet://emote", "fireworks");
    state::publish(app);
}

/// You did something: coding (a prompt to Claude) or playing with Glowby.
/// Keeps the streak and energy up, and says hello after a long absence.
pub fn activity(app: &AppHandle, coding: bool) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let now = Local::now();
    let (days_away, day) = {
        let mut p = lock(&state.progress);
        let days_away = if p.last_seen.is_empty() { 0 } else { p.days_away(now) };
        p.last_seen = now.to_rfc3339();
        let day = if coding { p.touch_day(now.date_naive()) } else { None };
        (days_away, day)
    };
    save(app);
    if settings.progression.neglect && days_away >= 2 {
        state::toast(app, "info", format!("I missed you! It's been {days_away} days."), String::new(), 8);
        crate::pet_window::show(app);
    }
    if let Some((streak, unlocks)) = day
        && settings.progression.enabled
    {
        if streak >= 2 {
            state::toast(app, "levelup", format!("{streak}-day coding streak!"), String::new(), 6);
        }
        celebrate(app, unlocks);
        award(app, 5 * streak.min(7), "daily streak");
    }
    state::publish(app);
}

/// A Claude turn finished. More XP if it actually did work (used tools).
pub fn task_finished(app: &AppHandle, used_tools: bool) {
    if used_tools {
        lock(&app.state::<AppState>().progress).stats.tasks += 1;
        award(app, TASK_XP, "task done");
    } else {
        award(app, CHAT_XP, "chat");
    }
}

pub fn fixed(app: &AppHandle) {
    lock(&app.state::<AppState>().progress).stats.fixes += 1;
    award(app, FIX_XP, "fixed it");
}

pub fn committed(app: &AppHandle) {
    lock(&app.state::<AppState>().progress).stats.commits += 1;
    award(app, COMMIT_XP, "commit");
}

pub fn took_break(app: &AppHandle) {
    lock(&app.state::<AppState>().progress).stats.breaks += 1;
    award(app, BREAK_XP, "rested");
}

/// Passing tests: XP at most once per cooldown per project.
pub fn tests_passed(app: &AppHandle, project_dir: &str) {
    let state = app.state::<AppState>();
    let now = Local::now();
    let allowed = {
        let mut p = lock(&state.progress);
        p.stats.tests_passed += 1;
        let key = project_dir.to_lowercase();
        let recent = p
            .last_test_xp
            .get(&key)
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .is_some_and(|t| (now - t.with_timezone(&Local)).num_minutes() < TEST_XP_COOLDOWN_MINS);
        if !recent {
            p.last_test_xp.insert(key, now.to_rfc3339());
        }
        !recent
    };
    if allowed {
        award(app, TESTS_PASSED_XP, "tests passed");
    } else {
        save(app);
    }
}

/// When energy will next drop (so the mood timer can update Glowby's look).
pub fn next_energy_drop(p: &Progress, now: DateTime<Local>) -> Option<std::time::Instant> {
    let seen = DateTime::parse_from_rfc3339(&p.last_seen).ok()?.with_timezone(&Local);
    let next = seen + chrono::Duration::days(p.days_away(now) + 1);
    let wait = (next - now).to_std().ok()?;
    Some(std::time::Instant::now() + wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn levels_follow_the_curve() {
        assert_eq!(level_for(0), 1);
        assert_eq!(level_for(49), 1);
        assert_eq!(level_for(50), 2);
        assert_eq!(level_for(150), 3);
        assert_eq!(level_for(threshold(6)), 6);
        assert_eq!(stage_for(5), 0);
        assert_eq!(stage_for(6), 1);
        assert_eq!(stage_for(15), 2);
        assert_eq!(stage_for(40), 3);
    }

    #[test]
    fn level_ups_report_unlocks_and_evolution() {
        let mut p = Progress::default();
        let a = p.add_xp(50);
        assert_eq!(a.new_level, Some(2));
        assert!(a.unlocked.contains(&"Sprout"));
        assert!(a.evolved_into.is_none());
        let a = p.add_xp((threshold(6) - p.xp) as u32);
        assert_eq!(a.new_level, Some(6));
        assert_eq!(a.evolved_into, Some("Lantern Glowby"));
    }

    #[test]
    fn streak_counts_consecutive_days_and_resets_after_a_gap() {
        let mut p = Progress::default();
        let d = |n| NaiveDate::from_ymd_opt(2026, 10, n).unwrap();
        assert_eq!(p.touch_day(d(1)).map(|x| x.0), Some(1));
        assert!(p.touch_day(d(1)).is_none(), "same day twice");
        assert_eq!(p.touch_day(d(2)).map(|x| x.0), Some(2));
        let (streak, award) = p.touch_day(d(3)).unwrap();
        assert_eq!(streak, 3);
        assert!(award.unlocked.contains(&"Cozy beanie"));
        assert_eq!(p.touch_day(d(5)).map(|x| x.0), Some(1), "missed a day");
        assert_eq!(p.best_streak, 3);
    }

    #[test]
    fn energy_drops_when_ignored_but_never_reaches_zero() {
        let now = Local.with_ymd_and_hms(2026, 10, 10, 12, 0, 0).unwrap();
        let mut p = Progress { last_seen: (now - chrono::Duration::hours(3)).to_rfc3339(), ..Default::default() };
        assert_eq!(p.energy(now), 100);
        p.last_seen = (now - chrono::Duration::days(2)).to_rfc3339();
        assert_eq!(p.energy(now), 40);
        p.last_seen = (now - chrono::Duration::days(30)).to_rfc3339();
        assert_eq!(p.energy(now), 10);
    }
}
