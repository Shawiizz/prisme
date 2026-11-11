//! The stems of a whole track: STFT, the network's masks, inverse STFT, chunk by chunk.

use std::ops::Range;

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

    /// The stems of a stereo track at [`Self::sample_rate`], each as left and right: every
    /// chunk of [`Self::begin`] from the start.
    ///
    /// `progress(done, total)` is called after each chunk: returning `false` cancels.
    pub fn separate(
        &mut self,
        left: &[f32],
        right: &[f32],
        mut progress: impl FnMut(usize, usize) -> bool,
    ) -> Result<Vec<[Vec<f32>; 2]>, StemsError> {
        let mut separation = self.begin(left.len().min(right.len()), 0);
        while self.step(&mut separation, left, right)? {
            if !progress(separation.done(), separation.chunks()) {
                return Err(StemsError::Cancelled);
            }
        }
        Ok(separation.into_stems())
    }

    /// A separation of a track of `len` samples, to be worked through by [`Self::step`]. The
    /// track is cut in chunks the size the network was made for, `overlap` of them over each
    /// sample ([`crate::Config::overlap`]), the last one ending with the track; where chunks
    /// overlap they are blended with Hamming weights. The chunks over sample `anchor` come
    /// first, then those after it, then those before it.
    pub fn begin(&self, len: usize, anchor: usize) -> Separation {
        let config = &self.net.geometry().config;
        let chunk = config.chunk_samples();
        let starts = chunk_starts(len, chunk, chunk / config.overlap);
        let order = order(&starts, chunk, anchor.min(len.saturating_sub(1)));
        Separation {
            len,
            chunk,
            window: hamming(chunk),
            sum: vec![[vec![0.0; len], vec![0.0; len]]; self.stems().len()],
            weight: vec![0.0; len],
            starts,
            order,
            done: 0,
            running: None,
        }
    }

    /// Separates the next chunk of `separation`, a separation of the stereo track `left`,
    /// `right` begun by this separator, which separates nothing else until it is through.
    /// The network works on the chunk after it meanwhile, and the STFTs are worked out while
    /// it runs. `false` once every chunk is through.
    pub fn step(
        &mut self,
        separation: &mut Separation,
        left: &[f32],
        right: &[f32],
    ) -> Result<bool, StemsError> {
        let Some(&index) = separation.order.get(separation.done) else { return Ok(false) };
        let current = match separation.running.take() {
            Some(running) => running,
            None => {
                let analysis = self.analyse_padded(separation.part(index, left, right), separation.chunk);
                self.net.start(&analysis.bins, analysis.frames)?;
                analysis
            }
        };
        let next = separation
            .order
            .get(separation.done + 1)
            .map(|&next| self.analyse_padded(separation.part(next, left, right), separation.chunk));
        let mut masks = vec![0.0; self.net.geometry().mask_len(current.frames)];
        self.net.finish(&mut masks)?;
        if let Some(next) = &next {
            self.net.start(&next.bins, next.frames)?;
        }
        let mut out = vec![[vec![0.0; separation.chunk], vec![0.0; separation.chunk]]; self.stems().len()];
        self.synthesise(&current, &masks, &mut out);
        separation.add(index, &out);
        separation.running = next;
        Ok(true)
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

/// A track being separated chunk by chunk ([`Separator::begin`]). The stems are final where
/// every chunk over them is through: [`Self::finished`], a span that grows at either end as
/// the chunks come in order.
pub struct Separation {
    len: usize,
    chunk: usize,
    window: Vec<f32>,
    /// Per stem, left and right: the weighted sum of the chunks through over each sample.
    sum: Vec<[Vec<f32>; 2]>,
    weight: Vec<f32>,
    starts: Vec<usize>,
    /// The chunks (indices into `starts`) in the order they are separated.
    order: Vec<usize>,
    /// How many of `order` are through.
    done: usize,
    /// The analysis of the chunk the network is working on: `order[done]`.
    running: Option<Analysis>,
}

impl Separation {
    pub fn chunks(&self) -> usize {
        self.order.len()
    }

    /// How many chunks are through.
    pub fn done(&self) -> usize {
        self.done
    }

    pub fn is_done(&self) -> bool {
        self.done == self.order.len()
    }

    /// The samples whose stems are final: those no chunk still to come lies over.
    pub fn finished(&self) -> Range<usize> {
        let through = &self.order[..self.done];
        let (Some(&first), Some(&last)) = (through.iter().min(), through.iter().max()) else {
            return 0..0;
        };
        let start = first.checked_sub(1).map_or(0, |before| self.starts[before] + self.chunk);
        let end = self.starts.get(last + 1).copied().unwrap_or(self.len);
        start.min(end)..end
    }

    /// Stem `stem` over `range` (final samples), left and right.
    pub fn read(&self, stem: usize, range: Range<usize>) -> [Vec<f32>; 2] {
        let weight = &self.weight[range.clone()];
        self.sum[stem]
            .each_ref()
            .map(|channel| channel[range.clone()].iter().zip(weight).map(|(v, w)| v / w.max(1e-10)).collect())
    }

    /// The stems, each as left and right, once every chunk is through.
    pub fn into_stems(mut self) -> Vec<[Vec<f32>; 2]> {
        for s in self.sum.iter_mut().flatten() {
            s.iter_mut().zip(&self.weight).for_each(|(v, w)| *v /= w.max(1e-10));
        }
        self.sum
    }

    /// Chunk `index` of the stereo track, which may end before a whole chunk.
    fn part<'a>(&self, index: usize, left: &'a [f32], right: &'a [f32]) -> [&'a [f32]; 2] {
        let start = self.starts[index];
        let end = (start + self.chunk).min(self.len);
        [&left[start..end], &right[start..end]]
    }

    /// Adds the stems of chunk `index`, weighted.
    fn add(&mut self, index: usize, out: &[[Vec<f32>; 2]]) {
        let start = self.starts[index];
        let n = self.chunk.min(self.len - start);
        for (s, o) in self.sum.iter_mut().flatten().zip(out.iter().flatten()) {
            for ((s, o), w) in s[start..start + n].iter_mut().zip(o).zip(&self.window) {
                *s += o * w;
            }
        }
        self.weight[start..start + n].iter_mut().zip(&self.window).for_each(|(a, w)| *a += w);
        self.done += 1;
    }
}

/// A chunk's STFT (left and right) and the bins the network takes.
struct Analysis {
    spec: [Vec<Complex32>; 2],
    bins: Vec<f32>,
    frames: usize,
}

/// The chunks over `anchor`, then those after them, then those before them backwards: the
/// chunks through always follow each other, so the final samples make one span.
fn order(starts: &[usize], chunk: usize, anchor: usize) -> Vec<usize> {
    let first = starts.iter().position(|&s| anchor < s + chunk).unwrap_or(starts.len() - 1);
    (first..starts.len()).chain((0..first).rev()).collect()
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

    #[test]
    fn the_chunks_over_the_anchor_come_first_then_those_after_then_those_before() {
        let starts = [0, 5, 10, 15, 20];
        assert_eq!(order(&starts, 10, 0), [0, 1, 2, 3, 4]);
        assert_eq!(order(&starts, 10, 12), [1, 2, 3, 4, 0]);
        assert_eq!(order(&starts, 10, 29), [4, 3, 2, 1, 0]);
    }

    #[test]
    fn the_final_samples_grow_from_the_anchor() {
        let starts = vec![0, 5, 10, 15];
        let mut separation = Separation {
            len: 25,
            chunk: 10,
            window: vec![1.0; 10],
            sum: vec![[vec![0.0; 25], vec![0.0; 25]]],
            weight: vec![0.0; 25],
            order: order(&starts, 10, 12),
            starts,
            done: 0,
            running: None,
        };
        assert_eq!(separation.order, [1, 2, 3, 0]);
        let out = [[vec![1.0; 10], vec![1.0; 10]]];
        let mut finished = vec![separation.finished()];
        while let Some(&index) = separation.order.get(separation.done) {
            separation.add(index, &out);
            finished.push(separation.finished());
        }
        assert_eq!(finished, [0..0, 10..10, 10..15, 10..25, 0..25]);
    }
}
