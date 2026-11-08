//! STFT and inverse STFT as `torch.stft` / `torch.istft` compute them for the network: centred
//! frames with reflect padding, a periodic Hann window, no normalisation.

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub struct Stft {
    n_fft: usize,
    hop: usize,
    window: Vec<f32>,
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
}

impl Stft {
    pub fn new(n_fft: usize, hop: usize) -> Self {
        let mut planner = RealFftPlanner::new();
        let window = (0..n_fft)
            .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n_fft as f64).cos()) as f32)
            .collect();
        Self {
            n_fft,
            hop,
            window,
            forward: planner.plan_fft_forward(n_fft),
            inverse: planner.plan_fft_inverse(n_fft),
        }
    }

    pub fn bins(&self) -> usize {
        self.n_fft / 2 + 1
    }

    pub fn frames(&self, samples: usize) -> usize {
        1 + samples / self.hop
    }

    /// Samples the inverse of `frames` frames gives back.
    pub fn samples(&self, frames: usize) -> usize {
        self.hop * (frames - 1)
    }

    /// `frames(signal.len())` rows of `bins()` values. The signal must be longer than half a
    /// window, for the reflect padding.
    pub fn forward(&self, signal: &[f32], out: &mut [Complex32]) {
        let len = signal.len();
        let half = self.n_fft / 2;
        assert!(len > half, "the signal is shorter than half a window");
        let reflect = |i: isize| -> f32 {
            let i = i.unsigned_abs();
            signal[if i < len { i } else { 2 * (len - 1) - i }]
        };
        let mut frame = vec![0.0; self.n_fft];
        let mut scratch = self.forward.make_scratch_vec();
        for (f, row) in out.chunks_exact_mut(self.bins()).take(self.frames(len)).enumerate() {
            let start = (f * self.hop) as isize - half as isize;
            for (i, (v, w)) in frame.iter_mut().zip(&self.window).enumerate() {
                *v = reflect(start + i as isize) * w;
            }
            self.forward.process_with_scratch(&mut frame, row, &mut scratch).expect("sizes match the plan");
        }
    }

    /// Overlap-adds the windowed frames and divides by the summed squared window.
    pub fn inverse(&self, spec: &[Complex32], out: &mut [f32]) {
        let frames = spec.len() / self.bins();
        let total = self.n_fft + self.hop * (frames - 1);
        let mut sum = vec![0.0f32; total];
        let mut envelope = vec![0.0f32; total];
        let mut row = vec![Complex32::default(); self.bins()];
        let mut frame = vec![0.0; self.n_fft];
        let mut scratch = self.inverse.make_scratch_vec();
        let scale = 1.0 / self.n_fft as f32;
        for (f, bins) in spec.chunks_exact(self.bins()).enumerate() {
            row.copy_from_slice(bins);
            row[0].im = 0.0;
            row[self.bins() - 1].im = 0.0;
            self.inverse
                .process_with_scratch(&mut row, &mut frame, &mut scratch)
                .expect("sizes match the plan");
            let at = f * self.hop;
            for (i, (v, w)) in frame.iter().zip(&self.window).enumerate() {
                sum[at + i] += v * scale * w;
                envelope[at + i] += w * w;
            }
        }
        let half = self.n_fft / 2;
        for (i, o) in out.iter_mut().take(self.samples(frames)).enumerate() {
            let e = envelope[half + i];
            *o = if e > 1e-11 { sum[half + i] / e } else { 0.0 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inverse_gives_the_signal_back() {
        let stft = Stft::new(64, 16);
        let signal: Vec<f32> = (0..16 * 30).map(|i| ((i * 7919) % 101) as f32 / 50.0 - 1.0).collect();
        let mut spec = vec![Complex32::default(); stft.frames(signal.len()) * stft.bins()];
        stft.forward(&signal, &mut spec);
        let mut back = vec![0.0; signal.len()];
        stft.inverse(&spec, &mut back);
        let err = signal.iter().zip(&back).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-5, "max error {err}");
    }
}
