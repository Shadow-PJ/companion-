//! A tiny local log file: %APPDATA%\dev.glowby.app\glowby.log (max ~256 KB,
//! one old copy kept). It never leaves your PC; it's there so problems that
//! would otherwise fail silently can be diagnosed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 256 * 1024;
static FILE: OnceLock<Mutex<PathBuf>> = OnceLock::new();

pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("glowby.log");
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("glowby.old.log"));
    }
    let _ = FILE.set(Mutex::new(path));
}

/// Extra detail (every hook event) only when started with GLOWBY_DEBUG=1.
pub fn debug(message: impl AsRef<str>) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("GLOWBY_DEBUG").is_some()) {
        line(message);
    }
}

pub fn line(message: impl AsRef<str>) {
    let text = format!("{} {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), message.as_ref());
    eprint!("{text}");
    if let Some(path) = FILE.get() {
        let path = path.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&*path) {
            let _ = f.write_all(text.as_bytes());
        }
    }
}
