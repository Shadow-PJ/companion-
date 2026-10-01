//! Functions the web pages may call with `invoke("name", args)`.
//! Each one is a small, explicit door from the UI into the Rust side.

use crate::hooks_installer::{self, HooksStatus, Preview};
use crate::pet_window::{self, MonitorInfo};
use crate::sessions::Mood;
use crate::settings::Settings;
use crate::state::{self, AppState, PetView, Rect, lock};
use crate::{chat, gamemode, pipe_server, tray};
use glowby_protocol::HookReply;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

// ---------- pet window ----------

#[tauri::command]
pub fn pet_ready(app: AppHandle) -> PetView {
    state::current_view(&app)
}

#[tauri::command]
pub fn set_hit_regions(app: AppHandle, regions: Vec<Rect>) {
    lock(&app.state::<AppState>().ui).regions = regions;
}

#[tauri::command]
pub fn answer_permission(app: AppHandle, id: u64, choice: String) {
    let reply = match choice.as_str() {
        "allow" => HookReply::Allow,
        "deny" => HookReply::Deny { message: "You denied this in Glowby.".into() },
        _ => HookReply::Pass, // "terminal": let Claude Code ask in the terminal
    };
    pipe_server::resolve(&app, id, reply);
}

#[tauri::command]
pub fn chat_open(app: AppHandle) {
    crate::applog::debug("chat: open");
    lock(&app.state::<AppState>().ui).chat_open = true;
    pet_window::focus_for_typing(&app);
    state::publish(&app);
}

#[tauri::command]
pub fn chat_close(app: AppHandle) {
    crate::applog::debug("chat: close");
    lock(&app.state::<AppState>().ui).chat_open = false;
    pet_window::release_focus(&app);
    state::publish(&app);
}

/// Diagnostics from the pet page (focus / key events, never what you type).
/// Only written when Glowby runs with GLOWBY_DEBUG=1.
#[tauri::command]
pub fn js_log(message: String) {
    crate::applog::debug(format!("page: {}", crate::sessions::shorten(&message, 300)));
}

#[tauri::command]
pub async fn chat_send(app: AppHandle, text: String) -> Result<(), String> {
    chat::send(app, text).await
}

#[tauri::command]
pub fn chat_cancel(app: AppHandle) {
    chat::cancel(&app);
}

#[tauri::command]
pub fn chat_new(app: AppHandle) {
    chat::new_conversation(&app);
}

/// Folder picked from the chat bubble's folder chip.
#[tauri::command]
pub fn set_chat_folder(app: AppHandle, path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err("That folder doesn't exist.".into());
    }
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    settings.chat.project_dir = path;
    state.save_settings(settings);
    state::publish(&app);
    Ok(())
}

#[tauri::command]
pub fn drag_start(app: AppHandle) {
    pet_window::start_drag(&app);
}

#[tauri::command]
pub fn dismiss_toast(app: AppHandle) {
    lock(&app.state::<AppState>().ui).toast = None;
    state::publish(&app);
}

#[tauri::command]
pub fn pet_hide(app: AppHandle) {
    pet_window::hide(&app);
}

#[tauri::command]
pub fn open_settings(app: AppHandle) {
    open_settings_window(&app);
}

// ---------- settings window ----------

/// Created on demand and destroyed when closed, so it uses no memory otherwise.
pub fn open_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    // Building a window from a background task avoids a known WebView2 deadlock.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let built = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("settings.html".into()))
            .title("Glowby Settings")
            .inner_size(860.0, 760.0)
            .min_inner_size(560.0, 480.0)
            .center()
            .additional_browser_args(pet_window::BROWSER_ARGS)
            .build();
        match built {
            Ok(_) => crate::applog::line("settings window opened"),
            Err(e) => crate::applog::line(format!("settings window failed: {e}")),
        }
    });
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    app.state::<AppState>().settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Settings {
    let state = app.state::<AppState>();
    let before = state.settings();
    let saved = state.save_settings(settings);
    if before.pet.monitor != saved.pet.monitor || before.pet.position != saved.pet.position {
        pet_window::place(&app);
    }
    if before.game_mode != saved.game_mode {
        tray::sync_game_mode(&app, saved.game_mode);
        gamemode::refresh(&app);
    }
    state::invalidate_status_line(&app);
    state::publish(&app);
    saved
}

#[tauri::command]
pub fn list_monitors(app: AppHandle) -> Vec<MonitorInfo> {
    pet_window::monitors(&app)
}

#[tauri::command]
pub fn hooks_status(app: AppHandle) -> HooksStatus {
    let status = hooks_installer::status(&app);
    lock(&app.state::<AppState>().ui).hooks_installed = status.state == "installed";
    status
}

#[tauri::command]
pub fn hooks_preview(app: AppHandle, install: bool) -> Result<Preview, String> {
    hooks_installer::preview(&app, install)
}

#[tauri::command]
pub fn hooks_apply(app: AppHandle, install: bool, token: String) -> Result<String, String> {
    let result = hooks_installer::apply(&app, install, &token);
    state::publish(&app);
    result
}

#[tauri::command]
pub fn preview_mood(app: AppHandle, mood: String) {
    let Some(mood) = Mood::parse(&mood) else { return };
    lock(&app.state::<AppState>().ui).preview = Some((mood, Instant::now() + Duration::from_secs(5)));
    pet_window::show(&app);
    state::publish(&app);
}

#[tauri::command]
pub fn show_pet(app: AppHandle) {
    pet_window::peek(&app, 4);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    data_dir: String,
    claude_path: Option<String>,
    fullscreen_now: bool,
    pipe_error: Option<String>,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    let state = app.state::<AppState>();
    let settings = state.settings();
    AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: state.paths.config_dir.display().to_string(),
        claude_path: chat::find_claude(&settings.chat.claude_path).map(|p| p.display().to_string()),
        fullscreen_now: gamemode::fullscreen_app_running(),
        pipe_error: lock(&state.ui).pipe_error.clone(),
    }
}

/// Opens one of Glowby's own folders in Explorer (only these, nothing arbitrary).
#[tauri::command]
pub fn open_folder(app: AppHandle, which: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let path = match which.as_str() {
        "data" => state.paths.config_dir.clone(),
        "backups" => state.paths.backups_dir.clone(),
        "claude" => hooks_installer::claude_settings_path(&app).parent().map(|p| p.to_path_buf()).unwrap_or_default(),
        _ => return Err("unknown folder".into()),
    };
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer.exe").arg(&path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}
