//! Break reminder. Glowby counts how long you've been coding without a pause;
//! after the interval from Settings he gets sleepy and suggests a short break.
//!
//! "Coding" = you send prompts to Claude Code or use Glowby. A gap of 15
//! minutes counts as a break and starts a new session. No polling: the moment
//! a reminder is due goes into the same single mood timer as everything else.

use crate::actions::Offer;
use crate::sounds::{self, Sound};
use crate::state::{self, AppState, lock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// This long without activity counts as having taken a break.
pub const BREAK_GAP: Duration = Duration::from_secs(15 * 60);
const SNOOZE: Duration = Duration::from_secs(15 * 60);
/// The suggestion stays on offer this long.
const OFFER_FOR: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct Breaks {
    session_start: Option<Instant>,
    last_activity: Option<Instant>,
    snooze_until: Option<Instant>,
    reminded: bool,
}

impl Breaks {
    pub fn activity(&mut self, now: Instant) {
        if self.last_activity.is_none_or(|t| now.duration_since(t) > BREAK_GAP) {
            self.session_start = Some(now);
            self.reminded = false;
            self.snooze_until = None;
        }
        self.last_activity = Some(now);
    }

    /// When the next reminder is due (None: not in a session, or already reminded).
    pub fn due_at(&self, now: Instant, interval: Duration) -> Option<Instant> {
        let (start, last) = (self.session_start?, self.last_activity?);
        if self.reminded || now.duration_since(last) > BREAK_GAP {
            return None;
        }
        let due = start + interval;
        Some(self.snooze_until.map_or(due, |s| s.max(due)))
    }

    pub fn session_length(&self, now: Instant) -> Duration {
        self.session_start.map(|s| now.duration_since(s)).unwrap_or_default()
    }

    pub fn took_break(&mut self) {
        *self = Breaks::default();
    }

    pub fn snooze(&mut self, now: Instant) {
        self.snooze_until = Some(now + SNOOZE);
        self.reminded = false;
    }
}

pub fn activity(app: &AppHandle) {
    lock(&app.state::<AppState>().breaks).activity(Instant::now());
}

/// Called when the mood timer fires: shows the reminder if it's due.
pub fn check(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    if !settings.breaks.enabled {
        return;
    }
    let now = Instant::now();
    let interval = Duration::from_secs(settings.breaks.interval_mins as u64 * 60);
    let length = {
        let mut breaks = lock(&state.breaks);
        match breaks.due_at(now, interval) {
            Some(due) if due <= now => {
                breaks.reminded = true;
                breaks.session_length(now)
            }
            _ => return,
        }
    };
    let mins = length.as_secs() / 60;
    let detail = if mins >= 60 { format!("You've been coding for {} h {:02} min.", mins / 60, mins % 60) } else { format!("You've been coding for {mins} minutes.") };
    let offer = Offer { kind: "break", title: "Time for a short break?".into(), detail, project: String::new() };
    lock(&state.ui).offer = Some((offer, now + OFFER_FOR));
    sounds::play(app, Sound::Break);
    crate::pet_window::show(app);
    state::publish(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Activity every 10 minutes from t0 until t0 + `mins`.
    fn busy(b: &mut Breaks, t0: Instant, mins: u64) {
        for m in (0..=mins).step_by(10) {
            b.activity(t0 + Duration::from_secs(m * 60));
        }
    }

    #[test]
    fn reminds_after_the_interval_of_continuous_activity() {
        let t0 = Instant::now();
        let hour = Duration::from_secs(3600);
        let mut b = Breaks::default();
        busy(&mut b, t0, 50);
        assert_eq!(b.due_at(t0 + Duration::from_secs(50 * 60), hour), Some(t0 + hour));
    }

    #[test]
    fn a_pause_counts_as_a_break() {
        let t0 = Instant::now();
        let mut b = Breaks::default();
        b.activity(t0);
        let later = t0 + Duration::from_secs(40 * 60); // 40 min of silence
        assert!(b.due_at(later, Duration::from_secs(3600)).is_none());
        b.activity(later);
        assert_eq!(b.session_length(later), Duration::ZERO, "new session started");
    }

    #[test]
    fn snooze_moves_the_reminder() {
        let t0 = Instant::now();
        let mut b = Breaks::default();
        busy(&mut b, t0, 60);
        let at = t0 + Duration::from_secs(3600);
        b.reminded = true;
        b.snooze(at);
        assert_eq!(b.due_at(at, Duration::from_secs(3600)), Some(at + SNOOZE));
    }
}
