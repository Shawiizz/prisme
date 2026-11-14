//! The stems of a whole track: STFT, the network's masks, inverse STFT, chunk by chunk.

use std::ops::Range;

use realfft::num_complex::Complex32;

use crate::network::MaskNetwork;
use crate::stft::Stft;
use crate::StemsError;

/// How far ahead of the frame played the stems are separated before anything else (s).
pub const LEAD_S: f64 = 20.0;

pub struct Separator<N: MaskNetwork> {
    net: N,
    stft: Stft,
    /// How many chunks cover each sample.
    overlap: usize,
}

impl<N: MaskNetwork> Separator<N> {
    pub fn new(net: N) -> Self {
        let c = &net.geometry().config;
        let stft = Stft::new(c.n_fft, c.hop);
        let overlap = c.overlap;
        Self { net, stft, overlap }
    }

    /// With at most `overlap` chunks over each sample, when the model asks for more: that
    /// many times fewer chunks to work through, for a little less of the quality (a
    /// difference of -25 to -35 dB on drums and bass from 4 to 2).
    pub fn overlapping_at_most(mut self, overlap: usize) -> Self {
        self.overlap = self.overlap.min(overlap.max(1));
        self
    }

    /// The network separating.
    pub fn network(&self) -> &N {
        &self.net
    }

    pub fn sample_rate(&self) -> u32 {
        self.net.geometry().config.sample_rate
    }

    /// How many chunks [`Self::separate`] cuts a track of `len` samples in.
    pub fn chunks(&self, len: usize) -> usize {
        let chunk = self.net.geometry().config.chunk_samples();
        chunk_starts(len, chunk, chunk / self.overlap).len()
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
        while self.step(&mut separation, left, right, || None)? {
            if !progress(separation.done(), separation.chunks()) {
                return Err(StemsError::Cancelled);
            }
        }
        Ok(separation.into_stems())
    }

    /// A separation of a track of `len` samples, to be worked through by [`Self::step`]. The
    /// track is cut in chunks the size the network was made for, as many over each sample as
    /// the model asks ([`crate::Config::overlap`], or [`Self::overlapping_at_most`]), the
    /// last one ending with the track; where chunks overlap they are blended with Hamming
    /// weights. Unless a frame played says otherwise ([`Self::step`]), the chunks come from
    /// the one over sample `anchor` to the end, then from the start.
    pub fn begin(&self, len: usize, anchor: usize) -> Separation {
        let chunk = self.net.geometry().config.chunk_samples();
        let starts = chunk_starts(len, chunk, chunk / self.overlap);
        let first = starts.iter().position(|&s| anchor < s + chunk).unwrap_or(starts.len() - 1);
        Separation {
            len,
            chunk,
            window: hamming(chunk),
            sum: vec![[vec![0.0; len], vec![0.0; len]]; self.stems().len()],
            weight: vec![0.0; len],
            through: vec![false; starts.len()],
            fill_from: starts[first],
            lead: (LEAD_S * f64::from(self.sample_rate())) as usize,
            starts,
            done: 0,
            running: None,
        }
    }

    /// Separates the next chunk of `separation`, a separation of the stereo track `left`,
    /// `right` begun by this separator, which separates nothing else until it is through:
    /// the chunks over the sample `at` tells and [`LEAD_S`] after it first, where the track
    /// is played, else the next ones of [`Separation::fill_from`]. The network works on the
    /// chunk after it meanwhile, and the STFTs are worked out while it runs. `false` once
    /// every chunk is through.
    pub fn step(
        &mut self,
        separation: &mut Separation,
        left: &[f32],
        right: &[f32],
        at: impl Fn() -> Option<usize>,
    ) -> Result<bool, StemsError> {
        let (index, current) = match separation.running.take() {
            Some(running) => running,
            None => {
                let Some(index) = separation.next(at(), None) else { return Ok(false) };
                let analysis = self.analyse_padded(separation.part(index, left, right), separation.chunk);
                self.net.start(&analysis.bins, analysis.frames)?;
                (index, analysis)
            }
        };
        let guessed = separation
            .next(at(), Some(index))
            .map(|next| (next, self.analysis(separation, next, left, right)));
        let mut masks = vec![0.0; self.net.geometry().mask_len(current.frames)];
        self.net.finish(&mut masks)?;
        // the deck may have jumped while the network worked: the chunk where it plays now
        let next = match (separation.next(at(), Some(index)), guessed) {
            (Some(wanted), Some(guessed)) if wanted == guessed.0 => Some(guessed),
            (wanted, _) => wanted.map(|next| (next, self.analysis(separation, next, left, right))),
        };
        if let Some((_, next)) = &next {
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

    /// The analysis of chunk `index` of `separation`.
    fn analysis(&self, separation: &Separation, index: usize, left: &[f32], right: &[f32]) -> Analysis {
        self.analyse_padded(separation.part(index, left, right), separation.chunk)
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
/// every chunk over them is through: [`Self::finished`], islands that grow and meet.
pub struct Separation {
    len: usize,
    chunk: usize,
    window: Vec<f32>,
    /// Per stem, left and right: the weighted sum of the chunks through over each sample.
    sum: Vec<[Vec<f32>; 2]>,
    weight: Vec<f32>,
    starts: Vec<usize>,
    /// Which chunks (by their index into `starts`) are through.
    through: Vec<bool>,
    done: usize,
    /// Where the chunks not wanted where the track is played go on from: the start of the
    /// last of them, so that they resume where they stopped.
    fill_from: usize,
    /// How far after the sample played the chunks come first ([`LEAD_S`], in samples).
    lead: usize,
    /// The chunk the network is working on, and its analysis.
    running: Option<(usize, Analysis)>,
}

impl Separation {
    pub fn chunks(&self) -> usize {
        self.starts.len()
    }

    /// How many chunks are through.
    pub fn done(&self) -> usize {
        self.done
    }

    pub fn is_done(&self) -> bool {
        self.done == self.starts.len()
    }

    /// The samples whose stems are final, those no chunk still to come lies over: islands in
    /// order, apart.
    pub fn finished(&self) -> Vec<Range<usize>> {
        let mut bounds: Vec<usize> =
            self.starts.iter().flat_map(|&s| [s, self.end(s)]).chain([0, self.len]).collect();
        bounds.sort_unstable();
        bounds.dedup();
        let mut islands: Vec<Range<usize>> = Vec::new();
        for pair in bounds.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let over: Vec<usize> = (0..self.starts.len())
                .filter(|&i| self.starts[i] <= from && from < self.end(self.starts[i]))
                .collect();
            if over.is_empty() || !over.iter().all(|&i| self.through[i]) {
                continue;
            }
            match islands.last_mut().filter(|last| last.end == from) {
                Some(last) => last.end = to,
                None => islands.push(from..to),
            }
        }
        islands
    }

    /// The end of the chunk starting at `start`.
    fn end(&self, start: usize) -> usize {
        (start + self.chunk).min(self.len)
    }

    /// The next chunk to separate, not `busy` (the network has it already): the first one not
    /// through over sample `at` and the [`LEAD_S`] after it, else the first one from
    /// [`Self::fill_from`] on, then from the start.
    fn next(&mut self, at: Option<usize>, busy: Option<usize>) -> Option<usize> {
        let mut open = (0..self.starts.len()).filter(|&i| !self.through[i] && Some(i) != busy);
        if let Some(at) = at.filter(|&at| at < self.len) {
            let wanted = at..at.saturating_add(self.lead).min(self.len);
            let over = |&i: &usize| self.starts[i] < wanted.end && self.end(self.starts[i]) > wanted.start;
            if let Some(i) = open.clone().find(over) {
                return Some(i);
            }
        }
        let i = open.clone().find(|&i| self.starts[i] >= self.fill_from).or_else(|| open.next())?;
        self.fill_from = self.starts[i];
        Some(i)
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
        self.through[index] = true;
        self.done += 1;
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

    /// A separation of 50 samples in chunks of 10 overlapping by half, leading by 15.
    fn separation(anchor: usize) -> Separation {
        let starts = chunk_starts(50, 10, 5);
        let first = starts.iter().position(|&s| anchor < s + 10).unwrap_or(starts.len() - 1);
        Separation {
            len: 50,
            chunk: 10,
            window: vec![1.0; 10],
            sum: vec![[vec![0.0; 50], vec![0.0; 50]]],
            weight: vec![0.0; 50],
            through: vec![false; starts.len()],
            fill_from: starts[first],
            lead: 15,
            starts,
            done: 0,
            running: None,
        }
    }

    /// Separates the chunk [`Separation::next`] picks for a track played at `at`.
    fn take(separation: &mut Separation, at: Option<usize>) -> Option<usize> {
        let index = separation.next(at, None)?;
        separation.add(index, &[[vec![1.0; 10], vec![1.0; 10]]]);
        Some(index)
    }

    #[test]
    fn the_chunks_where_the_track_plays_come_first_then_the_rest_from_where_it_stopped() {
        let mut s = separation(0);
        assert_eq!(s.starts, [0, 5, 10, 15, 20, 25, 30, 35, 40]);
        // played from 12: the chunks over 12 to 27 first
        let first: Vec<_> = (0..5).map_while(|_| take(&mut s, Some(12))).collect();
        assert_eq!(first, [1, 2, 3, 4, 5]);
        // the lead is there: the rest from the start, where nothing was filled yet
        assert_eq!(take(&mut s, Some(12)), Some(0));
        assert_eq!(take(&mut s, Some(12)), Some(6));
        // the play jumps: the chunks over the new place, then the filling resumes
        assert_eq!(take(&mut s, Some(42)), Some(7));
        assert_eq!(take(&mut s, Some(42)), Some(8));
        assert_eq!(take(&mut s, None), None);
        assert!(s.is_done());
    }

    #[test]
    fn not_played_the_chunks_go_from_the_anchor_to_the_end_then_from_the_start() {
        let mut s = separation(23);
        let order: Vec<_> = std::iter::from_fn(|| take(&mut s, None)).collect();
        assert_eq!(order, [3, 4, 5, 6, 7, 8, 0, 1, 2]);
    }

    #[test]
    fn the_busy_chunk_is_not_picked_twice() {
        let mut s = separation(0);
        assert_eq!(s.next(Some(0), None), Some(0));
        assert_eq!(s.next(Some(0), Some(0)), Some(1));
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn the_final_samples_are_islands_that_meet() {
        let mut s = separation(0);
        assert!(s.finished().is_empty());
        // chunk 0 alone: only what no other chunk lies over
        s.add(0, &[[vec![1.0; 10], vec![1.0; 10]]]);
        assert_eq!(s.finished(), [0..5]);
        s.add(1, &[[vec![1.0; 10], vec![1.0; 10]]]);
        assert_eq!(s.finished(), [0..10]);
        // an island further on
        for i in [6, 7] {
            s.add(i, &[[vec![1.0; 10], vec![1.0; 10]]]);
        }
        assert_eq!(s.finished(), [0..10, 35..40]);
        for i in [2, 3, 4, 5, 8] {
            s.add(i, &[[vec![1.0; 10], vec![1.0; 10]]]);
        }
        assert_eq!(s.finished(), [0..50]);
        assert_eq!(s.read(0, 0..50), [vec![1.0; 50], vec![1.0; 50]], "the overlaps are blended back");
    }
}
