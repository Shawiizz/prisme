//! Bar 1 on synthetic tracks whose sections start on a known beat.

mod common;

use common::Synth;
use prisme_analysis::{analyze, AnalysisOptions, BEATS_PER_BAR};

const BPM: f64 = 128.0;
const FIRST_BEAT_S: f64 = 0.3;
/// Beats of a section.
const SECTION: usize = 32;

/// An intro of two stray kicks, on beats 0 and 4, under hats; then sections from beat
/// `sections_from`: the kick, the bass with it, a break without the kick, the kick again.
fn track(sections_from: usize) -> Synth {
    let period = 60.0 / BPM;
    let beat = |b: usize| FIRST_BEAT_S + b as f64 * period;
    let mut s = Synth::new(125.0);
    let beats = (125.0 / period) as usize - 1;
    s.hats(BPM, FIRST_BEAT_S, 0.2);
    s.kick(beat(0), 1.0);
    s.kick(beat(4), 1.0);
    let section = |b: usize| b.checked_sub(sections_from).map(|b| b / SECTION);
    for b in 0..beats {
        if section(b).is_some_and(|n| n >= 1 && n != 3) {
            s.kick(beat(b), 1.0);
        }
        if section(b).is_some_and(|n| n >= 2) {
            s.note(beat(b) + period / 2.0, period / 2.0, 33.0, 0.3);
        }
    }
    s
}

#[test]
fn bar_one_is_on_the_beat_the_sections_start_on() {
    for sections_from in [0, 2] {
        let tempo =
            analyze(&track(sections_from).track(), &AnalysisOptions::default()).tempo.expect("a tempo");
        let period = tempo.beat_period_s();
        let beats = ((tempo.first_beat_s - FIRST_BEAT_S) / period).round() as i64 - sections_from as i64;
        assert_eq!(
            beats.rem_euclid(i64::from(BEATS_PER_BAR)),
            0,
            "sections from beat {sections_from}: bar 1 at {:.3} s",
            tempo.first_beat_s
        );
        assert!(tempo.bars_sure, "sections from beat {sections_from}");
    }
}
