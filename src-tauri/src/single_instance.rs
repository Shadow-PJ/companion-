//! Only one Glowby at a time. A second launch just asks the first to show up.

use glowby_protocol::{HookEnvelope, PROTOCOL_VERSION, SHOW_EVENT};
use std::io::Write;
use std::ptr::null;
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateMutexW;

/// True if we are the first instance. The mutex lives until the process exits.
pub fn acquire() -> bool {
    let name: Vec<u16> = "Local\\GlowbySingleInstance".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let handle = CreateMutexW(null(), 0, name.as_ptr());
        if handle.is_null() {
            return true; // can't tell; run anyway
        }
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Sends "please show yourself" to the running Glowby over its pipe.
pub fn wake_running_instance() {
    let envelope = HookEnvelope {
        v: PROTOCOL_VERSION,
        event: SHOW_EVENT.into(),
        wants_reply: false,
        from_pet_chat: false,
        payload: serde_json::json!({}),
    };
    if let (Ok(mut pipe), Ok(mut line)) = (
        std::fs::OpenOptions::new().write(true).open(glowby_protocol::pipe_name()),
        serde_json::to_string(&envelope),
    ) {
        line.push('\n');
        let _ = pipe.write_all(line.as_bytes());
    }
}
