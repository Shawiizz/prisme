//! Signal helpers shared by the analysers: mono mixdown with decimation, and a
//! short-time Fourier transform.

use prisme_media::TrackAudio;
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::f64::consts::PI;
use std::sync::Arc;

/// Rate the analysers work at: enough for onsets and for pitch up to a few kHz.
const ANALYSIS_RATE: f64 = 11_025.0;

/// A mono signal.
pub struct MonoSignal {
    pub samples: Vec<f32>,
    pub sample_rate: f64,
}

impl MonoSignal {
    pub fn duration_s(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate
    }
}

/// Mixes `track` down to mono and decimates it by an integer factor to about
/// [`ANALYSIS_RATE`], with a windowed-sinc anti-alias filter. Output sample `i` is
/// aligned with input frame `i * factor`.
pub fn analysis_signal(track: &TrackAudio) -> MonoSignal {
    let sr = track.sample_rate() as f64;
    let factor = ((sr / ANALYSIS_RATE).floor() as usize).max(1);
    let mono: Vec<f32> = track.samples().chunks_exact(2).map(|f| (f[0] + f[1]) * 0.5).collect();
    if factor == 1 {
        return MonoSignal { samples: mono, sample_rate: sr };
    }

    // pass band up to 0.8 x the new Nyquist, stop band (-74 dB) before what would fold
    // back below ~4.5 kHz
    let taps = lowpass_taps(0.4 / factor as f64, 16 * factor + 1);
    let half = taps.len() / 2;
    let samples = (0..mono.len().div_ceil(factor))
        .map(|o| {
            let start = (o * factor) as isize - half as isize;
            if start >= 0 && start as usize + taps.len() <= mono.len() {
                dot(&mono[start as usize..start as usize + taps.len()], &taps)
            } else {
                // edges: skip the taps that fall outside the signal
                taps.iter()
                    .enumerate()
                    .filter_map(|(j, t)| mono.get(usize::try_from(start + j as isize).ok()?).map(|x| x * t))
                    .sum()
            }
        })
        .collect();
    MonoSignal { samples, sample_rate: sr / factor as f64 }
}

/// Dot product with 8 independent accumulators, so the compiler can vectorize it.
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0.0f32; 8];
    let (a8, b8) = (a.chunks_exact(8), b.chunks_exact(8));
    let tail: f32 = a8.remainder().iter().zip(b8.remainder()).map(|(x, y)| x * y).sum();
    for (x, y) in a8.zip(b8) {
        for i in 0..8 {
            acc[i] += x[i] * y[i];
        }
    }
    acc.iter().sum::<f32>() + tail
}

/// Blackman-windowed sinc low-pass, `cutoff` in cycles per sample, unity DC gain.
fn lowpass_taps(cutoff: f64, len: usize) -> Vec<f32> {
    let m = (len - 1) as f64;
    let raw: Vec<f64> = (0..len)
        .map(|i| {
            let x = i as f64 - m / 2.0;
            let sinc = if x == 0.0 { 2.0 * cutoff } else { (2.0 * PI * cutoff * x).sin() / (PI * x) };
            let w = 0.42 - 0.5 * (2.0 * PI * i as f64 / m).cos() + 0.08 * (4.0 * PI * i as f64 / m).cos();
            sinc * w
        })
        .collect();
    let sum: f64 = raw.iter().sum();
    raw.iter().map(|v| (v / sum) as f32).collect()
}

/// Short-time Fourier transform with a Hann window. Magnitudes are scaled so that a
/// full-scale sine reads about 1.0 in its bin.
pub struct Stft {
    size: usize,
    hop: usize,
    window: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
    buffer: Vec<Complex32>,
    scratch: Vec<Complex32>,
    magnitudes: Vec<f32>,
}

impl Stft {
    pub fn new(size: usize, hop: usize) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(size);
        let window: Vec<f32> =
            (0..size).map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / size as f64).cos()) as f32).collect();
        let gain = 2.0 / window.iter().sum::<f32>();
        Self {
            size,
            hop,
            window: window.iter().map(|w| w * gain).collect(),
            scratch: vec![Complex32::default(); fft.get_inplace_scratch_len()],
            buffer: vec![Complex32::default(); size],
            magnitudes: vec![0.0; size / 2 + 1],
            fft,
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn hop(&self) -> usize {
        self.hop
    }

    pub fn frame_count(&self, len: usize) -> usize {
        if len < self.size {
            0
        } else {
            (len - self.size) / self.hop + 1
        }
    }

    /// Calls `f(frame, magnitudes)` for each full frame; `magnitudes` holds bins
    /// `0..=size / 2`.
    pub fn for_each_frame(&mut self, signal: &[f32], mut f: impl FnMut(usize, &[f32])) {
        for frame in 0..self.frame_count(signal.len()) {
            let start = frame * self.hop;
            for ((b, x), w) in self.buffer.iter_mut().zip(&signal[start..start + self.size]).zip(&self.window) {
                *b = Complex32::new(x * w, 0.0);
            }
            self.fft.process_with_scratch(&mut self.buffer, &mut self.scratch);
            for (m, b) in self.magnitudes.iter_mut().zip(&self.buffer) {
                *m = b.norm();
            }
            f(frame, &self.magnitudes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimation_keeps_the_pitch_and_level() {
        let sr = 44_100u32;
        let samples: Vec<f32> = (0..sr as usize)
            .flat_map(|i| {
                let v = (2.0 * PI * 440.0 * i as f64 / sr as f64).sin() as f32;
                [v, v]
            })
            .collect();
        let mono = analysis_signal(&TrackAudio::from_interleaved(sr, samples));
        assert_eq!(mono.sample_rate, 11_025.0);

        let mut stft = Stft::new(4096, 4096);
        let mut peak = (0, 0.0f32);
        stft.for_each_frame(&mono.samples[2000..], |_, m| {
            for (k, &v) in m.iter().enumerate() {
                if v > peak.1 {
                    peak = (k, v);
                }
            }
        });
        let freq = peak.0 as f64 * mono.sample_rate / 4096.0;
        assert!((freq - 440.0).abs() < 3.0, "peak at {freq} Hz");
        assert!((peak.1 - 1.0).abs() < 0.2, "level {}", peak.1);
    }

    #[test]
    fn high_frequencies_are_removed() {
        let sr = 44_100u32;
        let samples: Vec<f32> = (0..sr as usize)
            .flat_map(|i| {
                let v = (2.0 * PI * 9_000.0 * i as f64 / sr as f64).sin() as f32;
                [v, v]
            })
            .collect();
        let mono = analysis_signal(&TrackAudio::from_interleaved(sr, samples));
        let rms = (mono.samples[100..mono.samples.len() - 100].iter().map(|x| x * x).sum::<f32>()
            / (mono.samples.len() - 200) as f32)
            .sqrt();
        assert!(rms < 0.01, "aliased rms {rms}");
    }
}
