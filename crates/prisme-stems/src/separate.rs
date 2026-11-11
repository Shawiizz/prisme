//! The stems of a whole track: STFT, the network's masks, inverse STFT, chunk by chunk.

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

    /// How many chunks [`Self::separate`] cuts a track of `len` samples in.
    pub fn chunks(&self, len: usize) -> usize {
        let config = &self.net.geometry().config;
        chunk_starts(len, config.chunk_samples(), config.chunk_samples() / config.overlap).len()
    }

    /// The stems the network separates, in the order [`Self::separate`] returns them.
    pub fn stems(&self) -> &[String] {
        &self.net.geometry().stems
    }

    /// The stems of a stereo track at [`Self::sample_rate`], each as left and right. The
    /// track is cut in chunks the size the network was made for, `overlap` of them over each
    /// sample ([`crate::Config::overlap`]), the last one ending with the track; where chunks
    /// overlap they are blended with Hamming weights. Each chunk's STFT and the iSTFT of the
    /// one before are worked out while the network runs.
    ///
    /// `progress(done, total)` is called after each chunk: returning `false` cancels.
    pub fn separate(
        &mut self,
        left: &[f32],
        right: &[f32],
        mut progress: impl FnMut(usize, usize) -> bool,
    ) -> Result<Vec<[Vec<f32>; 2]>, StemsError> {
        let len = left.len().min(right.len());
        let config = &self.net.geometry().config;
        let chunk = config.chunk_samples();
        let starts = chunk_starts(len, chunk, chunk / config.overlap);
        let window = hamming(chunk);
        let stems = self.stems().len();
        let mut sum = vec![[vec![0.0f32; len], vec![0.0f32; len]]; stems];
        let mut weight = vec![0.0f32; len];
        let mut out = vec![[vec![0.0; chunk], vec![0.0; chunk]]; stems];
        let part = |start: usize| {
            let end = (start + chunk).min(len);
            [&left[start..end], &right[start..end]]
        };

        let mut current = self.analyse_padded(part(starts[0]), chunk);
        self.net.start(&current.bins, current.frames)?;
        for (done, &start) in starts.iter().enumerate() {
            let next = starts.get(done + 1).map(|&next| self.analyse_padded(part(next), chunk));
            let mut masks = vec![0.0; self.net.geometry().mask_len(current.frames)];
            self.net.finish(&mut masks)?;
            if let Some(next) = &next {
                self.net.start(&next.bins, next.frames)?;
            }
            self.synthesise(&current, &masks, &mut out);
            let n = chunk.min(len - start);
            for (s, o) in sum.iter_mut().flatten().zip(out.iter().flatten()) {
                for ((s, o), w) in s[start..start + n].iter_mut().zip(o).zip(&window) {
                    *s += o * w;
                }
            }
            weight[start..start + n].iter_mut().zip(&window).for_each(|(a, w)| *a += w);
            if !progress(done + 1, starts.len()) {
                return Err(StemsError::Cancelled);
            }
            if let Some(next) = next {
                current = next;
            }
        }
        for s in sum.iter_mut().flatten() {
            s.iter_mut().zip(&weight).for_each(|(v, w)| *v /= w.max(1e-10));
        }
        Ok(sum)
    }

    /// The stems of one chunk (at most the network's chunk size): `out` holds a left and a
    /// right buffer per stem, as long as the chunk.
    pub fn chunk(
        &mut self,
        left: &[f32],
        right: &[f32],
        out: &mut [[Vec<f32>; 2]],
    ) -> Result<(), StemsError> {
        let analysis = self.analyse(left, right);
        let mut masks = vec![0.0; self.net.geometry().mask_len(analysis.frames)];
        self.net.masks(&analysis.bins, analysis.frames, &mut masks)?;
        self.synthesise(&analysis, &masks, out);
        Ok(())
    }

    /// [`Self::analyse`] of a chunk padded with silence to `len` samples.
    fn analyse_padded(&self, [left, right]: [&[f32]; 2], len: usize) -> Analysis {
        let pad = |part: &[f32]| {
            let mut padded = vec![0.0; len];
            padded[..part.len()].copy_from_slice(part);
            padded
        };
        self.analyse(&pad(left), &pad(right))
    }

    /// The STFT of a chunk, and its bins gathered for the network.
    fn analyse(&self, left: &[f32], right: &[f32]) -> Analysis {
        let stft = &self.stft;
        let frames = stft.frames(left.len());
        let bins = stft.bins();
        let mut spec = [vec![Complex32::default(); frames * bins], vec![Complex32::default(); frames * bins]];
        stft.forward(left, &mut spec[0]);
        stft.forward(right, &mut spec[1]);

        let w = self.net.geometry();
        let features = w.features();
        let mut gathered = vec![0.0; frames * features];
        for (t, row) in gathered.chunks_exact_mut(features).enumerate() {
            for (pair, &index) in row.chunks_exact_mut(2).zip(&w.freq_indices) {
                let (ch, bin) = source(index);
                let c = spec[ch][t * bins + bin];
                pair.copy_from_slice(&[c.re, c.im]);
            }
        }
        Analysis { spec, bins: gathered, frames }
    }

    /// Each stem of a chunk from its masks: the chunk's STFT masked, then inverted.
    fn synthesise(&self, analysis: &Analysis, masks: &[f32], out: &mut [[Vec<f32>; 2]]) {
        let stft = &self.stft;
        let Analysis { spec, frames, .. } = analysis;
        let bins = stft.bins();
        let w = self.net.geometry();
        let features = w.features();
        let mut masked =
            [vec![Complex32::default(); frames * bins], vec![Complex32::default(); frames * bins]];
        for (stem_masks, stem_out) in masks.chunks_exact(frames * features).zip(out) {
            masked.iter_mut().for_each(|m| m.fill(Complex32::default()));
            for (t, row) in stem_masks.chunks_exact(features).enumerate() {
                for (pair, &index) in row.chunks_exact(2).zip(&w.freq_indices) {
                    let (ch, bin) = source(index);
                    masked[ch][t * bins + bin] += Complex32::new(pair[0], pair[1]);
                }
            }
            for (m, s) in masked.iter_mut().zip(spec) {
                for (i, (m, s)) in m.iter_mut().zip(s).enumerate() {
                    *m = s * *m / w.bands_per_freq[i % bins].max(1) as f32;
                }
            }
            for (m, out) in masked.iter().zip(stem_out) {
                let n = stft.samples(*frames).min(out.len());
                stft.inverse(m, &mut out[..n]);
                out[n..].fill(0.0);
            }
        }
    }
}

/// A chunk's STFT (left and right) and the bins the network takes.
struct Analysis {
    spec: [Vec<Complex32>; 2],
    bins: Vec<f32>,
    frames: usize,
}

/// The channel and STFT bin of a gathered bin.
fn source(index: u32) -> (usize, usize) {
    (index as usize % 2, index as usize / 2)
}

/// Where the chunks start: `step` apart, the last one moved back to end with the track.
fn chunk_starts(len: usize, chunk: usize, step: usize) -> Vec<usize> {
    let mut starts: Vec<usize> = (0..len.saturating_sub(chunk)).step_by(step).collect();
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
        assert_eq!(chunk_starts(25, 10, 10), vec![0, 10, 15]);
        assert_eq!(chunk_starts(20, 10, 10), vec![0, 10]);
        assert_eq!(chunk_starts(7, 10, 10), vec![0]);
        assert_eq!(chunk_starts(10, 10, 10), vec![0]);
    }

    #[test]
    fn overlapping_chunks_start_a_step_apart() {
        assert_eq!(chunk_starts(25, 10, 5), vec![0, 5, 10, 15]);
        assert_eq!(chunk_starts(27, 10, 5), vec![0, 5, 10, 15, 17]);
    }
}
