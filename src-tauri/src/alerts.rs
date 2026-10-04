//! Important alerts: things you shouldn't miss (a usage limit, a message Glowby
//! stopped to save your usage, a chat about to get expensive).
//!
//! Glowby makes sure you notice: he jumps out with a sound and a little bonk,
//! keeps the card open until you answer, sends a Windows notification if you
//! turned that on, and nudges you once more after a few minutes if the card is
//! still unanswered. Never while a fullscreen game runs (the card waits).

use crate::actions::Offer;
use crate::state::{self, AppState, lock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// One more nudge after this long if the alert is still unanswered.
const NUDGE_AGAIN_AFTER: Duration = Duration::from_secs(4 * 60);

/// How much an alert matters: a less important one never covers a more important
/// one that's still waiting for you.
fn weight(kind: &str) -> u8 {
    match kind {
        "guard" | "limitHit" => 5,
        "claudeBack" | "limits" => 4,
        "cacheSoon" => 3,
        "bigChat" => 2,
        _ => 1,
    }
}

/// Shows `offer` as an important alert. `notification` = (title, text) for the
/// Windows notification. false = a more important alert is still open, so this
/// one wasn't shown.
pub fn raise(app: &AppHandle, offer: Offer, stays: Duration, notification: Option<(String, String)>) -> bool {
    let state = app.state::<AppState>();
    let title = offer.title.clone();
    {
        let mut ui = lock(&state.ui);
        let now = Instant::now();
        if ui.offer.as_ref().is_some_and(|(o, until)| *until > now && weight(o.kind) > weight(offer.kind)) {
            return false;
        }
        ui.offer = Some((offer, now + stays));
    }
    if lock(&state.ui).game_active {
        state::publish(app); // the card waits for you after the game
        return true;
    }
    nudge(app);
    if let Some((title, body)) = notification
        && state.settings().alerts.windows_notifications
    {
        crate::notify::show(icon_file(app), title, body);
    }
    // still unanswered in a few minutes? one more nudge
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(NUDGE_AGAIN_AFTER).await;
        let state = app.state::<AppState>();
        let pending = lock(&state.ui).offer.as_ref().is_some_and(|(o, _)| o.title == title);
        if pending && !lock(&state.ui).game_active {
            nudge(&app);
        }
    });
    true
}

/// Jump out, bonk, sound.
fn nudge(app: &AppHandle) {
    state::hold_out(app, 90);
    crate::sounds::play(app, crate::sounds::Sound::Alert);
    crate::pet_window::show(app);
    let _ = app.emit_to(crate::pet_window::LABEL, "pet://nudge", ());
    state::publish(app);
}

/// Glowby's icon as a file, for the Windows notification.
fn icon_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    let path = app.state::<AppState>().paths.config_dir.join("glowby-icon.png");
    if !path.exists() {
        std::fs::write(&path, include_bytes!("../icons/128x128@2x.png")).ok()?;
    }
    Some(path)
}
