//! Shared app state + `publish()`, which turns the state into what the pet shows.
//!
//! Locking rule (avoids deadlocks): take a lock, copy what you need, drop it,
//! and only THEN call Win32 window functions or emit events.

use crate::chat::{self, ChatState};
use crate::permissions::{PermView, Queue};
use crate::sessions::{Mood, StatusView, Tracker};
use crate::settings::{self, ChatSessions, Settings};
use crate::{hotzone, pet_window, tray};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;

pub struct Paths {
    pub config_dir: PathBuf,
    pub settings_file: PathBuf,
    pub chat_sessions_file: PathBuf,
    pub backups_dir: PathBuf,
    pub chat_settings_file: PathBuf,
}

pub struct AppState {
    pub paths: Paths,
    pub settings: Mutex<Settings>,
    pub chat_sessions: Mutex<ChatSessions>,
    pub tracker: Mutex<Tracker>,
    pub perms: Mutex<Queue>,
    pub ui: Mutex<Ui>,
    pub chat: Mutex<ChatState>,
    timer: watch::Sender<Option<Instant>>,
    timer_rx: Mutex<Option<watch::Receiver<Option<Instant>>>>,
}

/// A rectangle in the pet window, in CSS pixels. Clicks inside these hit the
/// pet; everywhere else they fall through to the app underneath.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Toast {
    pub kind: &'static str,
    pub text: String,
    pub project: String,
}

/// Horizontal drag of the pet along the top edge (physical pixels).
pub struct Drag {
    pub offset: i32,
    pub min_x: i32,
    pub max_x: i32,
}

#[derive(Default)]
pub struct Ui {
    pub visible: bool,
    pub game_active: bool,
    pub regions: Vec<Rect>,
    pub click_through: bool,
    pub chat_open: bool,
    pub toast: Option<(Toast, Instant)>,
    pub preview: Option<(Mood, Instant)>,
    /// Bumped whenever the pet is shown or hidden; stale cursor loops notice and stop.
    pub loop_gen: u64,
    pub drag: Option<Drag>,
    pub prev_foreground: isize,
    pub last_inside: Option<Instant>,
    pub hooks_installed: bool,
    pub pipe_error: Option<String>,
    /// First run during a game: open Settings when the game closes, not on top of it.
    pub settings_after_game: bool,
    last_hot_color: Option<Option<(u32, u8)>>,
    last_tooltip: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PetView {
    pub mood: Mood,
    pub status: Option<StatusView>,
    pub permission: Option<PermView>,
    pub toast: Option<Toast>,
    pub chat: chat::ChatView,
    pub chat_open: bool,
    pub follow_mouse: bool,
    pub hooks_installed: bool,
    pub game_active: bool,
}

/// Lock that survives a panic in another thread (a "poisoned" mutex).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Self {
        let paths = Paths {
            settings_file: config_dir.join("settings.json"),
            chat_sessions_file: config_dir.join("chat-sessions.json"),
            backups_dir: config_dir.join("backups"),
            chat_settings_file: config_dir.join("chat-hooks.json"),
            config_dir,
        };
        let loaded: Settings = settings::load_json(&paths.settings_file);
        let chat_sessions: ChatSessions = settings::load_json(&paths.chat_sessions_file);
        let (timer, timer_rx) = watch::channel(None);
        Self {
            settings: Mutex::new(loaded.sanitized()),
            chat_sessions: Mutex::new(chat_sessions),
            tracker: Mutex::new(Tracker::new()),
            perms: Mutex::new(Queue::default()),
            ui: Mutex::new(Ui { click_through: true, ..Default::default() }),
            chat: Mutex::new(ChatState::default()),
            timer,
            timer_rx: Mutex::new(Some(timer_rx)),
            paths,
        }
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn save_settings(&self, new: Settings) -> Settings {
        let clean = new.sanitized();
        *lock(&self.settings) = clean.clone();
        if let Err(e) = settings::save_json(&self.paths.settings_file, &clean) {
            eprintln!("Glowby: couldn't save settings: {e}");
        }
        clean
    }
}

/// Shows a short message bubble (and pops Glowby out if allowed).
pub fn toast(app: &AppHandle, kind: &'static str, text: String, project: String, secs: u64) {
    let state = app.state::<AppState>();
    lock(&state.ui).toast = Some((Toast { kind, text, project }, Instant::now() + Duration::from_secs(secs)));
}

/// Computes what the pet should show right now.
pub fn current_view(app: &AppHandle) -> PetView {
    view_and_deadline(app).0
}

fn view_and_deadline(app: &AppHandle) -> (PetView, Option<Instant>) {
    let state = app.state::<AppState>();
    let now = Instant::now();
    let settings = state.settings();
    let (permission, has_perm) = {
        let q = lock(&state.perms);
        (q.front_view(), !q.is_empty())
    };
    let (tracked, status, tracker_next) = {
        let t = lock(&state.tracker);
        (t.mood(now), t.status(now), t.next_change(now))
    };
    let chat_view = chat::view(&state, &settings);

    let mut ui = lock(&state.ui);
    if ui.toast.as_ref().is_some_and(|(_, until)| *until <= now) {
        ui.toast = None;
    }
    if ui.preview.is_some_and(|(_, until)| until <= now) {
        ui.preview = None;
    }
    let mut deadlines: Vec<Instant> = tracker_next.into_iter().collect();
    deadlines.extend(ui.toast.as_ref().map(|(_, until)| *until));
    deadlines.extend(ui.preview.map(|(_, until)| until));

    let mood = match ui.preview {
        Some((preview, _)) => preview,
        None if has_perm => Mood::Alert,
        None if chat_view.busy && tracked != Mood::Alert => Mood::Working,
        None => tracked,
    };
    let view = PetView {
        mood,
        status,
        permission,
        toast: ui.toast.as_ref().map(|(t, _)| t.clone()),
        chat: chat_view,
        chat_open: ui.chat_open,
        follow_mouse: settings.pet.follow_mouse,
        hooks_installed: ui.hooks_installed,
        game_active: ui.game_active,
    };
    (view, deadlines.into_iter().filter(|d| *d > now).min())
}

/// Pushes the latest state to everything that displays it:
/// the pet window (only while visible), the top-edge status line, the tray tooltip.
pub fn publish(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (view, next_deadline) = view_and_deadline(app);
    let status_line_on = lock(&state.settings).pet.status_line;

    let (visible, hot_update, tooltip_update) = {
        let mut ui = lock(&state.ui);
        let hot = if ui.visible || ui.game_active || !status_line_on { None } else { status_color(view.mood) };
        let hot_update = (ui.last_hot_color != Some(hot)).then_some(hot);
        ui.last_hot_color = Some(hot);
        let tooltip = tooltip_text(&view);
        let tooltip_update = (ui.last_tooltip != tooltip).then(|| tooltip.clone());
        ui.last_tooltip = tooltip;
        (ui.visible, hot_update, tooltip_update)
    };

    if visible {
        let _ = app.emit_to(pet_window::LABEL, "pet://view", &view);
    }
    if let Some(color) = hot_update {
        hotzone::set_status(color);
    }
    if let Some(text) = tooltip_update {
        tray::set_tooltip(app, &text);
    }
    state.timer.send_if_modified(|current| {
        let changed = *current != next_deadline;
        *current = next_deadline;
        changed
    });
}

/// Forces the next publish to re-send the status-line colour.
pub fn invalidate_status_line(app: &AppHandle) {
    lock(&app.state::<AppState>().ui).last_hot_color = None;
}

/// One background task sleeps until the next mood deadline, then republishes.
/// No polling: if nothing is scheduled, it sleeps until something changes.
pub fn spawn_mood_timer(app: AppHandle) {
    let Some(mut rx) = lock(&app.state::<AppState>().timer_rx).take() else { return };
    tauri::async_runtime::spawn(async move {
        loop {
            let deadline = *rx.borrow_and_update();
            match deadline {
                Some(at) => {
                    tokio::select! {
                        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(at)) => {
                            lock(&app.state::<AppState>().tracker).forget_stale(Instant::now());
                            publish(&app);
                        }
                        changed = rx.changed() => if changed.is_err() { break },
                    }
                }
                None => {
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// Colour + opacity of the thin top-edge line while Glowby is hidden
/// (COLORREF = 0x00BBGGRR). Always faintly visible so you know where to hover;
/// bright while Claude works or needs you.
fn status_color(mood: Mood) -> Option<(u32, u8)> {
    let rgb = |r: u32, g: u32, b: u32| (b << 16) | (g << 8) | r;
    Some(match mood {
        Mood::Working => (rgb(127, 155, 255), 240),
        Mood::Alert => (rgb(239, 159, 39), 255),
        Mood::Sick => (rgb(151, 196, 89), 240),
        _ => (rgb(150, 170, 255), 110),
    })
}

fn tooltip_text(view: &PetView) -> String {
    if view.permission.is_some() {
        return "Glowby: Claude is waiting for your permission".into();
    }
    match &view.status {
        Some(s) => format!("Glowby: {} · {}", s.project, s.activity),
        None => "Glowby: all quiet".into(),
    }
}
