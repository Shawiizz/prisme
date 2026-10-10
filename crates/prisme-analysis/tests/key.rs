//! Key detection on synthetic chord progressions in every key.

mod common;

use common::{major_progression, minor_progression, Synth};
use prisme_analysis::{analyze, AnalysisOptions, Key, KeyEstimate, Mode};

fn key_of(chords: &[Vec<f64>], with_drums: bool) -> KeyEstimate {
    let mut s = Synth::new(32.0);
    s.progression(chords, 2.0, 0.2);
    if with_drums {
        s.drums(128.0, 0.0, 1.0, 0.4);
    }
    analyze(&s.track(), &AnalysisOptions::default()).key.expect("a key")
}

#[test]
fn finds_every_minor_key() {
    for tonic in 0..12 {
        let k = key_of(&minor_progression(tonic, 0.0), false);
        assert_eq!(k.key, Key::new(tonic, Mode::Minor), "expected {}", Key::new(tonic, Mode::Minor));
    }
}

#[test]
fn finds_every_major_key() {
    for tonic in 0..12 {
        let k = key_of(&major_progression(tonic, 0.0), false);
        assert_eq!(k.key, Key::new(tonic, Mode::Major), "expected {}", Key::new(tonic, Mode::Major));
    }
}

#[test]
fn drums_do_not_change_the_key() {
    for tonic in [0, 5, 9] {
        assert_eq!(key_of(&minor_progression(tonic, 0.0), true).key, Key::new(tonic, Mode::Minor));
        assert_eq!(key_of(&major_progression(tonic, 0.0), true).key, Key::new(tonic, Mode::Major));
    }
}

#[test]
fn detuned_tracks_are_handled() {
    for detune in [-0.4, -0.2, 0.25, 0.45] {
        let k = key_of(&minor_progression(2, detune), false);
        assert_eq!(k.key, Key::new(2, Mode::Minor), "detuned by {detune}");
        let cents = (detune * 100.0) as f32;
        assert!((k.tuning_cents - cents).abs() < 5.0, "tuning {} instead of {cents}", k.tuning_cents);
    }
}

#[test]
fn drums_alone_have_no_confident_key() {
    let mut s = Synth::new(32.0);
    s.drums(128.0, 0.0, 1.0, 0.4);
    let key = analyze(&s.track(), &AnalysisOptions::default()).key;
    assert!(key.is_none_or(|k| k.confidence < 0.2), "{key:?}");
}
