//! Game mode: hide Glowby while a fullscreen app or game runs.
//!
//! Event-driven: Windows calls `on_foreground` whenever the active window
//! changes. Only then do we ask "is something fullscreen?". No timers.

use crate::state::{self, AppState, lock};
use crate::{hotzone, pet_window};
use std::ptr::null_mut;
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromWindow};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::Shell::{
    QUERY_USER_NOTIFICATION_STATE, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    SHQueryUserNotificationState,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GWL_STYLE, GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WS_MAXIMIZE,
};

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Must run on the main thread (the hook is delivered through its message loop).
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            null_mut(),
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
    }
    refresh(app);
}

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    if let Some(app) = APP.get() {
        refresh(app);
    }
}

/// Re-checks fullscreen state and hides / restores Glowby if it changed.
pub fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    let enabled = lock(&state.settings).game_mode;
    let active = enabled && fullscreen_app_running();
    let (changed, visible, open_settings) = {
        let mut ui = lock(&state.ui);
        let changed = ui.game_active != active;
        ui.game_active = active;
        let open_settings = !active && std::mem::take(&mut ui.settings_after_game);
        (changed, ui.visible, open_settings)
    };
    if !changed {
        return;
    }
    if active {
        // Questions waiting on Glowby go straight back to the terminal.
        lock(&state.perms).pass_all();
        pet_window::hide_now(app);
        hotzone::set_visible(false);
    } else {
        hotzone::set_visible(!visible);
    }
    if open_settings {
        crate::commands::open_settings_window(app);
    }
    state::invalidate_status_line(app);
    state::publish(app);
}

pub fn fullscreen_app_running() -> bool {
    let mut quns: QUERY_USER_NOTIFICATION_STATE = 0;
    let shell_says = unsafe { SHQueryUserNotificationState(&mut quns) } >= 0
        && matches!(quns, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE);
    shell_says || foreground_covers_monitor()
}

/// Backup check for borderless-window games: the active window covers its whole
/// monitor and isn't the desktop or a normal maximised window.
fn foreground_covers_monitor() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(fg, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return false;
        }
        // A maximised window is a normal app, even without a title bar (Chrome,
        // Claude, VS Code draw their own). With an auto-hiding taskbar it covers
        // the whole monitor, which used to look like a borderless game.
        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        if style & WS_MAXIMIZE != 0 {
            return false;
        }
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(fg, &mut r);
        let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
        if monitor.is_null() {
            return false;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return false;
        }
        let m = info.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}
