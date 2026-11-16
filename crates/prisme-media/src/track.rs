//! A decoded track, playable while it is being decoded.
//!
//! The samples live in a buffer sized from the file header and never reallocated. The
//! decoding thread writes past [`TrackAudio::decoded_frames`] and then publishes the new
//! count (release); readers see only what was published (acquire). Samples are stored as
//! `f32` bits in `AtomicU32`s: plain loads and stores on x86 and ARM, no locks, no
//! `unsafe`.
//!
//! A stem being separated is written in parts from anywhere in the track, islands that grow
//! and meet ([`TrackAudio::pending`]): a frame can be read once the block of
//! [`PENDING_BLOCK`] frames around it is all written, which the writer counts after the
//! samples (release) and readers check before them (acquire).

use crate::{DecodeStream, MediaError};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

/// Room for more frames than the header announces (it can be slightly off).
const HEADER_MARGIN_S: usize = 2;

/// The frames of a [`TrackAudio::pending`] track that become readable together.
pub const PENDING_BLOCK: usize = 1024;

pub struct TrackAudio {
    sample_rate: u32,
    /// Interleaved stereo samples (`f32` bits).
    samples: Box<[AtomicU32]>,
    /// The end of the frames that can be read.
    decoded: AtomicUsize,
    /// The track's length: the header's estimate while decoding, exact afterwards.
    frames: AtomicUsize,
    /// For a [`TrackAudio::pending`] track, the frames written of each block of
    /// [`PENDING_BLOCK`]; empty for the others.
    filled: Box<[AtomicU32]>,
    /// For a [`TrackAudio::pending`] track, its blocks not all written yet.
    unfilled: AtomicUsize,
}

impl TrackAudio {
    /// A complete track from interleaved stereo samples.
    pub fn from_interleaved(sample_rate: u32, samples: Vec<f32>) -> Self {
        assert!(samples.len().is_multiple_of(2), "stereo data must have an even number of samples");
        let frames = samples.len() / 2;
        Self {
            sample_rate,
            samples: samples.into_iter().map(|s| AtomicU32::new(s.to_bits())).collect(),
            decoded: AtomicUsize::new(frames),
            frames: AtomicUsize::new(frames),
            filled: Box::new([]),
            unfilled: AtomicUsize::new(0),
        }
    }

    fn empty(sample_rate: u32, expected_frames: usize) -> Self {
        let capacity = expected_frames + HEADER_MARGIN_S * sample_rate as usize;
        Self::with_capacity(sample_rate, capacity, expected_frames)
    }

    /// A track of `frames` frames written later in parts by [`Self::publish`], anywhere in
    /// it: a stem being separated, played as it comes.
    pub fn pending(sample_rate: u32, frames: usize) -> Self {
        let blocks = frames.div_ceil(PENDING_BLOCK);
        Self {
            filled: (0..blocks).map(|_| AtomicU32::new(0)).collect(),
            unfilled: AtomicUsize::new(blocks),
            decoded: AtomicUsize::new(frames),
            ..Self::with_capacity(sample_rate, frames, frames)
        }
    }

    fn with_capacity(sample_rate: u32, capacity: usize, frames: usize) -> Self {
        Self {
            sample_rate,
            samples: (0..capacity * 2).map(|_| AtomicU32::new(0)).collect(),
            decoded: AtomicUsize::new(0),
            frames: AtomicUsize::new(frames),
            filled: Box::new([]),
            unfilled: AtomicUsize::new(0),
        }
    }

    fn is_pending(&self) -> bool {
        !self.filled.is_empty()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Length in stereo frames (estimated from the header until decoding ends).
    pub fn frames(&self) -> usize {
        self.frames.load(Ordering::Acquire)
    }

    pub fn duration_s(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }

    /// The end of the frames decoded from the start; a [`Self::pending`] track holds its
    /// length there, and tells what can be read block by block ([`Self::readable`]).
    pub fn decoded_frames(&self) -> usize {
        self.decoded.load(Ordering::Acquire)
    }

    /// The frames that can be read from the start on.
    pub fn readable(&self) -> Range<usize> {
        if self.is_pending() && !self.is_complete() {
            let blocks = self.filled.iter().enumerate();
            let first_gap = blocks.take_while(|&(b, _)| self.block_filled(b)).count();
            return 0..(first_gap * PENDING_BLOCK).min(self.frames());
        }
        0..self.decoded_frames()
    }

    pub fn is_complete(&self) -> bool {
        if self.is_pending() {
            return self.unfilled.load(Ordering::Acquire) == 0;
        }
        self.decoded_frames() >= self.frames()
    }

    fn block_filled(&self, block: usize) -> bool {
        let len = (self.frames() - block * PENDING_BLOCK).min(PENDING_BLOCK);
        self.filled[block].load(Ordering::Acquire) as usize == len
    }

    /// Frame `i` as (left, right); silence outside the track or not decoded yet.
    #[inline]
    pub fn frame(&self, i: isize) -> (f32, f32) {
        self.view().frame(i)
    }

    /// The frames that can be read so far, for reading many of them (one atomic load per
    /// view rather than per frame, two for a pending track; one more per frame of a pending
    /// track not complete).
    #[inline]
    pub fn view(&self) -> TrackView<'_> {
        if self.is_pending() && !self.is_complete() {
            return TrackView { samples: &self.samples, filled: Some(&self.filled) };
        }
        TrackView { samples: &self.samples[..self.decoded_frames() * 2], filled: None }
    }

    /// Calls `f` with the samples that can be read, interleaved, `block_frames` frames at a
    /// time.
    pub fn for_each_block(&self, block_frames: usize, mut f: impl FnMut(&[f32])) {
        let mut block = Vec::with_capacity(block_frames * 2);
        let readable = self.readable();
        for start in readable.clone().step_by(block_frames.max(1)) {
            let end = (start + block_frames).min(readable.end);
            block.clear();
            block.extend(
                self.samples[start * 2..end * 2].iter().map(|s| f32::from_bits(s.load(Ordering::Relaxed))),
            );
            f(&block);
        }
    }

    /// Writes interleaved frames from frame `start` of a [`Self::pending`] track, none of them
    /// written before, and makes readable the blocks they complete. Single writer.
    pub fn publish(&self, start: usize, interleaved: &[f32]) {
        assert!(self.is_pending(), "only a pending track is written in parts");
        let end = start + interleaved.len() / 2;
        assert!(end <= self.frames(), "the frames lie within the track");
        for (slot, &value) in self.samples[start * 2..end * 2].iter().zip(interleaved) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        let mut at = start;
        while at < end {
            let block = at / PENDING_BLOCK;
            let upto = ((block + 1) * PENDING_BLOCK).min(end);
            let len = (self.frames() - block * PENDING_BLOCK).min(PENDING_BLOCK);
            let filled =
                self.filled[block].fetch_add((upto - at) as u32, Ordering::Release) as usize + upto - at;
            assert!(filled <= len, "frames are written once");
            if filled == len {
                self.unfilled.fetch_sub(1, Ordering::Release);
            }
            at = upto;
        }
    }

    /// Appends interleaved frames; those beyond the buffer are dropped. Single writer.
    fn append(&self, interleaved: &[f32]) {
        let start = self.decoded.load(Ordering::Relaxed);
        let room = self.samples.len() / 2 - start;
        let frames = (interleaved.len() / 2).min(room);
        for (slot, &value) in self.samples[start * 2..(start + frames) * 2].iter().zip(interleaved) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        self.decoded.store(start + frames, Ordering::Release);
    }

    fn finish(&self) {
        self.frames.store(self.decoded.load(Ordering::Relaxed), Ordering::Release);
    }
}

impl std::fmt::Debug for TrackAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackAudio")
            .field("sample_rate", &self.sample_rate)
            .field("frames", &self.frames())
            .field("readable", &self.readable())
            .finish()
    }
}

/// The frames of a track that could be read when the view was taken: for a pending track,
/// those whose block is written when they are read.
#[derive(Clone, Copy)]
pub struct TrackView<'a> {
    /// Up to the last readable frame.
    samples: &'a [AtomicU32],
    /// The frames written of each block, while a pending track is not complete.
    filled: Option<&'a [AtomicU32]>,
}

impl TrackView<'_> {
    /// Frame `i` as (left, right); silence outside the view.
    #[inline]
    pub fn frame(&self, i: isize) -> (f32, f32) {
        self.get(i).unwrap_or((0.0, 0.0))
    }

    /// Frame `i` as (left, right), if the view holds it.
    #[inline]
    pub fn get(&self, i: isize) -> Option<(f32, f32)> {
        let i = usize::try_from(i).ok()?;
        if let Some(filled) = self.filled {
            let block = i / PENDING_BLOCK;
            let len = (self.samples.len() / 2).checked_sub(block * PENDING_BLOCK)?.min(PENDING_BLOCK);
            if filled.get(block)?.load(Ordering::Acquire) as usize != len {
                return None;
            }
        }
        let k = i.checked_mul(2)?;
        match self.samples.get(k..k + 2) {
            Some([l, r]) => {
                Some((f32::from_bits(l.load(Ordering::Relaxed)), f32::from_bits(r.load(Ordering::Relaxed))))
            }
            _ => None,
        }
    }
}

/// A track whose decoding has started: [`LoadingTrack::audio`] can be played right away
/// and [`LoadingTrack::decode_rest_while`] (on a worker thread) completes it.
pub struct LoadingTrack {
    audio: Arc<TrackAudio>,
    stream: Option<DecodeStream>,
}

/// Decoded before [`LoadingTrack::open`] returns, so that playback starts with sound.
const FIRST_FRAMES_S: f64 = 0.5;

impl LoadingTrack {
    /// Opens the file and decodes its first moments. Files whose header does not give
    /// their length are decoded entirely here.
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        let mut stream = DecodeStream::open(path)?;
        let expected = stream.estimated_frames();
        if expected == 0 {
            return Ok(Self { audio: Arc::new(crate::decode_file(path)?), stream: None });
        }
        let audio = Arc::new(TrackAudio::empty(stream.sample_rate(), expected));
        let first = (FIRST_FRAMES_S * stream.sample_rate() as f64) as usize;
        let mut chunk = Vec::new();
        while audio.decoded_frames() < first {
            chunk.clear();
            if !stream.next_chunk(&mut chunk)? {
                audio.finish();
                return Ok(Self { audio, stream: None });
            }
            audio.append(&chunk);
        }
        Ok(Self { audio, stream: Some(stream) })
    }

    pub fn audio(&self) -> &Arc<TrackAudio> {
        &self.audio
    }

    /// Decodes the rest of the track while `go_on` says so, asked between packets, each one
    /// readable in [`Self::audio`] by then: a track no longer wanted ends where its decoding
    /// was left. A file damaged further on keeps what was decoded, and the error says why it
    /// stops there.
    pub fn decode_rest_while(
        mut self,
        mut go_on: impl FnMut() -> bool,
    ) -> (Arc<TrackAudio>, Result<(), MediaError>) {
        let result = match self.stream.take() {
            Some(mut stream) => {
                let mut chunk = Vec::new();
                loop {
                    if !go_on() {
                        break Ok(());
                    }
                    chunk.clear();
                    match stream.next_chunk(&mut chunk) {
                        Ok(true) => self.audio.append(&chunk),
                        Ok(false) => break Ok(()),
                        Err(e) => break Err(e),
                    }
                }
            }
            None => Ok(()),
        };
        self.audio.finish();
        (self.audio, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_outside_the_track_are_silent() {
        let t = TrackAudio::from_interleaved(44_100, vec![0.5, -0.5]);
        assert_eq!(t.frame(0), (0.5, -0.5));
        assert_eq!(t.frame(-1), (0.0, 0.0));
        assert_eq!(t.frame(1), (0.0, 0.0));
        assert!(t.is_complete());
    }

    #[test]
    fn a_track_being_written_shows_what_is_written() {
        let t = TrackAudio::empty(10, 4);
        assert_eq!((t.frames(), t.decoded_frames()), (4, 0));
        t.append(&[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(t.frame(1), (0.3, 0.4));
        assert_eq!(t.frame(2), (0.0, 0.0), "not decoded yet");
        assert!(!t.is_complete());
        t.append(&[0.5, 0.6]);
        t.finish();
        assert_eq!((t.frames(), t.decoded_frames()), (3, 3), "the length becomes exact");
        assert!(t.is_complete());

        let mut seen = Vec::new();
        t.for_each_block(2, |block| seen.push(block.to_vec()));
        assert_eq!(seen, [vec![0.1, 0.2, 0.3, 0.4], vec![0.5, 0.6]]);
    }

    #[test]
    fn a_pending_track_reads_the_blocks_written() {
        let frames = 3 * PENDING_BLOCK + 10;
        let t = TrackAudio::pending(10, frames);
        assert!(t.view().get(5).is_none());
        // an island in the second block, then one over the third and the short last one
        t.publish(PENDING_BLOCK, &vec![0.5; 2 * PENDING_BLOCK]);
        t.publish(2 * PENDING_BLOCK, &vec![0.25; 2 * (PENDING_BLOCK + 10)]);
        assert_eq!(t.view().get(PENDING_BLOCK as isize), Some((0.5, 0.5)));
        assert_eq!(t.frame((3 * PENDING_BLOCK + 9) as isize), (0.25, 0.25));
        assert!(t.view().get(3).is_none(), "the first block is not written");
        assert_eq!(t.readable(), 0..0, "the frames read from the start");
        assert!(!t.is_complete());
        // the first block in two parts: readable once both are there
        t.publish(0, &vec![1.0; 2 * 100]);
        assert!(t.view().get(3).is_none());
        t.publish(100, &vec![1.0; 2 * (PENDING_BLOCK - 100)]);
        assert_eq!(t.view().get(3), Some((1.0, 1.0)));
        assert!(t.is_complete());
        assert_eq!(t.readable(), 0..frames);
        assert!(t.view().get(frames as isize).is_none());
    }

    #[test]
    #[should_panic(expected = "once")]
    fn a_pending_track_refuses_frames_written_twice() {
        let t = TrackAudio::pending(10, 2 * PENDING_BLOCK);
        t.publish(0, &vec![0.0; 2 * PENDING_BLOCK]);
        t.publish(10, &[0.0; 2]);
    }

    #[test]
    fn frames_beyond_the_buffer_are_dropped() {
        let t = TrackAudio::empty(1, 1);
        t.append(&[1.0; 20]);
        assert_eq!(t.decoded_frames(), 1 + HEADER_MARGIN_S);
    }

    #[test]
    fn reading_while_decoding_sees_whole_frames() {
        let t = Arc::new(TrackAudio::empty(1_000, 100_000));
        let writer = {
            let t = t.clone();
            std::thread::spawn(move || {
                for i in 0..1_000 {
                    let v = i as f32;
                    t.append(&[v; 200]);
                }
                t.finish();
            })
        };
        while !t.is_complete() {
            let n = t.decoded_frames();
            if n > 0 {
                let (l, r) = t.frame(n as isize - 1);
                assert_eq!(l, r);
                assert_eq!(l, ((n - 1) / 100) as f32);
            }
        }
        writer.join().unwrap();
        assert_eq!(t.frames(), 100_000);
    }
}
