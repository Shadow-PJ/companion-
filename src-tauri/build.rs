use std::path::PathBuf;

fn main() {
    embed_hook_program();
    tauri_build::build()
}

/// Puts a copy of glowby-hook.exe inside glowby.exe, so any copy of glowby.exe
/// (say, one you put on your Desktop) can install its own hook program.
/// Build the hook first (`npm run release` does); without it, Glowby falls back
/// to a glowby-hook.exe sitting next to it.
fn embed_hook_program() {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    // OUT_DIR = <target>/<profile>/build/glowby-<hash>/out → <target>/<profile>
    let hook = out.ancestors().nth(3).map(|dir| dir.join("glowby-hook.exe"));
    let bytes = hook.as_ref().and_then(|h| std::fs::read(h).ok()).unwrap_or_default();
    if let Some(h) = &hook {
        println!("cargo:rerun-if-changed={}", h.display());
    }
    std::fs::write(out.join("glowby-hook.bin"), bytes).expect("write embedded hook");
}
