//! Waveform summary: for each time slice, the peak amplitude and the energy of the low,
//! mid and high bands. The UI draws the height from the peak and the color from the
//! band balance.

use prisme_media::TrackAudio;
use std::f64::consts::TAU;

/// Crossover frequencies between the bands (Hz).
const LOW_MID_HZ: f64 = 200.0;
const MID_HIGH_HZ: f64 = 2_000.0;
/// Frames read from the track at a time.
const BLOCK_FRAMES: usize = 65_536;

/// Waveform summary of a track. All values are scaled to 0..=255.
#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    pub slices_per_second: u32,
    pub peak: Vec<u8>,
    pub low: Vec<u8>,
    pub mid: Vec<u8>,
    pub high: Vec<u8>,
}

impl Waveform {
    pub fn len(&self) -> usize {
        self.peak.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peak.is_empty()
    }
}

/// Two cascaded one-pole low-pass filters (12 dB/octave).
struct LowPass {
    a: f64,
    s1: f64,
    s2: f64,
}

impl LowPass {
    fn new(cutoff_hz: f64, sample_rate: f64) -> Self {
        Self { a: 1.0 - (-TAU * cutoff_hz / sample_rate).exp(), s1: 0.0, s2: 0.0 }
    }
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        self.s1 += self.a * (x - self.s1);
        self.s2 += self.a * (self.s1 - self.s2);
        self.s2
    }
}

/// Computes the waveform summary of `track` with `slices_per_second` slices per second.
pub fn compute_waveform(track: &TrackAudio, slices_per_second: u32) -> Waveform {
    let sr = track.sample_rate() as f64;
    let per_slice = (sr / slices_per_second as f64).max(1.0);
    let slices = (track.frames() as f64 / per_slice).ceil() as usize;
    let mut peak = vec![0.0f64; slices];
    let mut bands = vec![[0.0f64; 3]; slices];
    let mut counts = vec![0u32; slices];

    let mut low_pass = LowPass::new(LOW_MID_HZ, sr);
    let mut mid_pass = LowPass::new(MID_HIGH_HZ, sr);
    let mut i = 0usize;
    track.for_each_block(BLOCK_FRAMES, |block| {
        for frame in block.chunks_exact(2) {
            let x = (frame[0] as f64 + frame[1] as f64) * 0.5;
            let low = low_pass.process(x);
            let below_high = mid_pass.process(x);
            let mid = below_high - low;
            let high = x - below_high;
            let s = ((i as f64 / per_slice) as usize).min(slices - 1);
            peak[s] = peak[s].max(x.abs());
            bands[s][0] += low * low;
            bands[s][1] += mid * mid;
            bands[s][2] += high * high;
            counts[s] += 1;
            i += 1;
        }
    });

    let rms: Vec<[f64; 3]> = bands
        .iter()
        .zip(&counts)
        .map(|(b, &n)| {
            let n = n.max(1) as f64;
            [(b[0] / n).sqrt(), (b[1] / n).sqrt(), (b[2] / n).sqrt()]
        })
        .collect();
    let max_peak = peak.iter().cloned().fold(0.0, f64::max).max(1e-9);
    let max_band = rms.iter().flat_map(|b| b.iter().cloned()).fold(0.0, f64::max).max(1e-9);
    let scale = |v: f64, max: f64| ((v / max).clamp(0.0, 1.0) * 255.0).round() as u8;
    Waveform {
        slices_per_second,
        peak: peak.iter().map(|&p| scale(p, max_peak)).collect(),
        low: rms.iter().map(|b| scale(b[0], max_band)).collect(),
        mid: rms.iter().map(|b| scale(b[1], max_band)).collect(),
        high: rms.iter().map(|b| scale(b[2], max_band)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, seconds: f64) -> TrackAudio {
        let sr = 44_100.0;
        let n = (sr * seconds) as usize;
        let samples = (0..n).flat_map(|i| {
            let v = (TAU * freq * i as f64 / sr).sin() as f32 * 0.5;
            [v, v]
        });
        TrackAudio::from_interleaved(44_100, samples.collect())
    }

    #[test]
    fn slice_count_follows_the_duration() {
        let w = compute_waveform(&sine(440.0, 2.0), 100);
        assert_eq!(w.len(), 200);
        assert_eq!(w.peak.iter().max(), Some(&255));
    }

    #[test]
    fn bands_reflect_the_frequency() {
        let dominant = |f: f64| {
            let w = compute_waveform(&sine(f, 1.0), 50);
            let i = w.len() / 2;
            let b = [w.low[i], w.mid[i], w.high[i]];
            (0..3).max_by_key(|&k| b[k]).unwrap()
        };
        assert_eq!(dominant(60.0), 0, "bass");
        assert_eq!(dominant(700.0), 1, "mids");
        assert_eq!(dominant(8_000.0), 2, "highs");
    }
}
