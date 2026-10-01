//! The "language" spoken between `glowby-hook.exe` (the client Claude Code runs)
//! and the Glowby app (the server), over a Windows named pipe.
//!
//! Wire format: one JSON object per line ("newline-delimited JSON").
//!   hook  -> app : `HookEnvelope`
//!   app   -> hook: `HookReply`   (only when `wants_reply` is true)

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// Env var Glowby sets on the `claude -p` processes it starts for the chat box.
/// Hooks inherit it, so the app can tell "my own chat" apart from your terminal sessions.
pub const PET_CHAT_ENV: &str = "GLOWBY_PET_CHAT";

/// Pseudo-event name: the PreToolUse gate Glowby installs only for its own chat sessions.
pub const CHAT_GATE_EVENT: &str = "ChatGate";

/// Pseudo-event a second Glowby launch sends to the running one ("please show yourself").
pub const SHOW_EVENT: &str = "__glowby_show";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HookEnvelope {
    pub v: u32,
    /// Event name from the hook's command-line argument, e.g. "PreToolUse".
    pub event: String,
    /// True when the hook waits for Glowby's answer (permission questions).
    pub wants_reply: bool,
    /// True when the hook runs inside a chat that Glowby itself started.
    pub from_pet_chat: bool,
    /// The JSON Claude Code wrote to the hook's stdin (long strings trimmed).
    pub payload: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookReply {
    Allow,
    Deny { message: String },
    /// "No opinion": Claude Code continues with its normal flow (e.g. its own prompt).
    Pass,
}

/// Name of the pipe. It contains your Windows account SID, so every user on the PC
/// gets a separate pipe, e.g. `\\.\pipe\glowby-S-1-5-21-...-500`.
pub fn pipe_name() -> String {
    match win::current_user_sid().and_then(|sid| win::sid_to_string(&sid)) {
        Some(sid) => format!(r"\\.\pipe\glowby-{sid}"),
        None => r"\\.\pipe\glowby-hooks".to_string(),
    }
}

#[cfg(windows)]
pub mod win {
    //! Small, careful wrappers around the Win32 security APIs we need.
    use std::ffi::c_void;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// Raw bytes of the SID (security identifier) of the user running this process.
    pub fn current_user_sid() -> Option<Vec<u8>> {
        unsafe { process_token_sid(GetCurrentProcess()) }
    }

    /// SID of the user that owns process `pid` (None if we may not look at it).
    pub fn process_user_sid(pid: u32) -> Option<Vec<u8>> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let sid = process_token_sid(process);
            CloseHandle(process);
            sid
        }
    }

    /// Security check used by the hook: is the program listening on the pipe
    /// running as *me*? If another user grabbed the pipe name first, the answer
    /// is no, and the hook stays silent (fail-open) instead of trusting it.
    pub fn pipe_server_is_same_user(pipe: &std::fs::File) -> bool {
        use std::os::windows::io::AsRawHandle;
        let mut pid = 0u32;
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) } == 0 {
            return false;
        }
        match (process_user_sid(pid), current_user_sid()) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    pub fn sid_to_string(sid: &[u8]) -> Option<String> {
        unsafe {
            let mut wide: *mut u16 = null_mut();
            if ConvertSidToStringSidW(sid.as_ptr() as *mut c_void, &mut wide) == 0 || wide.is_null() {
                return None;
            }
            let mut len = 0;
            while *wide.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(wide, len));
            LocalFree(wide as *mut c_void);
            Some(text)
        }
    }

    unsafe fn process_token_sid(process: HANDLE) -> Option<Vec<u8>> {
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let mut needed = 0u32;
            GetTokenInformation(token, TokenUser, null_mut(), 0, &mut needed);
            // u64 buffer => 8-byte alignment, which TOKEN_USER requires.
            let mut buf = vec![0u64; (needed as usize).div_ceil(8).max(1)];
            let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr() as *mut c_void, needed, &mut needed);
            CloseHandle(token);
            if ok == 0 {
                return None;
            }
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let sid = user.User.Sid;
            let len = GetLengthSid(sid) as usize;
            Some(std::slice::from_raw_parts(sid as *const u8, len).to_vec())
        }
    }
}

#[cfg(not(windows))]
pub mod win {
    pub fn current_user_sid() -> Option<Vec<u8>> {
        None
    }
    pub fn sid_to_string(_sid: &[u8]) -> Option<String> {
        None
    }
}
