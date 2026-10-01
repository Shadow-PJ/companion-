//! The pet window: transparent, always on top, never steals focus, and lets
//! clicks fall through everywhere except where the pet / bubble is drawn.
//!
//! After creation, Glowby manages this window with plain Win32 calls (instead
//! of Tauri's helpers) so our special window styles are never overwritten.

use crate::state::{self, AppState, Drag, lock};
use crate::{gamemode, hotzone};
use serde::Serialize;
use std::ptr::null_mut;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, SetFocus, VK_LBUTTON, VK_RBUTTON};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GWL_EXSTYLE, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowLongPtrW,
    GetWindowRect, GetWindowThreadProcessId, HWND_TOPMOST,
    IsWindow, SM_SWAPBUTTON, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

pub const LABEL: &str = "pet";
/// Window size in CSS pixels. Most of it is transparent and click-through.
pub const WIDTH: f64 = 380.0;
pub const HEIGHT: f64 = 480.0;
/// The invisible hover strip at the top edge.
const HOT_WIDTH: f64 = 150.0;
const HOT_HEIGHT: f64 = 3.0;
/// Hide this long after the mouse leaves (unless something needs you).
const HIDE_AFTER: Duration = Duration::from_millis(1400);
/// While visible, the cursor is sampled ~30 times a second (eyes + click-through).
const CURSOR_TICK: Duration = Duration::from_millis(33);

/// WebView2 (Chromium) switches: skip features Glowby never uses, make sure the
/// embedded browser doesn't phone home, run the GPU and network services inside
/// the main browser process (2 fewer processes, about 11 MB less RAM), and let
/// Glowby play his event sounds without a click first (autoplay policy).
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,Translate,msEdgeTranslate,AutofillServerCommunication,MediaRouter --enable-features=NetworkServiceInProcess2 --in-process-gpu --autoplay-policy=no-user-gesture-required --disable-background-networking --disable-component-update --disable-sync --no-pings";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub name: String,
    pub label: String,
    pub primary: bool,
    #[serde(skip)]
    pub x: i32,
    #[serde(skip)]
    pub y: i32,
    #[serde(skip)]
    pub w: i32,
    #[serde(skip)]
    pub scale: f64,
}

#[derive(Serialize, Clone)]
struct CursorMsg {
    x: f64,
    y: f64,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("pet.html".into()))
        .title("Glowby")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .focused(false)
        .visible(false)
        .additional_browser_args(BROWSER_ARGS)
        .build()?;
    if let Ok(h) = window.hwnd() {
        // NOACTIVATE  = clicking never steals focus from your editor or game.
        // TOOLWINDOW  = no taskbar button, not in Alt-Tab.
        // LAYERED + TRANSPARENT = mouse clicks pass through (toggled per region later).
        // (Same style bits Tauri itself uses for click-through windows. We set them
        // directly and never call Tauri's window setters afterwards, because those
        // rewrite the whole style and would drop NOACTIVATE / TOOLWINDOW.)
        set_ex_style(
            h.0 as HWND,
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            WS_EX_APPWINDOW,
        );
    }
    place(app);
    Ok(())
}

pub fn hwnd(app: &AppHandle) -> Option<HWND> {
    app.get_webview_window(LABEL)?.hwnd().ok().map(|h| h.0 as HWND)
}

fn set_ex_style(h: HWND, add: u32, remove: u32) {
    unsafe {
        let old = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let new = (old | add) & !remove;
        if new != old {
            SetWindowLongPtrW(h, GWL_EXSTYLE, new as isize);
        }
    }
}

fn set_click_through(h: HWND, on: bool) {
    if on { set_ex_style(h, WS_EX_TRANSPARENT, 0) } else { set_ex_style(h, 0, WS_EX_TRANSPARENT) }
}

pub fn monitors(app: &AppHandle) -> Vec<MonitorInfo> {
    let primary = app.primary_monitor().ok().flatten().and_then(|m| m.name().cloned());
    app.available_monitors()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let name = m.name().cloned().unwrap_or_else(|| format!("monitor-{i}"));
            let is_primary = primary.as_deref() == Some(name.as_str());
            let size = m.size();
            MonitorInfo {
                label: format!(
                    "Display {} ({}×{}){}",
                    i + 1,
                    size.width,
                    size.height,
                    if is_primary { ", primary" } else { "" }
                ),
                primary: is_primary,
                x: m.position().x,
                y: m.position().y,
                w: size.width as i32,
                scale: m.scale_factor(),
                name,
            }
        })
        .collect()
}

fn target_monitor(app: &AppHandle, wanted: &str) -> Option<MonitorInfo> {
    let all = monitors(app);
    all.iter()
        .find(|m| !wanted.is_empty() && m.name == wanted)
        .or_else(|| all.iter().find(|m| m.primary))
        .or_else(|| all.first())
        .cloned()
}

/// Puts the window (and the hover strip) at the chosen spot on the top edge.
pub fn place(app: &AppHandle) {
    let settings = app.state::<AppState>().settings();
    let (Some(mon), Some(h)) = (target_monitor(app, &settings.pet.monitor), hwnd(app)) else { return };
    let w = (WIDTH * mon.scale).round() as i32;
    let height = (HEIGHT * mon.scale).round() as i32;
    let x = mon.x + ((mon.w - w).max(0) as f64 * settings.pet.position).round() as i32;
    unsafe { SetWindowPos(h, HWND_TOPMOST, x, mon.y, w, height, SWP_NOACTIVATE) };
    let zone_w = (HOT_WIDTH * mon.scale).round() as i32;
    let zone_h = ((HOT_HEIGHT * mon.scale).round() as i32).max(2);
    hotzone::set_geometry(x + w / 2 - zone_w / 2, mon.y, zone_w, zone_h);
}

/// Slides Glowby out. Safe to call from any thread.
pub fn show(app: &AppHandle) {
    gamemode::refresh(app);
    let state = app.state::<AppState>();
    let generation = {
        let mut ui = lock(&state.ui);
        if ui.game_active {
            return;
        }
        ui.last_inside = Some(Instant::now());
        if ui.visible {
            return;
        }
        ui.visible = true;
        ui.click_through = true;
        ui.loop_gen += 1;
        ui.loop_gen
    };
    place(app);
    if let Some(h) = hwnd(app) {
        set_click_through(h, true);
        unsafe {
            ShowWindow(h, SW_SHOWNOACTIVATE);
            SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
    hotzone::set_visible(false);
    state::publish(app);
    let _ = app.emit_to(LABEL, "pet://visibility", true);
    start_cursor_loop(app.clone(), generation);
}

/// Shows Glowby and keeps it out for `secs` even if the mouse isn't on it
/// (tray "Show Glowby", Settings "Show me").
pub fn peek(app: &AppHandle, secs: u64) {
    show(app);
    lock(&app.state::<AppState>().ui).last_inside = Some(Instant::now() + Duration::from_secs(secs));
}

/// Slides Glowby back up; the window is hidden once the animation is done.
pub fn hide(app: &AppHandle) {
    let state = app.state::<AppState>();
    let had_chat_focus = {
        let mut ui = lock(&state.ui);
        if !ui.visible {
            return;
        }
        ui.visible = false;
        ui.loop_gen += 1;
        ui.drag = None;
        std::mem::take(&mut ui.chat_open)
    };
    if had_chat_focus {
        release_focus(app);
    }
    let _ = app.emit_to(LABEL, "pet://visibility", false);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(260)).await;
        finish_hide(&app);
    });
}

/// Immediate hide without animation (game mode).
pub fn hide_now(app: &AppHandle) {
    hide(app);
    finish_hide(app);
}

fn finish_hide(app: &AppHandle) {
    let state = app.state::<AppState>();
    let game_active = {
        let mut ui = lock(&state.ui);
        if ui.visible {
            return; // shown again during the animation
        }
        ui.click_through = true;
        ui.game_active
    };
    if let Some(h) = hwnd(app) {
        unsafe { ShowWindow(h, SW_HIDE) };
        set_click_through(h, true);
    }
    hotzone::set_visible(!game_active);
    state::invalidate_status_line(app);
    state::publish(app);
}

/// Runs only while the pet is visible: makes the pet clickable under the mouse,
/// feeds the eyes, handles dragging and auto-hide. Stops the moment it hides.
fn start_cursor_loop(app: AppHandle, generation: u64) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(CURSOR_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_cursor = POINT { x: i32::MIN, y: i32::MIN };
        loop {
            tick.tick().await;
            let Some(h) = hwnd(&app) else { break };
            let (cursor, rect) = unsafe {
                let mut p = POINT { x: 0, y: 0 };
                GetCursorPos(&mut p);
                let mut r: RECT = std::mem::zeroed();
                GetWindowRect(h, &mut r);
                (p, r)
            };
            let scale = ((rect.right - rect.left) as f64 / WIDTH).max(0.5);
            let lx = (cursor.x - rect.left) as f64 / scale;
            let ly = (cursor.y - rect.top) as f64 / scale;
            let now = Instant::now();

            let state = app.state::<AppState>();
            let follow_mouse = lock(&state.settings).pet.follow_mouse;
            let has_permission = !lock(&state.perms).is_empty();

            let mut set_through = None;
            let mut move_to = None;
            let mut drag_ended = false;
            let should_hide;
            {
                let mut ui = lock(&state.ui);
                if ui.loop_gen != generation || !ui.visible {
                    break;
                }
                let over = ui.regions.iter().any(|r| r.contains(lx, ly));
                let dragging = ui.drag.is_some();
                if over || dragging {
                    ui.last_inside = Some(now);
                }
                let want_through = !(over || dragging);
                if want_through != ui.click_through {
                    ui.click_through = want_through;
                    set_through = Some(want_through);
                }
                if let Some(drag) = &ui.drag {
                    if primary_button_down() {
                        move_to = Some((cursor.x - drag.offset).clamp(drag.min_x, drag.max_x));
                    } else {
                        drag_ended = true;
                    }
                }
                if drag_ended {
                    ui.drag = None;
                }
                let toast_active = ui.toast.as_ref().is_some_and(|(_, until)| *until > now);
                let held = ui.hold_until.is_some_and(|until| until > now);
                // A held mouse button usually means you're dragging a file over to Glowby.
                let sticky = ui.chat_open
                    || has_permission
                    || toast_active
                    || held
                    || ui.preview.is_some()
                    || ui.drop_hover
                    || dragging
                    || primary_button_down();
                let away_for = ui.last_inside.map(|t| now.duration_since(t)).unwrap_or_default();
                should_hide = !sticky && away_for > HIDE_AFTER;
            }

            if let Some(through) = set_through {
                set_click_through(h, through);
            }
            if let Some(x) = move_to {
                unsafe { SetWindowPos(h, null_mut(), x, rect.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
            }
            if drag_ended {
                save_dragged_position(&app, rect.left, rect.right - rect.left);
            }
            if follow_mouse && (cursor.x != last_cursor.x || cursor.y != last_cursor.y) {
                last_cursor = cursor;
                let _ = app.emit_to(LABEL, "pet://cursor", CursorMsg { x: lx, y: ly });
            }
            if should_hide {
                hide(&app);
                break;
            }
        }
    });
}

fn primary_button_down() -> bool {
    unsafe {
        let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { VK_RBUTTON } else { VK_LBUTTON };
        (GetAsyncKeyState(key as i32) as u16 & 0x8000) != 0
    }
}

/// Called by the page when you start dragging the pet sideways.
pub fn start_drag(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let (Some(mon), Some(h)) = (target_monitor(app, &settings.pet.monitor), hwnd(app)) else { return };
    let (cursor, rect) = unsafe {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(h, &mut r);
        (p, r)
    };
    let w = rect.right - rect.left;
    lock(&state.ui).drag = Some(Drag { offset: cursor.x - rect.left, min_x: mon.x, max_x: mon.x + (mon.w - w).max(0) });
}

fn save_dragged_position(app: &AppHandle, left: i32, width: i32) {
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    if let Some(mon) = target_monitor(app, &settings.pet.monitor) {
        let span = (mon.w - width).max(1) as f64;
        settings.pet.position = ((left - mon.x) as f64 / span).clamp(0.0, 1.0);
        state.save_settings(settings);
        place(app);
    }
}

/// Typing in the chat box needs keyboard focus, so ONLY then Glowby becomes a
/// normal focusable window. We remember who had focus to give it back later.
///
/// Why this is tricky: Windows only lets a program take the foreground if it
/// received the user's last input. Your click landed in WebView2's helper
/// process, not in glowby.exe, so a plain SetForegroundWindow is refused.
/// (And Tauri's own set_focus() does nothing here: it thinks the window is
/// hidden, because Glowby shows it with a direct Win32 call.)
pub fn focus_for_typing(app: &AppHandle) {
    let Some(h) = hwnd(app) else { return };
    let previous = unsafe { GetForegroundWindow() };
    if previous != h {
        lock(&app.state::<AppState>().ui).prev_foreground = previous as isize;
    }
    // Without NOACTIVATE, any click inside the chat also activates the window
    // the normal way (Windows always allows activation by a real click).
    set_ex_style(h, 0, WS_EX_NOACTIVATE | WS_EX_TRANSPARENT);
    let h_raw = h as isize;
    let app2 = app.clone();
    // Window activation must run on the thread that owns the window (the main thread).
    let _ = app.run_on_main_thread(move || {
        let h = h_raw as HWND;
        let how = force_foreground(h);
        let ex = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
        crate::applog::debug(format!(
            "chat focus: {how}, foreground is glowby={}, exstyle=0x{ex:X}",
            unsafe { GetForegroundWindow() } == h
        ));
        // Put keyboard focus inside the web page (WebView2 "MoveFocus").
        if let Some(window) = app2.get_webview_window(LABEL) {
            let webview: &tauri::Webview = window.as_ref();
            let _ = webview.set_focus();
        }
    });
}

/// True if the active window belongs to Glowby (the pet, Settings, or a folder picker).
pub fn glowby_is_foreground() -> bool {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
        pid == std::process::id()
    }
}

/// Brings Glowby to the foreground for typing.
///
/// Windows only lets a program take the foreground if it received the last
/// input. If plain SetForegroundWindow is refused, we briefly *attach* our UI
/// thread's input queue to the current foreground thread (AttachThreadInput):
/// for that moment the two threads share input state, so the switch is allowed.
/// (We do NOT fake an Alt key press: if that lands in Glowby, Windows enters
/// "menu mode" and swallows everything you type.)
fn force_foreground(h: HWND) -> &'static str {
    unsafe {
        if GetForegroundWindow() == h {
            return "already active";
        }
        if SetForegroundWindow(h) != 0 && GetForegroundWindow() == h {
            return "activated directly";
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, null_mut());
        let my_thread = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != my_thread && AttachThreadInput(my_thread, fg_thread, 1) != 0;
        BringWindowToTop(h);
        SetForegroundWindow(h);
        SetFocus(h);
        if attached {
            AttachThreadInput(my_thread, fg_thread, 0);
        }
        if GetForegroundWindow() == h { "activated via AttachThreadInput" } else { "activation refused (a click in the box will do it)" }
    }
}

pub fn release_focus(app: &AppHandle) {
    let Some(h) = hwnd(app) else { return };
    set_ex_style(h, WS_EX_NOACTIVATE, 0);
    let previous = std::mem::take(&mut lock(&app.state::<AppState>().ui).prev_foreground) as HWND;
    unsafe {
        if !previous.is_null() && IsWindow(previous) != 0 && GetForegroundWindow() == h {
            SetForegroundWindow(previous);
        }
    }
}
