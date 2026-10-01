// Release builds are GUI-only: no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applog;
mod chat;
mod commands;
mod gamemode;
mod hooks_installer;
mod hotzone;
mod permissions;
mod pet_window;
mod pipe_server;
mod sessions;
mod settings;
mod single_instance;
mod state;
mod tray;

use state::{AppState, lock};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

fn main() {
    if !single_instance::acquire() {
        single_instance::wake_running_instance();
        return;
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let config_dir = app.path().app_config_dir()?;
            applog::init(&config_dir);
            applog::line(format!("Glowby {} starting", app.package_info().version));
            app.manage(AppState::new(config_dir));

            // Keep the installed hook program up to date if hooks are installed.
            let status = hooks_installer::status(&handle);
            if status.state != "notInstalled" {
                let _ = hooks_installer::ensure_hook_binary(&handle);
            }
            lock(&app.state::<AppState>().ui).hooks_installed = status.state == "installed";

            // Order matters: windows first, then things that reference them.
            hotzone::create(&handle);
            pet_window::create(&handle)?;
            tray::create(&handle)?;
            gamemode::install(&handle);
            state::spawn_mood_timer(handle.clone());
            tauri::async_runtime::spawn(pipe_server::run(handle.clone()));

            let game_active = lock(&app.state::<AppState>().ui).game_active;
            hotzone::set_visible(!game_active);
            state::publish(&handle);
            applog::line(format!("started: hooks={}, fullscreen app running={game_active}", status.state));

            // First run without hooks: open Settings so you can connect Glowby,
            // but never on top of a fullscreen game (then: when the game closes).
            if status.state == "notInstalled" {
                if game_active {
                    lock(&app.state::<AppState>().ui).settings_after_game = true;
                } else {
                    commands::open_settings_window(&handle);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == pet_window::LABEL && matches!(event, WindowEvent::Focused(false)) {
                let _ = window.emit_to(pet_window::LABEL, "pet://blur", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::pet_ready,
            commands::set_hit_regions,
            commands::answer_permission,
            commands::chat_open,
            commands::chat_close,
            commands::chat_send,
            commands::chat_cancel,
            commands::chat_new,
            commands::drag_start,
            commands::dismiss_toast,
            commands::pet_hide,
            commands::open_settings,
            commands::get_settings,
            commands::save_settings,
            commands::list_monitors,
            commands::hooks_status,
            commands::hooks_preview,
            commands::hooks_apply,
            commands::preview_mood,
            commands::show_pet,
            commands::app_info,
            commands::open_folder,
        ])
        .build(tauri::generate_context!())
        .expect("Glowby failed to start");

    app.run(|handle, event| {
        if let RunEvent::ExitRequested { .. } = event {
            // Hooks still waiting get "no opinion" so Claude Code asks in the terminal.
            if let Some(state) = handle.try_state::<AppState>() {
                lock(&state.perms).pass_all();
            }
        }
    });
}
