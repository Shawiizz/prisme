//! The decks' samplers: sounds decoded once, kept to be given back to the engine after
//! an audio restart, their files remembered for the next start.

use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_engine::{Command, MAX_DECKS, SAMPLE_SLOTS};
use prisme_media::TrackAudio;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tracing::warn;

/// A sampler is for short sounds; longer files belong on a deck.
pub const MAX_SAMPLE_S: f64 = 30.0;

pub struct LoadedSample {
    pub path: PathBuf,
    pub audio: Arc<TrackAudio>,
}

pub type Samplers = [[Option<LoadedSample>; SAMPLE_SLOTS]; MAX_DECKS];

pub fn decode(path: &Path) -> AppResult<Arc<TrackAudio>> {
    let audio = prisme_media::decode_file(path)?;
    if audio.duration_s() > MAX_SAMPLE_S {
        return Err(AppError::SampleTooLong(MAX_SAMPLE_S));
    }
    Ok(Arc::new(audio))
}

/// A copy of `sound`, named `name`, kept in `samples` for the slot alone and decoded: the
/// slot's copy before stays until this one decodes.
pub fn keep_copy(
    samples: &Path,
    deck: usize,
    slot: usize,
    name: &str,
    mut sound: impl Read,
) -> AppResult<(PathBuf, Arc<TrackAudio>)> {
    let kept = samples.join(format!("{}-{}", deck + 1, slot + 1));
    let staged = kept.with_extension("new");
    let _ = fs::remove_dir_all(&staged);
    fs::create_dir_all(&staged)?;
    let copy = staged.join(name);
    let copied = File::create(&copy).and_then(|mut file| io::copy(&mut sound, &mut file));
    let audio = copied.map_err(AppError::from).and_then(|_| decode(&copy)).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staged);
    })?;
    let _ = fs::remove_dir_all(&kept);
    fs::rename(&staged, &kept)?;
    Ok((kept.join(name), audio))
}

/// What the pad shows: the file name without its extension.
pub fn name(path: &Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

/// Puts a decoded sound in a slot (and in the engine if it runs).
pub fn put(
    state: &AppState,
    deck: usize,
    slot: usize,
    path: PathBuf,
    audio: Arc<TrackAudio>,
) -> AppResult<()> {
    let sample = Some(audio.clone());
    lock(&state.samplers)[deck][slot] = Some(LoadedSample { path, audio });
    state.send_if_running(Command::SetSample { deck, slot, sample })
}

/// Decodes the sounds saved in the preferences, off the main thread.
pub fn load_saved(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let saved = state.preferences.get().decks;
        for (deck, preferences) in saved.iter().enumerate() {
            for (slot, path) in preferences.samples.iter().enumerate().take(SAMPLE_SLOTS) {
                let Some(path) = path.as_ref().map(PathBuf::from) else { continue };
                let loaded = decode(&path).and_then(|audio| put(&state, deck, slot, path.clone(), audio));
                if let Err(e) = loaded {
                    warn!("sample {} not loaded: {e}", path.display());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A WAV file of `seconds` of silence.
    fn wav(seconds: f64) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut bytes = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
        for _ in 0..(seconds * 8_000.0) as usize {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
        bytes.into_inner()
    }

    #[test]
    fn a_sound_with_no_path_is_kept_for_its_slot_alone() {
        let samples = std::env::temp_dir().join(format!("prisme-samples-{}", std::process::id()));
        let _ = fs::remove_dir_all(&samples);

        let (kick, audio) = keep_copy(&samples, 0, 1, "Kick.wav", &wav(0.5)[..]).unwrap();
        assert_eq!(kick, samples.join("1-2").join("Kick.wav"));
        assert!((audio.duration_s() - 0.5).abs() < 0.01);
        assert_eq!(name(&kick), "Kick");

        // a sound too long, or no sound, leaves the slot's copy as it was
        assert!(matches!(
            keep_copy(&samples, 0, 1, "Long.wav", &wav(31.0)[..]),
            Err(AppError::SampleTooLong(_))
        ));
        assert!(keep_copy(&samples, 0, 1, "Notes.txt", &b"no sound"[..]).is_err());
        assert!(kick.is_file());
        assert!(!samples.join("1-2.new").exists());

        // another sound replaces it; the other slots keep theirs
        keep_copy(&samples, 1, 0, "Kick.wav", &wav(0.2)[..]).unwrap();
        let (snare, _) = keep_copy(&samples, 0, 1, "Snare.wav", &wav(0.3)[..]).unwrap();
        let in_slot: Vec<_> =
            fs::read_dir(samples.join("1-2")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(in_slot, [snare.file_name().unwrap()]);
        assert!(samples.join("2-1").join("Kick.wav").is_file());
        fs::remove_dir_all(&samples).unwrap();
    }
}
