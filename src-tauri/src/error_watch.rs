//! Error watcher (OPT-IN, off by default).
//!
//! When it's on, Windows tells Glowby each time the clipboard changes
//! (`AddClipboardFormatListener` → `WM_CLIPBOARDUPDATE`, no polling). Glowby
//! reads the text, checks it LOCALLY with simple rules, and throws it away
//! immediately unless it looks like an error. Nothing is logged or saved, and
//! clipboard items marked private by password managers are skipped unread.

use crate::actions::{self, Offer};
use crate::state::{self, AppState, lock};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, RemoveClipboardFormatListener,
};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

const CF_UNICODETEXT: u32 = 13;
/// Bigger clipboard texts are ignored (not an error message you'd paste).
const MAX_CHARS: usize = 20_000;

static LISTENING: AtomicBool = AtomicBool::new(false);
static LAST_OFFERED: AtomicU64 = AtomicU64::new(0);

/// Starts or stops listening. Off means Glowby never even looks at the clipboard.
pub fn apply(app: &AppHandle, enabled: bool) {
    let _ = app.run_on_main_thread(move || {
        let zone = crate::hotzone::hwnd();
        if zone.is_null() {
            return;
        }
        unsafe {
            if enabled && !LISTENING.swap(true, Ordering::Relaxed) {
                AddClipboardFormatListener(zone);
            } else if !enabled && LISTENING.swap(false, Ordering::Relaxed) {
                RemoveClipboardFormatListener(zone);
            }
        }
    });
}

/// Called (on the UI thread) a moment after the clipboard changed.
pub fn on_clipboard_settled(app: &AppHandle, owner: HWND) {
    let state = app.state::<AppState>();
    if !lock(&state.settings).error_watcher {
        return;
    }
    let Some(text) = (unsafe { read_clipboard_text(owner) }) else { return };
    if !looks_like_error(&text) {
        return; // not an error: forgotten right here
    }
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    let hash = hasher.finish();
    if LAST_OFFERED.swap(hash, Ordering::Relaxed) == hash {
        return; // same error copied again
    }
    actions::remember_error(app, &text, "something you copied".into());
    let settings = state.settings();
    let (dir, _) = crate::chat::effective_dir(&state, &settings);
    let offer = Offer {
        kind: "clipboardError",
        title: "That looks like an error.".into(),
        detail: crate::sessions::first_line(&text, 160),
        project: crate::sessions::project_name(&dir),
        url: None,
        dir: None,
    };
    lock(&state.ui).offer = Some((offer, Instant::now() + Duration::from_secs(25)));
    state::hold_out(app, 25);
    crate::sounds::play(app, crate::sounds::Sound::Notice);
    crate::pet_window::show(app);
    state::publish(app);
}

unsafe fn read_clipboard_text(owner: HWND) -> Option<String> {
    unsafe {
        if private_marker_present() || IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }
        if OpenClipboard(owner) == 0 {
            return None;
        }
        let text = read_open_clipboard();
        CloseClipboard();
        text
    }
}

unsafe fn read_open_clipboard() -> Option<String> {
    unsafe {
        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle.is_null() {
            return None;
        }
        let ptr = GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            return None;
        }
        let max = (GlobalSize(handle) / 2).min(MAX_CHARS + 1);
        let mut len = 0;
        while len < max && *ptr.add(len) != 0 {
            len += 1;
        }
        let text = (len <= MAX_CHARS).then(|| String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len)));
        GlobalUnlock(handle);
        text
    }
}

/// Password managers and other apps mark private clipboard content with these
/// formats. If any is present, Glowby doesn't read the clipboard at all.
unsafe fn private_marker_present() -> bool {
    ["ExcludeClipboardContentFromMonitorProcessing", "CanIncludeInClipboardHistory", "Clipboard Viewer Ignore"]
        .iter()
        .any(|name| unsafe {
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            let format = RegisterClipboardFormatW(wide.as_ptr());
            format != 0 && IsClipboardFormatAvailable(format) != 0
        })
}

/// Local, rule-based check: does this text look like an error message or stack trace?
pub fn looks_like_error(text: &str) -> bool {
    let text = text.trim();
    if text.len() < 12 || text.len() > MAX_CHARS * 2 {
        return false;
    }
    // One distinctive line is enough.
    const STRONG: &[&str] = &[
        "Traceback (most recent call last)",
        "panicked at",
        "error[E0",
        "npm ERR!",
        "Exception in thread",
        "Unhandled exception",
        "UnhandledPromiseRejection",
        "Segmentation fault",
        "FullyQualifiedErrorId",
        "is not recognized as an internal or external command",
        "error: could not compile",
        "Build FAILED",
        "fatal error",
        "Cannot find module",
    ];
    if STRONG.iter().any(|m| text.contains(m)) {
        return true;
    }
    // Typical error words (need a second clue).
    let mut clues = 0;
    for line in text.lines().map(str::trim) {
        let lower = line.to_lowercase();
        if lower.starts_with("error") || lower.starts_with("fatal:") || lower.contains("exception:") {
            clues += 1;
        }
        // TypeError:, ValueError:, ModuleNotFoundError: …
        if line.split_whitespace().next().is_some_and(|w| w.ends_with("Error:") && w.len() > 6) {
            clues += 1;
        }
        if ["error TS", "error CS", "error C2", "error LNK", ": error:", " ERR!", "undefined reference to",
            "No such file or directory", "Permission denied", "command not found", "ENOENT", "EACCES", "EADDRINUSE",
            "exit code", "Exit code", "failed with"]
            .iter()
            .any(|m| line.contains(m))
        {
            clues += 1;
        }
    }
    // Stack-trace shaped lines.
    let frames = text
        .lines()
        .map(str::trim)
        .filter(|l| {
            (l.starts_with("at ") && (l.contains('(') || l.contains(':')))
                || (l.starts_with("File \"") && l.contains("line "))
                || l.starts_with("--> ")
                || has_line_col(l)
        })
        .count();
    clues >= 2 || (clues >= 1 && frames >= 1)
}

/// `src/app.ts:12:5` style locations.
fn has_line_col(line: &str) -> bool {
    line.split(|c: char| c.is_whitespace() || c == '(' || c == ')').any(|part| {
        let pieces: Vec<&str> = part.rsplitn(3, ':').collect();
        pieces.len() == 3
            && pieces[0].chars().all(|c| c.is_ascii_digit())
            && !pieces[0].is_empty()
            && pieces[1].chars().all(|c| c.is_ascii_digit())
            && !pieces[1].is_empty()
            && pieces[2].contains('.')
    })
}

#[cfg(test)]
mod tests {
    use super::looks_like_error;

    #[test]
    fn recognises_real_errors() {
        let samples = [
            "Traceback (most recent call last):\n  File \"app.py\", line 3, in <module>\n    import foo\nModuleNotFoundError: No module named 'foo'",
            "error[E0308]: mismatched types\n --> src/main.rs:4:18",
            "TypeError: Cannot read properties of undefined (reading 'map')\n    at App (src/App.tsx:12:20)",
            "npm ERR! code ENOENT\nnpm ERR! syscall open",
            "src/index.ts:3:7 - error TS2322: Type 'string' is not assignable to type 'number'.",
            "thread 'main' panicked at src/main.rs:2:5:\nattempt to divide by zero",
            "Get-Item : Cannot find path 'C:\\nope' because it does not exist.\n    + CategoryInfo : ObjectNotFound\n    + FullyQualifiedErrorId : PathNotFound",
            "Program.cs(10,5): error CS1002: ; expected\nBuild FAILED.",
        ];
        for s in samples {
            assert!(looks_like_error(s), "should be an error: {s}");
        }
    }

    #[test]
    fn ignores_normal_text() {
        let samples = [
            "Hello! Are we still meeting at 5?",
            "https://github.com/tauri-apps/tauri",
            "No errors found. All 42 tests passed.",
            "try:\n    run()\nexcept ValueError:\n    pass",
            "correct-horse-battery-staple-91",
            "Error",
            "The report covers errors in measurement and how to reduce them in future studies.",
        ];
        for s in samples {
            assert!(!looks_like_error(s), "should NOT be an error: {s}");
        }
    }
}
