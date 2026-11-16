//! Beat grids on synthetic tracks whose kicks start at known instants: the grid must sit on
//! the start of every kick, from the first beat to the last.

mod common;

use common::{minor_progression, Kick, Synth};
use prisme_analysis::{analyze, AnalysisOptions, Tempo};

/// Close enough for two tracks not to flam.
const TOLERANCE_MS: f64 = 2.0;

fn tempo_of(synth: &Synth) -> Tempo {
    analyze(&synth.track(), &AnalysisOptions::default()).tempo.expect("a tempo")
}

/// The largest distance between a kick and the nearest beat of the grid (ms), over the
/// track's kicks at `bpm` from `first_beat`, `silent` ones apart.
fn worst_offset_ms(
    tempo: &Tempo,
    synth: &Synth,
    bpm: f64,
    first_beat: f64,
    silent: std::ops::Range<usize>,
) -> f64 {
    let (period, end) = (tempo.beat_period_s(), synth.samples.len() as f64 / f64::from(common::SR));
    (0..)
        .map(|beat| (beat, first_beat + beat as f64 * 60.0 / bpm))
        .take_while(|&(_, t)| t < end)
        .filter(|(beat, _)| !silent.contains(beat))
        .map(|(_, t)| {
            let d = (tempo.first_beat_s - t).rem_euclid(period);
            d.min(period - d) * 1000.0
        })
        .fold(0.0, f64::max)
}

#[test]
fn the_grid_is_on_the_start_of_every_kind_of_kick() {
    for (name, kick) in [("click", Kick::CLICK), ("sweep", Kick::SWEEP), ("sub", Kick::SUB)] {
        for (bpm, first_beat) in [(122.0, 0.213), (128.0, 0.05), (140.0, 0.377)] {
            let mut s = Synth::new(90.0);
            s.kicks(bpm, first_beat, kick, 0..0);
            s.hats(bpm, first_beat, 0.3);
            let t = tempo_of(&s);
            assert_eq!(t.bpm, bpm, "{name} kick at {bpm} BPM");
            let off = worst_offset_ms(&t, &s, bpm, first_beat, 0..0);
            assert!(off < TOLERANCE_MS, "{name} kick at {bpm} BPM: grid off by {off:.2} ms");
        }
    }
}

#[test]
fn a_pad_ducked_by_the_kick_does_not_move_the_grid() {
    let (bpm, first_beat) = (124.0, 0.31);
    // chords and a bass line as loud as the kick, dipping under it
    let mut s = Synth::new(90.0);
    s.sidechained_pad(bpm, first_beat, &minor_progression(4, 0.0), 0.1, 0.8);
    s.kicks(bpm, first_beat, Kick::SWEEP, 0..0);
    s.hats(bpm, first_beat, 0.2);
    let t = tempo_of(&s);
    assert_eq!(t.bpm, bpm);
    let off = worst_offset_ms(&t, &s, bpm, first_beat, 0..0);
    assert!(off < TOLERANCE_MS, "grid off by {off:.2} ms");
}

#[test]
fn a_tempo_just_off_a_whole_bpm_is_kept_and_holds_to_the_end() {
    let (bpm, first_beat) = (123.95, 0.12);
    let mut s = Synth::new(300.0);
    s.kicks(bpm, first_beat, Kick::CLICK, 0..0);
    s.hats(bpm, first_beat, 0.3);
    let t = tempo_of(&s);
    assert!((t.bpm - bpm).abs() < 0.001, "found {:.4} BPM", t.bpm);
    let off = worst_offset_ms(&t, &s, bpm, first_beat, 0..0);
    assert!(off < TOLERANCE_MS, "grid off by {off:.2} ms");
}

#[test]
fn a_breakdown_without_kicks_does_not_move_the_grid() {
    let (bpm, first_beat) = (128.0, 0.4);
    let breakdown = 96..160;
    let mut s = Synth::new(150.0);
    s.kicks(bpm, first_beat, Kick::SUB, breakdown.clone());
    s.hats(bpm, first_beat, 0.2);
    s.progression(&minor_progression(9, 0.0), 60.0 / bpm * 4.0, 0.1);
    let t = tempo_of(&s);
    assert_eq!(t.bpm, bpm);
    let off = worst_offset_ms(&t, &s, bpm, first_beat, breakdown);
    assert!(off < TOLERANCE_MS, "grid off by {off:.2} ms");
}
