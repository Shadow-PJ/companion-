//! Which sound to play, and whether to play it at all (Settings, volume, game
//! mode). The pet page synthesises the actual sound with WebAudio: no files.

use crate::state::{AppState, lock};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, Debug)]
pub enum Sound {
    Done,
    Alert,
    Notice,
    Error,
    LevelUp,
    Evolve,
    Break,
    /// You petted Glowby (a soft purr).
    Pet,
}

impl Sound {
    fn name(self) -> &'static str {
        match self {
            Sound::Done => "done",
            Sound::Alert => "alert",
            Sound::Notice => "notice",
            Sound::Error => "error",
            Sound::LevelUp => "levelup",
            Sound::Evolve => "evolve",
            Sound::Break => "break",
            Sound::Pet => "pet",
        }
    }
}

#[derive(Serialize, Clone)]
struct SoundMsg {
    name: &'static str,
    volume: f32,
}

pub fn play(app: &AppHandle, sound: Sound) {
    let state = app.state::<AppState>();
    let s = lock(&state.settings).sounds.clone();
    // Game mode: no sounds while a fullscreen game runs.
    if !s.enabled || s.volume == 0 || lock(&state.ui).game_active {
        return;
    }
    let wanted = match sound {
        Sound::Done => s.task_done,
        Sound::Alert | Sound::Notice => s.needs_you,
        Sound::Error => s.problems,
        Sound::LevelUp | Sound::Evolve => s.level_up,
        Sound::Break => s.breaks,
        Sound::Pet => true, // you asked for it by petting
    };
    if wanted {
        let _ = app.emit_to(crate::pet_window::LABEL, "pet://sound", SoundMsg { name: sound.name(), volume: s.volume as f32 / 100.0 });
    }
}
