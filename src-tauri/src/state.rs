//! Shared app state + `publish()`, which turns the state into what the pet shows.
//!
//! Locking rule (avoids deadlocks): take a lock, copy what you need, drop it,
//! and only THEN call Win32 window functions or emit events.

use crate::actions::{LastError, Offer};
use crate::chat::{self, ChatState};
use crate::health::Health;
use crate::permissions::{PermView, Queue};
use crate::progress::{Look, Progress, ProgressView};
use crate::sessions::{Mood, StatusView, Tracker};
use crate::wellbeing::Breaks;
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
    pub health_file: PathBuf,
    pub progress_file: PathBuf,
    pub quests_file: PathBuf,
    pub projects_file: PathBuf,
    pub characters_file: PathBuf,
    pub characters_dir: PathBuf,
    pub squad_file: PathBuf,
}

pub struct AppState {
    pub paths: Paths,
    pub settings: Mutex<Settings>,
    pub chat_sessions: Mutex<ChatSessions>,
    pub tracker: Mutex<Tracker>,
    pub perms: Mutex<Queue>,
    pub ui: Mutex<Ui>,
    pub chat: Mutex<ChatState>,
    pub health: Mutex<Health>,
    pub last_error: Mutex<Option<LastError>>,
    pub progress: Mutex<Progress>,
    pub breaks: Mutex<Breaks>,
    pub quests: Mutex<crate::quests::QuestBook>,
    pub projects: Mutex<crate::projects::Projects>,
    pub learn: Mutex<crate::learn::Learn>,
    pub github: Mutex<crate::github::Github>,
    pub characters: Mutex<crate::characters::Characters>,
    pub squad: Mutex<crate::squad::Squad>,
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
    /// "That looks like an error" offer from the error watcher, until it expires.
    pub offer: Option<(Offer, Instant)>,
    /// A file is being dragged over Glowby.
    pub drop_hover: bool,
    /// Today's briefing, until you dismiss it.
    pub briefing: Option<crate::briefing::BriefingView>,
    /// Keep Glowby out until then (a new offer, briefing or question), even if
    /// the mouse isn't on him. Afterwards he hides normally; hover to see it again.
    pub hold_until: Option<Instant>,
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
    pub offer: Option<Offer>,
    pub chat: chat::ChatView,
    pub chat_open: bool,
    pub drop_hover: bool,
    pub quick_actions: Vec<ActionView>,
    /// None when XP and levels are turned off.
    pub progress: Option<ProgressView>,
    pub look: Look,
    pub emotes: Vec<ActionView>,
    pub quests: Vec<crate::quests::QuestView>,
    pub briefing: Option<crate::briefing::BriefingView>,
    pub quiz: Option<crate::learn::QuizView>,
    /// Squad mode: one small pet per live Claude Code session (empty when off).
    pub squad: Vec<crate::squad::SquadView>,
    /// Your imported characters (id + name), for the squad pets' "Look" choice.
    pub characters: Vec<ActionView>,
    /// Anime pets you've unlocked (id + name).
    pub pets: Vec<ActionView>,
    pub follow_mouse: bool,
    pub hooks_installed: bool,
    pub game_active: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ActionView {
    pub id: String,
    pub label: String,
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
            health_file: config_dir.join("health.json"),
            progress_file: config_dir.join("progress.json"),
            quests_file: config_dir.join("quests.json"),
            projects_file: config_dir.join("projects.json"),
            characters_file: config_dir.join("characters.json"),
            characters_dir: config_dir.join("characters"),
            squad_file: config_dir.join("squad.json"),
            config_dir,
        };
        let characters: crate::characters::Characters = settings::load_json(&paths.characters_file);
        let squad: crate::squad::Squad = settings::load_json(&paths.squad_file);
        let progress: Progress = settings::load_json(&paths.progress_file);
        let quests: crate::quests::QuestBook = settings::load_json(&paths.quests_file);
        let projects: crate::projects::Projects = settings::load_json(&paths.projects_file);
        let loaded: Settings = settings::load_json(&paths.settings_file);
        let chat_sessions: ChatSessions = settings::load_json(&paths.chat_sessions_file);
        let health: Health = settings::load_json(&paths.health_file);
        let (timer, timer_rx) = watch::channel(None);
        Self {
            settings: Mutex::new(loaded.sanitized()),
            chat_sessions: Mutex::new(chat_sessions),
            tracker: Mutex::new(Tracker::new()),
            perms: Mutex::new(Queue::default()),
            ui: Mutex::new(Ui { click_through: true, ..Default::default() }),
            chat: Mutex::new(ChatState::default()),
            health: Mutex::new(health),
            last_error: Mutex::new(None),
            progress: Mutex::new(progress),
            breaks: Mutex::new(Breaks::default()),
            quests: Mutex::new(quests),
            projects: Mutex::new(projects),
            learn: Mutex::new(crate::learn::Learn::default()),
            github: Mutex::new(crate::github::Github::default()),
            characters: Mutex::new(characters),
            squad: Mutex::new(squad),
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

/// Keeps Glowby out for `secs` (he hides normally afterwards).
pub fn hold_out(app: &AppHandle, secs: u64) {
    let until = Instant::now() + Duration::from_secs(secs);
    let state = app.state::<AppState>();
    let mut ui = lock(&state.ui);
    ui.hold_until = Some(ui.hold_until.map_or(until, |t| t.max(until)));
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
    let (tracked, status, tracker_next, live) = {
        let t = lock(&state.tracker);
        let live = if settings.squad.enabled { t.live_sessions(now) } else { Vec::new() };
        (t.mood(now), t.status(now), t.next_change(now), live)
    };
    let characters: Vec<ActionView> =
        lock(&state.characters).list.iter().map(|c| ActionView { id: c.id.clone(), label: c.name.clone() }).collect();
    let known_character = |id: &str| characters.iter().any(|c| c.id == id);
    let pets: Vec<ActionView> = crate::progress::unlocked_pets(&lock(&state.progress))
        .into_iter()
        .map(|(id, label)| ActionView { id: id.into(), label: label.into() })
        .collect();
    let pet_ok = |id: &str| pets.iter().any(|p| p.id == id);
    let squad = crate::squad::views(app, &live, settings.squad.max_shown as usize, known_character, pet_ok);
    let chat_view = chat::view(&state, &settings);
    let failing = if settings.health { lock(&state.health).latest().cloned() } else { None };
    let wall_now = chrono::Local::now();
    let (progress_view, mut look, emotes, energy_drop) = {
        let p = lock(&state.progress);
        let emotes = crate::progress::unlocked_emotes(&p).into_iter().map(|(id, label)| ActionView { id: id.into(), label: label.into() }).collect();
        (crate::progress::view(&p, wall_now), crate::progress::look(&p, &settings, wall_now), emotes, crate::progress::next_energy_drop(&p, wall_now))
    };
    if !known_character(&look.character) {
        look.character.clear();
    }
    let quests = crate::quests::views(app);
    let (quiz, quiz_deadline) = {
        let mut learn = lock(&state.learn);
        learn.expire(now);
        (learn.view(now), learn.deadline())
    };
    let break_due = if settings.breaks.enabled {
        lock(&state.breaks).due_at(now, Duration::from_secs(settings.breaks.interval_mins as u64 * 60))
    } else {
        None
    };

    let mut ui = lock(&state.ui);
    if ui.toast.as_ref().is_some_and(|(_, until)| *until <= now) {
        ui.toast = None;
    }
    if ui.preview.is_some_and(|(_, until)| until <= now) {
        ui.preview = None;
    }
    if ui.offer.as_ref().is_some_and(|(_, until)| *until <= now) {
        ui.offer = None;
    }
    let mut deadlines: Vec<Instant> = tracker_next.into_iter().collect();
    deadlines.extend(ui.toast.as_ref().map(|(_, until)| *until));
    deadlines.extend(ui.preview.map(|(_, until)| until));
    deadlines.extend(ui.offer.as_ref().map(|(_, until)| *until));
    deadlines.extend(break_due);
    deadlines.extend(quiz_deadline);
    if settings.progression.neglect {
        deadlines.extend(energy_drop);
    }

    // A copied error offer wins; otherwise failing tests/builds stay on offer until they pass.
    let clipboard_offer = ui.offer.as_ref().map(|(o, _)| o.clone());
    let offer = clipboard_offer.clone().or_else(|| {
        failing.as_ref().map(|f| Offer {
            kind: "failingChecks",
            title: if f.kind == crate::health::CheckKind::Tests { "Tests are failing".into() } else { "The build is failing".into() },
            detail: f.command.clone(),
            project: crate::sessions::project_name(&f.project_dir),
            url: None,
            dir: Some(f.project_dir.clone()),
        })
    });

    let break_offer = clipboard_offer.as_ref().is_some_and(|o| o.kind == "break");
    let ci_offer = clipboard_offer.as_ref().is_some_and(|o| o.kind == "ci");
    let error_offer = clipboard_offer.as_ref().is_some_and(|o| o.kind == "clipboardError");
    let mood = match ui.preview {
        Some((preview, _)) => preview,
        None if ui.drop_hover => Mood::Happy,
        None if has_perm || error_offer || tracked == Mood::Alert => Mood::Alert,
        None if failing.is_some() || ci_offer => Mood::Sick,
        None if break_offer => Mood::Sleepy,
        None if chat_view.busy => Mood::Working,
        // Ignored for days: low energy makes Glowby sleepy (but never worse).
        None if tracked == Mood::Idle && look.weak => Mood::Sleepy,
        None => tracked,
    };
    let quick_actions = if settings.quick_actions.enabled {
        settings.quick_actions.actions.iter().map(|a| ActionView { id: a.id.clone(), label: a.label.clone() }).collect()
    } else {
        Vec::new()
    };
    let view = PetView {
        mood,
        status,
        permission,
        toast: ui.toast.as_ref().map(|(t, _)| t.clone()),
        offer,
        chat: chat_view,
        chat_open: ui.chat_open,
        drop_hover: ui.drop_hover,
        quick_actions,
        progress: settings.progression.enabled.then_some(progress_view),
        look,
        emotes,
        quests,
        briefing: ui.briefing.clone(),
        quiz,
        squad,
        characters,
        pets,
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
                            crate::wellbeing::check(&app);
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
