//! System tray icon: show Glowby, open Settings, toggle game mode, quit.

use crate::state::{AppState, lock};
use crate::{commands, gamemode, pet_window};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "glowby";

pub struct TrayItems {
    pub game: CheckMenuItem<Wry>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let game_on = app.state::<AppState>().settings().game_mode;
    let show = MenuItem::with_id(app, "show", "Show Glowby", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let game = CheckMenuItem::with_id(app, "game", "Game mode", true, game_on, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Glowby", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &settings, &game, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Glowby")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => pet_window::peek(app, 4),
            "settings" => commands::open_settings_window(app),
            "game" => toggle_game_mode(app),
            "quit" => {
                // Any question still waiting goes back to the terminal right away.
                lock(&app.state::<AppState>().perms).pass_all();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                pet_window::peek(tray.app_handle(), 4);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(TrayItems { game });
    Ok(())
}

pub fn set_tooltip(app: &AppHandle, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

pub fn sync_game_mode(app: &AppHandle, on: bool) {
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.game.set_checked(on);
    }
}

fn toggle_game_mode(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    settings.game_mode = !settings.game_mode;
    let on = state.save_settings(settings).game_mode;
    sync_game_mode(app, on);
    gamemode::refresh(app);
}
