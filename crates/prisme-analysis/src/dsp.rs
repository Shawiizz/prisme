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

/// Mixes `track` down to mono and decimates it to about [`ANALYSIS_RATE`] (see
/// [`Decimator`]).
pub fn analysis_signal(track: &TrackAudio) -> MonoSignal {
    let mut decimator = Decimator::new(track.sample_rate());
    track.for_each_block(65_536, |block| decimator.push(block));
    decimator.finish()
}

/// Streaming mono mixdown and decimation by an integer factor to about
/// [`ANALYSIS_RATE`], with a windowed-sinc anti-alias filter. Output sample `i` is
/// aligned with input frame `i * factor`; the signal is taken as silent outside.
pub struct Decimator {
    factor: usize,
    taps: Vec<f32>,
    /// Mono input not yet fully consumed; `pending[0]` is input frame `base`.
    pending: Vec<f32>,
    base: isize,
    input_frames: usize,
    output: Vec<f32>,
    sample_rate: f64,
}

impl Decimator {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f64;
        let factor = ((sr / ANALYSIS_RATE).floor() as usize).max(1);
        // pass band up to 0.8 x the new Nyquist, stop band (-74 dB) before what would
        // fold back below ~4.5 kHz
        let taps = if factor == 1 { vec![1.0] } else { lowpass_taps(0.4 / factor as f64, 16 * factor + 1) };
        let half = taps.len() / 2;
        Self {
            factor,
            // silence before the start
            pending: vec![0.0; half],
            base: -(half as isize),
            taps,
            input_frames: 0,
            output: Vec::new(),
            sample_rate: sr / factor as f64,
        }
    }

    /// Adds interleaved stereo samples.
    pub fn push(&mut self, stereo: &[f32]) {
        self.pending.extend(stereo.chunks_exact(2).map(|f| (f[0] + f[1]) * 0.5));
        self.input_frames += stereo.len() / 2;
        self.drain(false);
    }

    pub fn finish(mut self) -> MonoSignal {
        // silence after the end
        self.pending.extend(std::iter::repeat_n(0.0, self.taps.len()));
        self.drain(true);
        MonoSignal { samples: self.output, sample_rate: self.sample_rate }
    }

    /// Computes every output whose filter window is available.
    fn drain(&mut self, last: bool) {
        let half = (self.taps.len() / 2) as isize;
        let total_outputs = self.input_frames.div_ceil(self.factor);
        loop {
            let o = self.output.len();
            if last && o >= total_outputs {
                break;
            }
            let start = (o * self.factor) as isize - half - self.base;
            let end = start as usize + self.taps.len();
            if end > self.pending.len() {
                break;
            }
            self.output.push(dot(&self.pending[start as usize..end], &self.taps));
        }
        // drop what no future output needs
        let next_start = (self.output.len() * self.factor) as isize - half - self.base;
        if next_start > 65_536 {
            self.pending.drain(..next_start as usize);
            self.base += next_start;
        }
    }
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
            for ((b, x), w) in self.buffer.iter_mut().zip(&signal[start..start + self.size]).zip(&self.window)
            {
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
    fn streaming_gives_the_same_signal_whatever_the_chunks() {
        let samples: Vec<f32> = (0..100_003).map(|i| ((i * 7919) % 2001) as f32 / 1000.0 - 1.0).collect();
        let samples = &samples[..samples.len() / 2 * 2];
        let whole = analysis_signal(&TrackAudio::from_interleaved(44_100, samples.to_vec()));
        assert_eq!(whole.samples.len(), (samples.len() / 2).div_ceil(4));

        let mut d = Decimator::new(44_100);
        let mut rest = samples;
        let mut size = 2;
        while !rest.is_empty() {
            let n = size.min(rest.len());
            d.push(&rest[..n]);
            rest = &rest[n..];
            size = (size * 3 + 2) % 4000 / 2 * 2 + 2;
        }
        assert_eq!(d.finish().samples, whole.samples);
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
