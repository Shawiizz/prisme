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
    /// Share of the readings giving a position more than 20 ms off.
    wrong_ratio: f64,
    reaction_ms: Option<f64>,
}

fn run(sc: Scenario) -> Result {
    let format = &SERATO_CV02_A;
    let mut generator = SignalGenerator::new(format, SR, sc.start_ms, 60_000.0);
    let mut degrader = Degrader::new(sc.degradation, SR, 42);
    let mut decoder = TimecodeDecoder::new(DecoderConfig { format, sample_rate: SR, swap_channels: false });
    let mut buf = Vec::with_capacity(BLOCK * 2);
    let mut result = Result::default();
    let (mut speed_err, mut pos_err, mut evaluated, mut lost, mut wrong) = (0.0, 0.0, 0usize, 0usize, 0usize);
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
                    Some(_) => {
                        lost += 1;
                        wrong += 1;
                    }
                    None => lost += 1,
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
        result.wrong_ratio = wrong as f64 / evaluated as f64;
    }
    result
}

fn steady(speed: fn(f64) -> f64, start_ms: f64, degradation: Degradation) -> Result {
    run(Scenario { measure: Measure::Steady, start_ms, speed, degradation })
}

#[test]
fn clean_signal_locks_fast_and_is_exact() {
    let r = steady(|_| 1.0, 5_000.0, Degradation::default());
    // 20 bits fill the register, 24 more confirm it: a cycle each at 1 kHz
    assert!(r.lock_ms.unwrap() < 50.0, "{r:?}");
    assert!(r.speed_rms < 1e-3, "{r:?}");
    assert!(r.position_rms < 0.5, "{r:?}");
    assert_eq!(r.lost_ratio, 0.0, "{r:?}");
}

#[test]
fn pitched_and_half_speed_play() {
    for speed in [|_| 1.06, |_| 0.92, |_| 0.5] as [fn(f64) -> f64; 3] {
        let r = steady(speed, 5_000.0, Degradation::default());
        assert!(r.lock_ms.unwrap() < 100.0, "{r:?}");
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
    // a bounce drops the position (its phase says nothing), the bits give it back
    assert!(dropouts.speed_rms < 0.015 && dropouts.lost_ratio < 0.3, "{dropouts:?}");
    assert_eq!(dropouts.wrong_ratio, 0.0, "{dropouts:?}");
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
    assert!(r.reaction_ms.unwrap() < 50.0, "{r:?}");
}

#[test]
fn scratching_keeps_the_position() {
    let scratch = |t: f64| 1.0 + 1.6 * (TAU * 2.0 * t).sin();
    let r = steady(scratch, 20_000.0, Degradation::default());
    assert!(r.lost_ratio < 0.1, "{r:?}");
    assert!(r.speed_rms < 0.08, "{r:?}");
}

/// Plays a clean record at the nominal speed for 3 s, letting `alter` spoil each block
/// (given its start time), and hands `check` the time, the decoded and the true position.
fn play_altered(mut alter: impl FnMut(f64, &mut [f32]), mut check: impl FnMut(f64, Option<f64>, f64)) {
    let format = &SERATO_CV02_A;
    let mut generator = SignalGenerator::new(format, SR, 5_000.0, 60_000.0);
    let mut decoder = TimecodeDecoder::new(DecoderConfig { format, sample_rate: SR, swap_channels: false });
    let mut buf = Vec::with_capacity(BLOCK * 2);
    let mut done = 0;
    while done < SR as usize * 3 {
        let t = done as f64 / SR as f64;
        buf.clear();
        generator.render(BLOCK, |_| 1.0, &mut buf);
        alter(t, &mut buf);
        decoder.process(&buf);
        done += BLOCK;
        check(t, decoder.position_ms(), generator.position_ms());
    }
}

#[test]
fn a_needle_drop_relocks_while_the_needle_settles() {
    // after the drop the level dips to 40 % and back in 100 ms while the needle settles
    let format = &SERATO_CV02_A;
    let mut generator = SignalGenerator::new(format, SR, 5_000.0, 60_000.0);
    let mut decoder = TimecodeDecoder::new(DecoderConfig { format, sample_rate: SR, swap_channels: false });
    let mut buf = Vec::with_capacity(BLOCK * 2);
    let mut done = 0;
    let mut relocked_ms = None;
    while done < SR as usize * 3 && relocked_ms.is_none() {
        let t = done as f64 / SR as f64;
        if done == 2 * SR as usize {
            generator.jump_ms(40_000.0);
        }
        buf.clear();
        generator.render(BLOCK, |_| 1.0, &mut buf);
        if t >= 2.0 {
            let gain = 1.0 - 0.6 * (std::f64::consts::PI * ((t - 2.0) / 0.1).min(1.0)).sin();
            buf.iter_mut().for_each(|x| *x *= gain as f32);
        }
        decoder.process(&buf);
        done += BLOCK;
        if t > 2.0 && decoder.position_ms().is_some_and(|p| (p - generator.position_ms()).abs() < 5.0) {
            relocked_ms = Some((t - 2.0) * 1000.0);
        }
    }
    let ms = relocked_ms.expect("relocked");
    assert!(ms < 60.0, "relocked after {ms:.0} ms");
}

#[test]
fn unreadable_bits_keep_the_position() {
    // 200 ms where every block has a random level, as in a slow or reversing gesture:
    // no bit can be read, but the phase still says where the record is
    let mut seed = 7u64;
    play_altered(
        |t, block| {
            if (2.0..2.2).contains(&t) {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let gain = 0.6 + 0.8 * (seed >> 11) as f32 / (1u64 << 53) as f32;
                block.iter_mut().for_each(|x| *x *= gain);
            }
        },
        |t, pos, truth| {
            if t > 1.0 {
                let pos = pos.expect("the position is kept");
                assert!((pos - truth).abs() < 1.0, "at {t:.3} s: {pos} ms instead of {truth}");
            }
        },
    );
}

#[test]
fn a_position_is_never_stale_after_the_signal_was_lost() {
    // the needle off the record for 30 ms while it turns: the phase misses 30 cycles
    play_altered(
        |t, block| {
            if (2.0..2.03).contains(&t) {
                block.fill(0.0);
            }
        },
        |t, pos, truth| {
            // within the block that loses the signal the phase stops: up to two cycles behind
            if let Some(pos) = pos.filter(|_| t > 1.0) {
                assert!((pos - truth).abs() < 3.0, "at {t:.3} s: {pos} ms instead of {truth}");
            }
        },
    );
}
