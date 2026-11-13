//! The identity of a file's audio stays when the file is renamed or tagged again, and tells
//! two tracks apart.

use prisme_media::{audio_id, write_genre, write_tempo_and_key, Replacing, TempoAndKey};
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("prisme-audio-id-{}-{name}", std::process::id()))
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// A second of a tone as a 16-bit stereo WAV.
fn tone(path: &Path, hz: f32) {
    let rate = 44_100u32;
    let pcm: Vec<u8> = (0..rate)
        .flat_map(|i| {
            let sample = ((i as f32 * hz * std::f32::consts::TAU / rate as f32).sin() * 20_000.0) as i16;
            [sample.to_le_bytes(), sample.to_le_bytes()].concat()
        })
        .collect();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    std::fs::write(path, wav).unwrap();
}

#[test]
fn a_file_renamed_and_tagged_again_is_known_by_its_audio() {
    for name in ["silence.mp3", "silence.flac", "silence.ogg", "silence.m4a"] {
        let before = audio_id(&fixture(name)).unwrap();
        let copy = scratch(&format!("renamed-{name}"));
        std::fs::copy(fixture(name), &copy).unwrap();
        let values = TempoAndKey { bpm: Some(124.0), key: Some("Am") };
        assert!(write_tempo_and_key(&copy, values, Replacing::Any).unwrap().any(), "{name}");
        assert!(write_genre(&copy, Some("A genre long enough to move the audio in the file")).unwrap());
        assert_ne!(std::fs::read(&copy).unwrap(), std::fs::read(fixture(name)).unwrap());
        assert_eq!(audio_id(&copy).unwrap(), before, "{name}");
        std::fs::remove_file(copy).unwrap();
    }
}

#[test]
fn two_tracks_are_told_apart() {
    let (low, high, tagged) = (scratch("low.wav"), scratch("high.wav"), scratch("tagged.wav"));
    tone(&low, 220.0);
    tone(&high, 440.0);
    std::fs::copy(&low, &tagged).unwrap();
    write_genre(&tagged, Some("House")).unwrap();
    assert_ne!(audio_id(&low).unwrap(), audio_id(&high).unwrap());
    assert_eq!(audio_id(&low).unwrap(), audio_id(&tagged).unwrap());
    for file in [low, high, tagged] {
        std::fs::remove_file(file).unwrap();
    }
}

#[test]
fn a_file_that_is_not_audio_has_no_identity() {
    let file = scratch("notes.mp3");
    std::fs::write(&file, "not audio at all").unwrap();
    assert!(audio_id(&file).is_err());
    std::fs::remove_file(file).unwrap();
}
