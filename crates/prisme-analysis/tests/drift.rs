//! Grids pinned on the kicks of tracks whose tempo drifts, and steady grids left steady.

mod common;

use common::{Kick, Synth};
use prisme_analysis::{analyze, AnalysisOptions};

/// The instants of the kicks of a drummer around `bpm`: each beat a little longer or shorter
/// than the last, the tempo swaying by `sway` (2 %: 0.02) over `bars` bars.
fn swaying(bpm: f64, first_beat: f64, sway: f64, bars: f64, duration_s: f64) -> Vec<f64> {
    let mut beats = vec![first_beat];
    while beats.last().is_some_and(|&t| t < duration_s - 1.0) {
        let k = beats.len() as f64;
        let period = 60.0 / bpm * (1.0 + sway * (std::f64::consts::TAU * k / (4.0 * bars)).sin());
        beats.push(beats[beats.len() - 1] + period);
    }
    beats
}

#[test]
fn a_drifting_drummer_is_followed_kick_by_kick() {
    let duration_s = 150.0;
    let kicks = swaying(124.0, 0.5, 0.02, 16.0, duration_s);
    let mut synth = Synth::new(duration_s);
    for (k, &t) in kicks.iter().enumerate() {
        synth.shaped_kick(t, 1.0, Kick::CLICK);
        if let Some(&next) = kicks.get(k + 1) {
            synth.hat((t + next) / 2.0, 0.2);
        }
    }
    let analysis = analyze(&synth.track(), &AnalysisOptions::default());
    let grid = analysis.beat_grid().expect("a grid");
    assert!(analysis.anchors.is_some(), "the tempo drifts");
    // every kick as far from its beat as the others (the grid sits on the kicks' click), but
    // for the first and the last few bars
    let offsets: Vec<f64> = kicks[16..kicks.len() - 16]
        .iter()
        .map(|&t| (grid.time_of(grid.beats_at(t).round()) - t) * 1000.0)
        .collect();
    let mut sorted = offsets.clone();
    sorted.sort_by(f64::total_cmp);
    let bias = sorted[sorted.len() / 2];
    let mut errors: Vec<f64> = offsets.iter().map(|o| (o - bias).abs()).collect();
    errors.sort_by(f64::total_cmp);
    let (median, worst) = (errors[errors.len() / 2], errors[errors.len() - 1]);
    assert!(median < 3.0 && worst < 10.0, "median {median:.1} ms, worst {worst:.1} ms");
    let steady = prisme_grid::BeatGrid::steady(124.0, 0.5, true);
    let off =
        kicks.iter().map(|&t| (steady.time_of(steady.beats_at(t).round()) - t).abs()).fold(0.0, f64::max);
    assert!(off > 0.05, "a steady grid strays {off:.3} s");
}

#[test]
fn a_steady_track_with_a_breakdown_keeps_one_tempo() {
    let mut synth = Synth::new(150.0);
    // kicks but in a breakdown of 32 beats, hats throughout
    synth.kicks(126.0, 0.3, Kick::SWEEP, 128..160);
    synth.hats(126.0, 0.3, 0.2);
    let analysis = analyze(&synth.track(), &AnalysisOptions::default());
    assert!(analysis.anchors.is_none(), "{:?}", analysis.anchors);
    assert!(analysis.beat_grid().expect("a grid").is_steady());
}
