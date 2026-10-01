//! The hover strip: a tiny native window (no web view, ~0 memory) along the
//! top edge where Glowby lives. Windows itself tells us when the mouse touches
//! it (WM_MOUSEMOVE), so detecting "hover the top edge" needs NO polling.
//!
//! It doubles as the status line: invisible when idle, a thin blue line while
//! Claude works, amber when Claude needs you. A static colour costs no CPU.

use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use tauri::AppHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetCursorPos, GetWindowRect, HWND_TOPMOST, IDC_ARROW,
    KillTimer, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

/// Sent after TrackMouseEvent when the mouse leaves (defined in a module we don't otherwise need).
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Sent when the clipboard changes (only while the opt-in error watcher is on).
const WM_CLIPBOARDUPDATE: u32 = 0x031D;
const CLIPBOARD_TIMER: usize = 2;
/// Wait a moment before reading, so the app that copied has finished writing.
const CLIPBOARD_SETTLE_MS: u32 = 150;

static APP: OnceLock<AppHandle> = OnceLock::new();
static ZONE: AtomicIsize = AtomicIsize::new(0);
static COLOR: AtomicU32 = AtomicU32::new(0);
static TRACKING: AtomicBool = AtomicBool::new(false);
const DWELL_TIMER: usize = 1;
/// The mouse must rest on the strip this long, so flicking to a browser tab
/// at the top of the screen doesn't summon Glowby by accident.
const DWELL_MS: u32 = 160;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn zone() -> HWND {
    ZONE.load(Ordering::Relaxed) as HWND
}

/// The strip's window handle (also used for clipboard and drag-and-drop messages).
pub fn hwnd() -> HWND {
    zone()
}

/// Must run on the main (UI) thread, which pumps this window's messages.
pub fn create(app: &AppHandle) {
    let _ = APP.set(app.clone());
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide("GlowbyHotZone");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        // Alpha 1/255: invisible to you, but still receives the mouse.
        SetLayeredWindowAttributes(hwnd, 0, 1, LWA_ALPHA);
        ZONE.store(hwnd as isize, Ordering::Relaxed);
    }
}

pub fn set_geometry(x: i32, y: i32, w: i32, h: i32) {
    let hwnd = zone();
    if !hwnd.is_null() {
        unsafe { SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE) };
    }
}

pub fn set_visible(visible: bool) {
    let hwnd = zone();
    if !hwnd.is_null() {
        unsafe { ShowWindow(hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE }) };
    }
}

/// `Some((colour, opacity))` shows a thin status line, `None` makes the strip invisible.
pub fn set_status(color: Option<(u32, u8)>) {
    let hwnd = zone();
    if hwnd.is_null() {
        return;
    }
    unsafe {
        match color {
            Some((c, alpha)) => {
                COLOR.store(c, Ordering::Relaxed);
                SetLayeredWindowAttributes(hwnd, 0, alpha.max(1), LWA_ALPHA);
            }
            None => {
                SetLayeredWindowAttributes(hwnd, 0, 1, LWA_ALPHA);
            }
        }
        InvalidateRect(hwnd, null(), 1);
    }
}

fn cursor_inside(hwnd: HWND) -> bool {
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        let mut r: RECT = std::mem::zeroed();
        GetCursorPos(&mut p);
        GetWindowRect(hwnd, &mut r);
        p.x >= r.left - 2 && p.x <= r.right + 2 && p.y >= r.top - 2 && p.y <= r.bottom + 2
    }
}

fn summon() {
    if let Some(app) = APP.get() {
        crate::pet_window::show(app);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEMOVE => {
                if !TRACKING.swap(true, Ordering::Relaxed) {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tme);
                    SetTimer(hwnd, DWELL_TIMER, DWELL_MS, None);
                }
                0
            }
            WM_MOUSELEAVE => {
                TRACKING.store(false, Ordering::Relaxed);
                KillTimer(hwnd, DWELL_TIMER);
                0
            }
            WM_TIMER if wparam == CLIPBOARD_TIMER => {
                KillTimer(hwnd, CLIPBOARD_TIMER);
                if let Some(app) = APP.get() {
                    crate::error_watch::on_clipboard_settled(app, hwnd);
                }
                0
            }
            WM_TIMER => {
                KillTimer(hwnd, DWELL_TIMER);
                if cursor_inside(hwnd) {
                    summon();
                }
                0
            }
            WM_CLIPBOARDUPDATE => {
                SetTimer(hwnd, CLIPBOARD_TIMER, CLIPBOARD_SETTLE_MS, None);
                0
            }
            WM_LBUTTONDOWN => {
                summon();
                0
            }
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rc);
                let brush = CreateSolidBrush(COLOR.load(Ordering::Relaxed));
                FillRect(hdc, &rc, brush);
                DeleteObject(brush);
                EndPaint(hwnd, &ps);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
