//! Synthetic music with a known tempo, beat grid and key.

#![allow(dead_code)]

use prisme_media::TrackAudio;
use std::f64::consts::TAU;

pub const SR: u32 = 44_100;

/// A mono buffer being written.
pub struct Synth {
    pub samples: Vec<f32>,
    rng: u64,
}

impl Synth {
    pub fn new(duration_s: f64) -> Self {
        Self { samples: vec![0.0; (duration_s * SR as f64) as usize], rng: 0x2545_f491_4f6c_dd1d }
    }

    fn noise(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }

    fn add(&mut self, i: usize, v: f64) {
        if let Some(s) = self.samples.get_mut(i) {
            *s += v as f32;
        }
    }

    /// Kick drum starting at `t`: pitch sweep from 150 Hz down to 50 Hz.
    pub fn kick(&mut self, t: f64, gain: f64) {
        let start = (t * SR as f64).round() as usize;
        let mut phase = 0.0;
        for i in 0..(0.3 * SR as f64) as usize {
            let tt = i as f64 / SR as f64;
            phase += TAU * (50.0 + 100.0 * (-tt * 30.0).exp()) / SR as f64;
            self.add(start + i, gain * (-tt * 10.0).exp() * phase.sin());
        }
    }

    /// Closed hi-hat starting at `t`: short burst of high-passed noise.
    pub fn hat(&mut self, t: f64, gain: f64) {
        let start = (t * SR as f64).round() as usize;
        let mut last = 0.0;
        for i in 0..(0.05 * SR as f64) as usize {
            let n = self.noise();
            let tt = i as f64 / SR as f64;
            self.add(start + i, gain * (-tt * 90.0).exp() * (n - last));
            last = n;
        }
    }

    /// A note with a few harmonics (MIDI pitch, fractional for detuning).
    pub fn note(&mut self, t: f64, duration: f64, pitch: f64, gain: f64) {
        let hz = 440.0 * 2f64.powf((pitch - 69.0) / 12.0);
        let start = (t * SR as f64).round() as usize;
        let len = (duration * SR as f64) as usize;
        for i in 0..len {
            let tt = i as f64 / SR as f64;
            let env = (tt / 0.01).min(1.0) * ((duration - tt) / 0.05).clamp(0.0, 1.0);
            let x: f64 = [1.0, 0.5, 0.3, 0.2]
                .iter()
                .enumerate()
                .map(|(h, a)| a * (TAU * hz * (h + 1) as f64 * tt).sin())
                .sum();
            self.add(start + i, gain * env * x);
        }
    }

    /// Four-on-the-floor kicks from `first_beat` with off-beat hats.
    pub fn drums(&mut self, bpm: f64, first_beat: f64, kick_gain: f64, hat_gain: f64) {
        let period = 60.0 / bpm;
        let end = self.samples.len() as f64 / SR as f64;
        let mut t = first_beat;
        while t < end {
            self.kick(t, kick_gain);
            self.hat(t + period / 2.0, hat_gain);
            t += period;
        }
    }

    /// Chords (MIDI pitches) one after the other, `chord_s` seconds each, looped over
    /// the whole buffer; the lowest note is doubled an octave down as a bass line.
    pub fn progression(&mut self, chords: &[Vec<f64>], chord_s: f64, gain: f64) {
        let end = self.samples.len() as f64 / SR as f64;
        let mut t = 0.0;
        let mut i = 0;
        while t < end {
            let chord = &chords[i % chords.len()];
            for &p in chord {
                self.note(t, chord_s, p, gain);
            }
            self.note(t, chord_s, chord[0] - 12.0, gain * 1.5);
            t += chord_s;
            i += 1;
        }
    }

    pub fn track(&self) -> TrackAudio {
        let peak = self.samples.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-9);
        let scale = 0.9 / peak;
        TrackAudio::from_interleaved(SR, self.samples.iter().flat_map(|&s| [s * scale, s * scale]).collect())
    }
}

/// Triad on `root` (MIDI), major or minor, in root position.
pub fn triad(root: f64, minor: bool) -> Vec<f64> {
    vec![root, root + if minor { 3.0 } else { 4.0 }, root + 7.0]
}

/// i - iv - i - VI - i - V in the minor key on `tonic` (pitch class). The major V
/// (harmonic minor) is what tells a minor key from its relative major: a loop of
/// i - VI - III - VII uses exactly the chords of the relative major.
pub fn minor_progression(tonic: u8, detune: f64) -> Vec<Vec<f64>> {
    let t = 48.0 + tonic as f64 + detune;
    let i = triad(t, true);
    vec![i.clone(), triad(t + 5.0, true), i.clone(), triad(t - 4.0, false), i, triad(t - 5.0, false)]
}

/// I - V - vi - IV in the major key on `tonic` (pitch class).
pub fn major_progression(tonic: u8, detune: f64) -> Vec<Vec<f64>> {
    let t = 60.0 + tonic as f64 + detune;
    vec![
        triad(t, false),
        triad(t + 7.0 - 12.0, false),
        triad(t + 9.0 - 12.0, true),
        triad(t + 5.0 - 12.0, false),
    ]
}
