// Release builds are GUI-only: no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod agent_watch;
mod codex_chat;
mod alerts;
mod applog;
mod autoallow;
mod briefing;
mod characters;
mod chat;
mod clipboard;
mod codex_hooks_installer;
mod commands;
mod detective;
mod dropzone;
mod error_watch;
mod gamemode;
mod github;
mod guard;
mod health;
mod hooks_installer;
mod hotzone;
mod learn;
mod notify;
mod limits;
mod permissions;
mod pet_window;
mod pipe_server;
mod progress;
mod pulse;
mod projects;
mod quests;
mod sessions;
mod settings;
mod single_instance;
mod squad;
mod sounds;
mod state;
mod tray;
mod wellbeing;

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
            app.manage(crate::pulse::Pulse::new(config_dir.clone()));
            app.manage(AppState::new(config_dir));
            crate::pulse::spawn(handle.clone());

            // Keep the installed hook program up to date if hooks are installed.
            // (Install it BEFORE checking the status: a fresh copy of glowby.exe,
            // e.g. on your Desktop, puts its built-in hook program in place first.)
            if (hooks_installer::status(&handle).state != "notInstalled"
                || codex_hooks_installer::status(&handle).state != "notInstalled")
                && let Err(e) = hooks_installer::ensure_hook_binary(&handle)
            {
                applog::line(format!("hook program: {e}"));
            }
            let status = hooks_installer::status(&handle);
            lock(&app.state::<AppState>().ui).hooks_installed = matches!(status.state, "installed" | "outdated") || matches!(codex_hooks_installer::status(&handle).state, "installed" | "outdated");

            // Order matters: windows first, then things that reference them.
            hotzone::create(&handle);
            dropzone::register_edge(&handle);
            error_watch::apply(&handle, app.state::<AppState>().settings().error_watcher);
            pet_window::create(&handle)?;
            tray::create(&handle)?;
            gamemode::install(&handle);
            state::spawn_mood_timer(handle.clone());
            tauri::async_runtime::spawn(pipe_server::run(handle.clone()));
            github::spawn(handle.clone()); // sleeps unless the CI check is turned on
            crate::agent_watch::spawn(&handle);
            limits::refresh_now(&handle); // fresh Claude / Codex limit numbers at start
            limits::resume(&handle); // still out of Claude usage? "back" alert at the reset

            let game_active = lock(&app.state::<AppState>().ui).game_active;
            hotzone::set_visible(!game_active);
            state::publish(&handle);
            applog::line(format!("started: hooks={}, fullscreen app running={game_active}", status.state));

            // First run without hooks: open Settings so you can connect Glowby,
            // but never on top of a fullscreen game (then: when the game closes).
            if !lock(&app.state::<AppState>().ui).hooks_installed {
                if game_active {
                    lock(&app.state::<AppState>().ui).settings_after_game = true;
                } else {
                    commands::open_settings_window(&handle);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == pet_window::LABEL
                && let WindowEvent::DragDrop(drag) = event
            {
                dropzone::on_pet_drag(window.app_handle(), drag);
            }
            if window.label() == pet_window::LABEL
                && let WindowEvent::Focused(focused) = event
            {
                applog::debug(format!("pet window focused={focused}"));
                if !focused {
                    // Windows reports a tiny "lost focus" blip while it hands focus from the
                    // window frame to the web page inside it. Only treat it as "you clicked
                    // somewhere else" if another program is still in front a moment later.
                    let app = window.app_handle().clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                        if !pet_window::glowby_is_foreground() {
                            let _ = app.emit_to(pet_window::LABEL, "pet://blur", ());
                        }
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            crate::pulse::pulse_open,
            crate::pulse::pulse_open_page,
            crate::pulse::pulse_view,
            crate::pulse::pulse_refresh,
            crate::pulse::pulse_preferences,
            crate::pulse::pulse_theme,
            crate::pulse::pulse_open_source,
            commands::pet_ready,
            commands::set_hit_regions,
            commands::answer_permission,
            commands::chat_open,
            commands::chat_select_agent,
            commands::companion_status,
            commands::chat_close,
            commands::chat_send,
            commands::chat_cancel,
            commands::chat_new,
            commands::set_chat_folder,
            commands::js_log,
            commands::chat_remove_attachment,
            commands::run_action,
            commands::offer_action,
            commands::health_clear,
            commands::default_quick_actions,
            commands::get_progress,
            commands::play_emote,
            commands::briefing_show,
            commands::briefing_dismiss,
            commands::briefing_do,
            commands::quiz_answer,
            commands::quiz_skip,
            commands::quests_today,
            commands::github_save_token,
            commands::github_remove_token,
            commands::github_status,
            commands::github_check_now,
            commands::squad_chat_open,
            commands::squad_set_look,
            commands::pet_petted,
            commands::limits_refresh,
            commands::detective_run,
            commands::detective_close,
            commands::detective_fix,
            commands::detective_last,
            commands::auto_allow_start,
            commands::auto_allow_stop,
            commands::auto_allow_log,
            commands::auto_allow_clear_log,
            commands::auto_allow_defaults,
            commands::characters_list,
            commands::character_read_source,
            commands::character_add,
            commands::character_rename,
            commands::character_delete,
            commands::character_image,
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
            commands::codex_hooks_status,
            commands::codex_hooks_preview,
            commands::codex_hooks_apply,
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
