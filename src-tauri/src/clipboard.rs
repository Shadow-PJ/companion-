//! Puts text on the clipboard (the fresh-start note for a new chat).
//!
//! Windows only accepts clipboard data from a window that owns the clipboard,
//! so the copy runs on the UI thread with Glowby's hover-strip window.

use std::time::Duration;
use tauri::AppHandle;
use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

const CF_UNICODETEXT: u32 = 13;

/// true = the text is on the clipboard. Call from a background thread.
pub fn write_text(app: &AppHandle, text: String) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let queued = app.run_on_main_thread(move || {
        let _ = tx.send(unsafe { write_with_owner(crate::hotzone::hwnd(), &text) });
    });
    let ok = queued.is_ok() && rx.recv_timeout(Duration::from_secs(3)).unwrap_or(false);
    crate::applog::debug(format!("clipboard: note copied = {ok}"));
    ok
}

unsafe fn write_with_owner(owner: HWND, text: &str) -> bool {
    if owner.is_null() {
        return false;
    }
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        // another app may hold the clipboard for a moment
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(owner) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !opened {
            return false;
        }
        EmptyClipboard();
        let memory = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        if memory.is_null() {
            CloseClipboard();
            return false;
        }
        let target = GlobalLock(memory) as *mut u16;
        if target.is_null() {
            GlobalFree(memory);
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
        GlobalUnlock(memory);
        // on success Windows owns the memory; otherwise we free it
        let ok = !SetClipboardData(CF_UNICODETEXT, memory).is_null();
        if !ok {
            GlobalFree(memory);
        }
        CloseClipboard();
        ok
    }
}
