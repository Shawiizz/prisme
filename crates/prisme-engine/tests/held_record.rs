//! A record held still by a finger while the platter turns under it.

mod common;

use common::{sine_track, Rig, BLOCK, SR};
use prisme_engine::{Command, VinylMode};

/// The motion of a record held against a turning platter: still, but for the jitter a finger
/// lets through (`amplitude`, as a speed, at `hz`, with as much noise) and a creep forward.
fn held(amplitude: f64, hz: f64, creep: f64) -> impl FnMut(f64) -> f64 {
    let mut noise = 0x2545_f491_4f6c_dd1du64;
    move |t| {
        noise ^= noise << 13;
        noise ^= noise >> 7;
        noise ^= noise << 17;
        let random = (noise >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
        creep + amplitude * (std::f64::consts::TAU * hz * t).sin() + amplitude * random
    }
}

#[derive(Debug)]
struct Heard {
    /// How far the track travelled, both ways (s).
    travel_s: f64,
    rms: f32,
    /// The largest change from one sample to the next, within a block and from one to the next.
    largest_jump: f32,
    largest_jump_between_blocks: f32,
    /// The level of what is heard: above 60 Hz, as speakers play it.
    audible_rms: f32,
}

fn hold(mode: VinylMode, amplitude: f64, hz: f64, creep: f64) -> Heard {
    let mut rig = Rig::new();
    rig.turntable(0, 12.0, 1.0);
    rig.send(Command::LoadTrack { deck: 0, track: sine_track(60.0, 0.5) });
    rig.send(Command::SetVinylMode { deck: 0, mode });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(2.0);
    // the finger stops the record within a tenth of a second
    let start = 2.0;
    rig.set_turntable_motion(0, {
        let mut held = held(amplitude, hz, creep);
        move |t| {
            let into = t - start;
            if into < 0.1 {
                1.0 - into * 10.0
            } else {
                held(t)
            }
        }
    });
    rig.run(0.5);
    let mut heard = Heard {
        travel_s: 0.0,
        rms: 0.0,
        largest_jump: 0.0,
        largest_jump_between_blocks: 0.0,
        audible_rms: 0.0,
    };
    // a first order high-pass at 60 Hz
    let k = (-std::f32::consts::TAU * 60.0 / SR as f32).exp();
    let (mut x_prev, mut y) = (0.0f32, 0.0f32);
    let mut audible = 0.0;
    let mut last = rig.deck(0).position_s;
    let mut previous = rig.output()[rig.output().len() - 2];
    let mut sum = 0.0;
    let blocks = 2 * SR as usize / BLOCK;
    for _ in 0..blocks {
        rig.run(BLOCK as f64 / SR as f64);
        let now = rig.deck(0).position_s;
        heard.travel_s += (now - last).abs();
        last = now;
        let out: Vec<f32> = rig.output().iter().step_by(2).copied().collect();
        heard.largest_jump_between_blocks = heard.largest_jump_between_blocks.max((out[0] - previous).abs());
        for pair in out.windows(2) {
            heard.largest_jump = heard.largest_jump.max((pair[1] - pair[0]).abs());
        }
        sum += out.iter().map(|v| v * v).sum::<f32>();
        for &x in &out {
            y = k * (y + x - x_prev);
            x_prev = x;
            audible += y * y;
        }
        previous = out[out.len() - 1];
    }
    heard.rms = (sum / (blocks * BLOCK) as f32).sqrt();
    heard.audible_rms = (audible / (blocks * BLOCK) as f32).sqrt();
    heard
}

/// The audible level of the track dragged at a steady `speed`.
fn dragged(speed: f64) -> f32 {
    let mut rig = Rig::new();
    rig.turntable(0, 12.0, 1.0);
    rig.send(Command::LoadTrack { deck: 0, track: common::sine_track(60.0, 0.5) });
    rig.send(Command::SetVinylMode { deck: 0, mode: VinylMode::Relative });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(1.0);
    rig.set_turntable_speed(0, speed);
    rig.run(0.3);
    rig.run_rms(0.5)
}

#[test]
fn a_record_held_over_the_turning_platter_barely_sounds() {
    for mode in [VinylMode::Absolute, VinylMode::Relative] {
        // the track at full speed plays at 0.35
        for (amplitude, hz, creep, at_most) in
            [(0.005, 25.0, 0.0, 0.001), (0.02, 25.0, 0.0, 0.01), (0.05, 40.0, 0.002, 0.02)]
        {
            let heard = hold(mode, amplitude, hz, creep);
            assert!(heard.audible_rms < at_most, "{mode:?}, ±{amplitude}: {heard:?}");
            assert!(
                heard.largest_jump < 0.01 && heard.largest_jump_between_blocks < 0.01,
                "{mode:?}, ±{amplitude}: {heard:?}"
            );
        }
    }
}

#[test]
fn a_record_dragged_slowly_sounds_softer_and_a_scratch_at_full_level() {
    let full = dragged(1.0);
    assert!((0.34..0.36).contains(&full), "{full}");
    let slow = dragged(0.1);
    assert!(slow > 0.3 * full && slow < 0.5 * full, "{slow} at a tenth of the speed");
    let scratch = dragged(0.5);
    assert!((scratch - full).abs() < 0.01, "{scratch} at half the speed");
}
