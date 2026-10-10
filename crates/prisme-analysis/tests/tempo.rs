//! Tempo and beat grid on synthetic tracks with a known grid.

mod common;

use common::Synth;
use prisme_analysis::{analyze, AnalysisOptions, Tempo};

fn tempo_of(synth: &Synth, min_bpm: f64) -> Tempo {
    analyze(&synth.track(), &AnalysisOptions { min_bpm }).tempo.expect("a tempo")
}

/// Distance between two grid phases (seconds), modulo the beat period.
fn phase_error(tempo: &Tempo, first_beat: f64) -> f64 {
    let period = tempo.beat_period_s();
    let d = (tempo.first_beat_s - first_beat).rem_euclid(period);
    d.min(period - d)
}

#[test]
fn finds_the_bpm_and_the_grid_of_dj_tempos() {
    for &(bpm, first_beat) in &[
        (95.5, 0.31),
        (120.0, 0.05),
        (123.45, 0.737),
        (128.0, 0.2),
        (140.0, 1.003),
        (150.0, 0.4),
        (174.0, 0.12),
    ] {
        let mut s = Synth::new(60.0);
        s.drums(bpm, first_beat, 1.0, 0.3);
        let t = tempo_of(&s, 88.0);
        assert!((t.bpm - bpm).abs() < 0.01, "{bpm} BPM: found {:.4}", t.bpm);
        let err = phase_error(&t, first_beat);
        assert!(err < 0.006, "{bpm} BPM: grid off by {:.1} ms", err * 1000.0);
        assert!(t.confidence > 0.5, "{bpm} BPM: confidence {}", t.confidence);
    }
}

#[test]
#[ignore = "calibration helper: prints the signed grid offset"]
fn print_grid_offsets() {
    for &(bpm, first_beat) in &[(95.5, 0.31), (120.0, 0.05), (128.0, 0.2), (140.0, 1.003), (174.0, 0.12)] {
        let mut s = Synth::new(60.0);
        s.drums(bpm, first_beat, 1.0, 0.3);
        let t = tempo_of(&s, 88.0);
        let period = t.beat_period_s();
        let d = (t.first_beat_s - first_beat + period / 2.0).rem_euclid(period) - period / 2.0;
        println!("{bpm}: {:+.2} ms", d * 1000.0);
    }
}

#[test]
fn folds_the_tempo_into_the_requested_octave() {
    let mut s = Synth::new(60.0);
    s.drums(80.0, 0.1, 1.0, 0.3);
    assert!((tempo_of(&s, 88.0).bpm - 160.0).abs() < 0.02);
    assert!((tempo_of(&s, 70.0).bpm - 80.0).abs() < 0.01);
}

#[test]
fn the_grid_sits_on_the_kicks_even_with_loud_hats() {
    let mut s = Synth::new(60.0);
    s.drums(126.0, 0.25, 0.6, 1.0);
    let t = tempo_of(&s, 88.0);
    assert!((t.bpm - 126.0).abs() < 0.01, "found {:.4}", t.bpm);
    assert!(phase_error(&t, 0.25) < 0.006, "grid off by {:.1} ms", phase_error(&t, 0.25) * 1000.0);
}

#[test]
fn long_tracks_get_a_very_precise_bpm() {
    let mut s = Synth::new(300.0);
    s.drums(127.83, 0.4, 1.0, 0.3);
    let t = tempo_of(&s, 88.0);
    // 0.002 BPM over 5 minutes is less than 5 ms of drift
    assert!((t.bpm - 127.83).abs() < 0.002, "found {:.4}", t.bpm);
    assert!(phase_error(&t, 0.4) < 0.006);
}

#[test]
fn whole_tempos_are_reported_exactly() {
    for bpm in [122.0, 128.0, 160.0] {
        let mut s = Synth::new(120.0);
        s.drums(bpm, 0.3, 1.0, 0.3);
        assert_eq!(tempo_of(&s, 88.0).bpm, bpm);
    }
}

#[test]
fn chords_over_drums_do_not_disturb_the_tempo() {
    let mut s = Synth::new(60.0);
    s.drums(124.0, 0.15, 1.0, 0.3);
    s.progression(&common::minor_progression(9, 0.0), 60.0 / 124.0 * 8.0, 0.15);
    let t = tempo_of(&s, 88.0);
    assert!((t.bpm - 124.0).abs() < 0.01, "found {:.4}", t.bpm);
    assert!(phase_error(&t, 0.15) < 0.006);
}

#[test]
fn silence_and_short_clips_have_no_tempo() {
    let silent = Synth::new(30.0);
    assert!(analyze(&silent.track(), &AnalysisOptions::default()).tempo.is_none());

    let mut short = Synth::new(5.0);
    short.drums(128.0, 0.0, 1.0, 0.3);
    assert!(analyze(&short.track(), &AnalysisOptions::default()).tempo.is_none());
}
