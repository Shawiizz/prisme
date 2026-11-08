//! Vocals of a whole track: STFT, the network's masks, inverse STFT, chunk by chunk.

use realfft::num_complex::Complex32;

use crate::network::MaskNetwork;
use crate::stft::Stft;
use crate::StemsError;

pub struct Separator<N: MaskNetwork> {
    net: N,
    stft: Stft,
}

impl<N: MaskNetwork> Separator<N> {
    pub fn new(net: N) -> Self {
        let c = &net.geometry().config;
        let stft = Stft::new(c.n_fft, c.hop);
        Self { net, stft }
    }

    pub fn sample_rate(&self) -> u32 {
        self.net.geometry().config.sample_rate
    }

    /// The vocals of a stereo track at [`Self::sample_rate`]. The track is cut in chunks the
    /// size the network was made for, the last one ending with the track; where two chunks
    /// overlap they are blended with Hamming weights.
    ///
    /// `progress(done, total)` is called after each chunk: returning `false` cancels.
    pub fn vocals(
        &mut self,
        left: &[f32],
        right: &[f32],
        mut progress: impl FnMut(usize, usize) -> bool,
    ) -> Result<[Vec<f32>; 2], StemsError> {
        let len = left.len().min(right.len());
        let chunk = self.net.geometry().config.chunk_samples();
        let window = hamming(chunk);
        let starts = chunk_starts(len, chunk);
        let mut sum = [vec![0.0f32; len], vec![0.0f32; len]];
        let mut weight = vec![0.0f32; len];
        let mut part = [vec![0.0; chunk], vec![0.0; chunk]];
        let mut out = [vec![0.0; chunk], vec![0.0; chunk]];
        for (done, &start) in starts.iter().enumerate() {
            let n = chunk.min(len - start);
            for (p, src) in part.iter_mut().zip([left, right]) {
                p[..n].copy_from_slice(&src[start..start + n]);
                p[n..].fill(0.0);
            }
            let [l, r] = &mut out;
            self.chunk_vocals(&part[0], &part[1], l, r)?;
            for (s, o) in sum.iter_mut().zip(&out) {
                for ((s, o), w) in s[start..start + n].iter_mut().zip(o).zip(&window) {
                    *s += o * w;
                }
            }
            weight[start..start + n].iter_mut().zip(&window).for_each(|(a, w)| *a += w);
            if !progress(done + 1, starts.len()) {
                return Err(StemsError::Cancelled);
            }
        }
        for s in &mut sum {
            s.iter_mut().zip(&weight).for_each(|(v, w)| *v /= w.max(1e-10));
        }
        Ok(sum)
    }

    /// The vocals of one chunk (at most the network's chunk size).
    pub fn chunk_vocals(
        &mut self,
        left: &[f32],
        right: &[f32],
        out_left: &mut [f32],
        out_right: &mut [f32],
    ) -> Result<(), StemsError> {
        let stft = &self.stft;
        let frames = stft.frames(left.len());
        let bins = stft.bins();
        let mut spec = [vec![Complex32::default(); frames * bins], vec![Complex32::default(); frames * bins]];
        stft.forward(left, &mut spec[0]);
        stft.forward(right, &mut spec[1]);

        let w = self.net.geometry();
        let features = w.features();
        let source = |index: u32| (index as usize % 2, index as usize / 2);
        let mut gathered = vec![0.0; frames * features];
        for (t, row) in gathered.chunks_exact_mut(features).enumerate() {
            for (pair, &index) in row.chunks_exact_mut(2).zip(&w.freq_indices) {
                let (ch, bin) = source(index);
                let c = spec[ch][t * bins + bin];
                pair.copy_from_slice(&[c.re, c.im]);
            }
        }
        let mut masks = vec![0.0; frames * features];
        self.net.masks(&gathered, frames, &mut masks)?;

        let w = self.net.geometry();
        let mut summed =
            [vec![Complex32::default(); frames * bins], vec![Complex32::default(); frames * bins]];
        for (t, row) in masks.chunks_exact(features).enumerate() {
            for (pair, &index) in row.chunks_exact(2).zip(&w.freq_indices) {
                let (ch, bin) = source(index);
                summed[ch][t * bins + bin] += Complex32::new(pair[0], pair[1]);
            }
        }
        for (s, m) in spec.iter_mut().zip(&summed) {
            for (i, (s, m)) in s.iter_mut().zip(m).enumerate() {
                *s *= m / w.bands_per_freq[i % bins].max(1) as f32;
            }
        }
        for (s, out) in spec.iter().zip([out_left, out_right]) {
            let n = stft.samples(frames).min(out.len());
            stft.inverse(s, &mut out[..n]);
            out[n..].fill(0.0);
        }
        Ok(())
    }
}

/// Where the chunks start: back to back, the last one moved back to end with the track.
fn chunk_starts(len: usize, chunk: usize) -> Vec<usize> {
    let mut starts: Vec<usize> = (0..len.saturating_sub(chunk)).step_by(chunk).collect();
    let tail = len.saturating_sub(chunk);
    if starts.last() != Some(&tail) {
        starts.push(tail);
    }
    starts
}

/// The symmetric Hamming window (`scipy.signal.windows.hamming`).
fn hamming(len: usize) -> Vec<f32> {
    let m = (len.max(2) - 1) as f64;
    (0..len).map(|i| (0.54 - 0.46 * (2.0 * std::f64::consts::PI * i as f64 / m).cos()) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_cover_the_track_once_except_the_tail() {
        assert_eq!(chunk_starts(25, 10), vec![0, 10, 15]);
        assert_eq!(chunk_starts(20, 10), vec![0, 10]);
        assert_eq!(chunk_starts(7, 10), vec![0]);
        assert_eq!(chunk_starts(10, 10), vec![0]);
    }
}
