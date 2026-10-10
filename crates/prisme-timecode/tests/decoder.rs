//! Behaviour of the decoder on synthetic signals. The thresholds come from the
//! benchmark against xwax (see the spike in the Mixxx fork) with some margin: a change
//! that makes the decoder slower or less robust fails here.

use prisme_timecode::testing::{Degradation, Degrader, SignalGenerator};
use prisme_timecode::{DecoderConfig, TimecodeDecoder, SERATO_CV02_A};
use std::f64::consts::TAU;

const SR: u32 = 48_000;
const BLOCK: usize = 64;

#[derive(Clone, Copy, PartialEq)]
enum Measure {
    /// steady accuracy after 1.5 s
    Steady,
    /// reaction to a change at t = 2 s: time until |speed - target| < tol
    Reaction { target: f64, tol: f64 },
    /// needle drop at t = 2 s (+40 s): time until the position is right again
    NeedleDrop,
}

struct Scenario {
    measure: Measure,
    start_ms: f64,
    speed: fn(f64) -> f64,
    degradation: Degradation,
}

#[derive(Debug, Default)]
struct Result {
    lock_ms: Option<f64>,
    speed_rms: f64,
    position_rms: f64,
    lost_ratio: f64,
    reaction_ms: Option<f64>,
}

fn run(sc: Scenario) -> Result {
    let format = &SERATO_CV02_A;
    let mut generator = SignalGenerator::new(format, SR, sc.start_ms, 60_000.0);
    let mut degrader = Degrader::new(sc.degradation, SR, 42);
    let mut decoder = TimecodeDecoder::new(DecoderConfig { format, sample_rate: SR, swap_channels: false });
    let mut buf = Vec::with_capacity(BLOCK * 2);
    let mut result = Result::default();
    let (mut speed_err, mut pos_err, mut evaluated, mut lost) = (0.0, 0.0, 0usize, 0usize);
    let mut done = 0usize;
    let mut jumped = false;
    while done < (SR as f64 * 4.5) as usize {
        let t0 = done as f64 / SR as f64;
        if sc.measure == Measure::NeedleDrop && !jumped && t0 >= 2.0 {
            generator.jump_ms(40_000.0);
            jumped = true;
        }
        let speed = sc.speed;
        buf.clear();
        generator.render(BLOCK, |i| speed(t0 + i as f64 / SR as f64), &mut buf);
        degrader.apply(&mut buf);
        decoder.process(&buf);
        done += BLOCK;

        let t = done as f64 / SR as f64;
        let truth_speed = (sc.speed)(t);
        let truth_pos = generator.position_ms();
        let pos = decoder.position_ms();
        let pos_ok = pos.is_some_and(|p| (p - truth_pos).abs() < 5.0);
        if result.lock_ms.is_none() && pos_ok {
            result.lock_ms = Some(t * 1000.0);
        }
        match sc.measure {
            Measure::Steady if t > 1.5 => {
                evaluated += 1;
                speed_err += (decoder.speed() - truth_speed).powi(2);
                match pos {
                    Some(p) if (p - truth_pos).abs() < 20.0 => pos_err += (p - truth_pos).powi(2),
                    _ => lost += 1,
                }
            }
            Measure::Reaction { target, tol } => {
                if t > 2.0 && result.reaction_ms.is_none() && (decoder.speed() - target).abs() < tol {
                    result.reaction_ms = Some((t - 2.0) * 1000.0);
                }
            }
            Measure::NeedleDrop => {
                if t > 2.0 && result.reaction_ms.is_none() && pos_ok {
                    result.reaction_ms = Some((t - 2.0) * 1000.0);
                }
            }
            _ => {}
        }
    }
    if evaluated > 0 {
        result.speed_rms = (speed_err / evaluated as f64).sqrt();
        let kept = (evaluated - lost).max(1);
        result.position_rms = (pos_err / kept as f64).sqrt();
        result.lost_ratio = lost as f64 / evaluated as f64;
    }
    result
}

fn steady(speed: fn(f64) -> f64, start_ms: f64, degradation: Degradation) -> Result {
    run(Scenario { measure: Measure::Steady, start_ms, speed, degradation })
}

#[test]
fn clean_signal_locks_fast_and_is_exact() {
    let r = steady(|_| 1.0, 5_000.0, Degradation::default());
    assert!(r.lock_ms.unwrap() < 40.0, "{r:?}");
    assert!(r.speed_rms < 1e-3, "{r:?}");
    assert!(r.position_rms < 0.5, "{r:?}");
    assert_eq!(r.lost_ratio, 0.0, "{r:?}");
}

#[test]
fn pitched_and_half_speed_play() {
    for speed in [|_| 1.06, |_| 0.92, |_| 0.5] as [fn(f64) -> f64; 3] {
        let r = steady(speed, 5_000.0, Degradation::default());
        assert!(r.lock_ms.unwrap() < 80.0, "{r:?}");
        assert!(r.speed_rms < 1e-3, "{r:?}");
        assert_eq!(r.lost_ratio, 0.0, "{r:?}");
    }
}

#[test]
fn backwards_position_is_not_offset() {
    let r = steady(|_| -1.0, 200_000.0, Degradation::default());
    assert!(r.position_rms < 1.0, "{r:?}");
    assert_eq!(r.lost_ratio, 0.0, "{r:?}");
}

#[test]
fn wow_and_flutter_are_followed() {
    let r = steady(
        |t| 1.0 + 0.005 * (TAU * 0.55 * t).sin() + 0.002 * (TAU * 8.0 * t).sin(),
        5_000.0,
        Degradation::default(),
    );
    assert!(r.speed_rms < 0.003, "{r:?}");
}

#[test]
fn noise_and_low_level() {
    let noisy = steady(|_| 1.0, 5_000.0, Degradation { noise: 0.05, ..Default::default() });
    assert!(noisy.speed_rms < 0.002, "{noisy:?}");
    let quiet = steady(|_| 1.0, 5_000.0, Degradation { gain: 0.032, noise: 0.002, ..Default::default() });
    assert!(quiet.speed_rms < 0.002, "{quiet:?}");
    assert_eq!(quiet.lost_ratio, 0.0, "{quiet:?}");
}

#[test]
fn dust_dropouts_and_worn_records() {
    let dust = steady(|_| 1.0, 5_000.0, Degradation { crackles_per_s: 30.0, ..Default::default() });
    assert!(dust.speed_rms < 0.012 && dust.lost_ratio < 0.2, "{dust:?}");
    let dropouts =
        steady(|_| 1.0, 5_000.0, Degradation { dropouts_per_s: 4.0, dropout_ms: 15.0, ..Default::default() });
    assert!(dropouts.speed_rms < 0.015 && dropouts.lost_ratio < 0.2, "{dropouts:?}");
    let worn = steady(
        |t| 1.02 + 0.004 * (TAU * 0.55 * t).sin(),
        5_000.0,
        Degradation {
            noise: 0.03,
            crackles_per_s: 15.0,
            imbalance: 0.7,
            dropouts_per_s: 1.0,
            dropout_ms: 8.0,
            ..Default::default()
        },
    );
    assert!(worn.speed_rms < 0.015 && worn.lost_ratio < 0.15, "{worn:?}");
}

#[test]
fn reacts_quickly() {
    let react = |target: f64, tol: f64, speed: fn(f64) -> f64| {
        run(Scenario {
            measure: Measure::Reaction { target, tol },
            start_ms: 5_000.0,
            speed,
            degradation: Degradation::default(),
        })
        .reaction_ms
    };
    let nudge = react(1.06, 0.01, |t| if t < 2.0 { 1.0 } else { 1.06 }).unwrap();
    assert!(nudge < 10.0, "nudge {nudge} ms");
    let reverse = react(-1.0, 0.1, |t| if t < 2.0 { 1.0 } else { -1.0 }).unwrap();
    assert!(reverse < 15.0, "reverse {reverse} ms");
    let stop = react(0.0, 0.05, |t| if t < 2.0 { 1.0 } else { 0.0 }).unwrap();
    assert!(stop < 45.0, "stop {stop} ms");
}

#[test]
fn needle_drop_relocks() {
    let r = run(Scenario {
        measure: Measure::NeedleDrop,
        start_ms: 5_000.0,
        speed: |_| 1.0,
        degradation: Degradation::default(),
    });
    assert!(r.reaction_ms.unwrap() < 35.0, "{r:?}");
}

#[test]
fn scratching_keeps_the_position() {
    let scratch = |t: f64| 1.0 + 1.6 * (TAU * 2.0 * t).sin();
    let r = steady(scratch, 20_000.0, Degradation::default());
    assert!(r.lost_ratio < 0.1, "{r:?}");
    assert!(r.speed_rms < 0.08, "{r:?}");
}
