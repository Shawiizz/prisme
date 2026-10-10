//! Musical key of a track.
//!
//! 1. Spectral peaks between 100 Hz and 2 kHz (chords, and bass lines through their
//!    harmonics; drums give broad noise rather than peaks, except the kick's low
//!    ring, hence the floor), located precisely by interpolation.
//! 2. Tuning: the mean deviation of the peaks from the equal-tempered grid, so a
//!    track a few cents off A440 does not smear across two semitones.
//! 3. Chroma: the peak energy per pitch class over the whole track.
//! 4. Key: the major or minor profile, among the 24 rotations, that correlates best
//!    with the chroma.

use crate::dsp::{MonoSignal, Stft};
use std::f64::consts::TAU;
use std::fmt;

/// 0.74 s frames (1.35 Hz bins at 11 025 Hz) every 0.37 s.
const FRAME: usize = 8192;
const HOP: usize = 4096;
/// Peaks quieter than this fraction of the loudest one of their frame are ignored.
const PEAK_FLOOR: f32 = 0.05;

/// Key profiles: weight of each pitch class above the tonic, (major, minor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profiles {
    /// D. Temperley, "Music and Probability" (2007), from the Kostka-Payne corpus.
    Temperley,
    /// C. Krumhansl & E. Kessler (1982), probe-tone ratings.
    KrumhanslKessler,
}

impl Profiles {
    fn values(self) -> (&'static [f64; 12], &'static [f64; 12]) {
        match self {
            Profiles::Temperley => (
                &[0.748, 0.060, 0.488, 0.082, 0.670, 0.460, 0.096, 0.715, 0.104, 0.366, 0.057, 0.400],
                &[0.712, 0.084, 0.474, 0.618, 0.049, 0.460, 0.105, 0.747, 0.404, 0.067, 0.133, 0.330],
            ),
            Profiles::KrumhanslKessler => (
                &[6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88],
                &[6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17],
            ),
        }
    }
}

/// Tuning knobs of the key detection (the defaults are what the evaluation on real
/// tracks retained).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyConfig {
    /// Frequency range of the peaks that feed the chroma (Hz).
    pub min_hz: f64,
    pub max_hz: f64,
    /// Lowest peak used to estimate the tuning (Hz): kick drums ring around 50 Hz at
    /// arbitrary pitches and would drag it.
    pub tuning_min_hz: f64,
    /// Estimate the tuning (otherwise assume A440).
    pub estimate_tuning: bool,
    /// Weight peaks by `ln(1 + k * magnitude)` rather than the magnitude, so a loud
    /// bass does not drown the chords.
    pub log_magnitude: bool,
    pub profiles: Profiles,
}

impl Default for KeyConfig {
    /// On 24 reference tracks (Beatport / Tunebat keys, examples/key_eval.rs), starting
    /// at 100 Hz rather than 40 Hz took the agreement from 14 to 18: below that, the
    /// kick drums' ringing tails outweigh the bass lines.
    fn default() -> Self {
        Self {
            min_hz: 100.0,
            max_hz: 2_000.0,
            tuning_min_hz: 100.0,
            estimate_tuning: true,
            log_magnitude: false,
            profiles: Profiles::Temperley,
        }
    }
}

const MAJOR_NAMES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
const MINOR_NAMES: [&str; 12] = ["Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "Bbm", "Bm"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    tonic: u8,
    mode: Mode,
}

impl Key {
    /// `tonic`: pitch class, 0 = C ... 11 = B.
    pub fn new(tonic: u8, mode: Mode) -> Self {
        Self { tonic: tonic % 12, mode }
    }

    pub fn tonic(&self) -> u8 {
        self.tonic
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Usual DJ spelling: "Am", "F#m", "Eb"...
    pub fn name(&self) -> &'static str {
        match self.mode {
            Mode::Major => MAJOR_NAMES[self.tonic as usize],
            Mode::Minor => MINOR_NAMES[self.tonic as usize],
        }
    }

    /// Camelot wheel number (1..=12): neighbours on the wheel mix harmonically.
    pub fn camelot_number(&self) -> u8 {
        // C major is 8B and each step of a fifth moves one place; a minor key sits
        // with its relative major (three semitones up)
        let major_tonic = match self.mode {
            Mode::Major => self.tonic as u32,
            Mode::Minor => (self.tonic as u32 + 3) % 12,
        };
        match (8 + 7 * major_tonic) % 12 {
            0 => 12,
            n => n as u8,
        }
    }

    /// "8A" (minor) or "8B" (major).
    pub fn camelot(&self) -> String {
        let letter = match self.mode {
            Mode::Major => 'B',
            Mode::Minor => 'A',
        };
        format!("{}{}", self.camelot_number(), letter)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyEstimate {
    pub key: Key,
    /// 0 (ambiguous) to 1: how far the best key is ahead of the runner-up.
    pub confidence: f32,
    /// Deviation of the track's tuning from A440 (cents, -50..50).
    pub tuning_cents: f32,
}

/// Estimates the key with the default settings. `None` when the audio has no tonal
/// content.
pub fn estimate_key(signal: &MonoSignal) -> Option<KeyEstimate> {
    estimate_key_with(signal, &KeyConfig::default())
}

pub fn estimate_key_with(signal: &MonoSignal, config: &KeyConfig) -> Option<KeyEstimate> {
    SpectralPeaks::extract(signal, config.min_hz.min(config.tuning_min_hz), config.max_hz).estimate(config)
}

/// A spectral peak: frequency, pitch in MIDI note numbers (fractional), magnitude.
struct Peak {
    hz: f64,
    pitch: f64,
    magnitude: f64,
}

/// The spectral peaks of a whole track, from which keys are estimated (extract once,
/// estimate with several configurations).
pub struct SpectralPeaks {
    peaks: Vec<Peak>,
}

impl SpectralPeaks {
    pub fn extract(signal: &MonoSignal, min_hz: f64, max_hz: f64) -> Self {
        let bin_hz = signal.sample_rate / FRAME as f64;
        let first = ((min_hz / bin_hz).ceil() as usize).max(2);
        let last = ((max_hz / bin_hz).floor() as usize).min(FRAME / 2 - 1);
        let mut peaks = Vec::new();
        if first >= last {
            return Self { peaks };
        }

        let mut stft = Stft::new(FRAME, HOP);
        stft.for_each_frame(&signal.samples, |_, m| {
            let loudest = m[first..=last].iter().cloned().fold(0.0f32, f32::max);
            if loudest < 1e-4 {
                return;
            }
            let floor = loudest * PEAK_FLOOR;
            for k in first..=last {
                if m[k] > floor && m[k] > m[k - 1] && m[k] >= m[k + 1] {
                    // parabolic interpolation of the log magnitude
                    let (a, b, c) = (m[k - 1].max(1e-9).ln(), m[k].ln(), m[k + 1].max(1e-9).ln());
                    let denom = a - 2.0 * b + c;
                    let delta =
                        if denom.abs() > 1e-9 { (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { 0.0 };
                    let hz = (k as f64 + delta as f64) * bin_hz;
                    peaks.push(Peak { hz, pitch: 69.0 + 12.0 * (hz / 440.0).log2(), magnitude: m[k] as f64 });
                }
            }
        });
        Self { peaks }
    }

    pub fn estimate(&self, config: &KeyConfig) -> Option<KeyEstimate> {
        let weight = |p: &Peak| {
            if config.log_magnitude {
                (1.0 + 100.0 * p.magnitude).ln()
            } else {
                p.magnitude
            }
        };
        let tuning = if config.estimate_tuning {
            tuning_semitones(
                self.peaks.iter().filter(|p| p.hz >= config.tuning_min_hz && p.hz <= config.max_hz),
                weight,
            )
        } else {
            0.0
        };
        let chroma = chroma(
            self.peaks.iter().filter(|p| p.hz >= config.min_hz && p.hz <= config.max_hz),
            tuning,
            weight,
        );
        if chroma.iter().all(|&c| c <= 0.0) {
            return None;
        }

        let (major, minor) = config.profiles.values();
        let mut scores: Vec<(Key, f64)> = (0..12u8)
            .flat_map(|tonic| {
                [(Mode::Major, major), (Mode::Minor, minor)].map(|(mode, profile)| {
                    (Key::new(tonic, mode), correlation(&chroma, profile, tonic as usize))
                })
            })
            .collect();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        let (key, best) = scores[0];
        let second = scores[1].1;
        let confidence = if best > 0.0 { ((best - second) / best).clamp(0.0, 1.0) as f32 } else { 0.0 };
        Some(KeyEstimate { key, confidence, tuning_cents: (tuning * 100.0) as f32 })
    }
}

/// Weighted circular mean of the peaks' deviation from the semitone grid (semitones).
fn tuning_semitones<'a>(peaks: impl Iterator<Item = &'a Peak>, weight: impl Fn(&Peak) -> f64) -> f64 {
    let (mut s, mut c) = (0.0, 0.0);
    for p in peaks {
        let angle = TAU * (p.pitch - p.pitch.round());
        s += weight(p) * angle.sin();
        c += weight(p) * angle.cos();
    }
    s.atan2(c) / TAU
}

fn chroma<'a>(
    peaks: impl Iterator<Item = &'a Peak>,
    tuning: f64,
    weight: impl Fn(&Peak) -> f64,
) -> [f64; 12] {
    let mut chroma = [0.0; 12];
    for p in peaks {
        let pitch = p.pitch - tuning;
        let note = pitch.round();
        // full weight on the semitone, none half-way between two
        let closeness = 1.0 - 2.0 * (pitch - note).abs();
        chroma[(note as i64).rem_euclid(12) as usize] += weight(p) * closeness;
    }
    chroma
}

/// Pearson correlation between `chroma` and `profile` rotated to start on `tonic`.
fn correlation(chroma: &[f64; 12], profile: &[f64; 12], tonic: usize) -> f64 {
    let mean_c = chroma.iter().sum::<f64>() / 12.0;
    let mean_p = profile.iter().sum::<f64>() / 12.0;
    let (mut num, mut var_c, mut var_p) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let c = chroma[(tonic + i) % 12] - mean_c;
        let p = profile[i] - mean_p;
        num += c * p;
        var_c += c * c;
        var_p += p * p;
    }
    let denom = (var_c * var_p).sqrt();
    if denom > 0.0 {
        num / denom
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_camelot_codes() {
        let cases = [
            (9, Mode::Minor, "Am", "8A"),
            (0, Mode::Major, "C", "8B"),
            (0, Mode::Minor, "Cm", "5A"),
            (2, Mode::Minor, "Dm", "7A"),
            (8, Mode::Minor, "G#m", "1A"),
            (11, Mode::Major, "B", "1B"),
            (4, Mode::Major, "E", "12B"),
            (1, Mode::Minor, "C#m", "12A"),
            (6, Mode::Major, "F#", "2B"),
        ];
        for (tonic, mode, name, camelot) in cases {
            let key = Key::new(tonic, mode);
            assert_eq!(key.name(), name);
            assert_eq!(key.camelot(), camelot, "{name}");
        }
    }

    #[test]
    fn every_key_has_its_own_camelot_code() {
        let mut codes: Vec<String> = (0..12)
            .flat_map(|t| [Key::new(t, Mode::Major), Key::new(t, Mode::Minor)])
            .map(|k| k.camelot())
            .collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), 24);
    }
}
