//! A decoded track, playable while it is being decoded.
//!
//! The samples live in a buffer sized from the file header and never reallocated. The
//! decoding thread writes past [`TrackAudio::decoded_frames`] and then publishes the new
//! count (release); readers see only what was published (acquire). Samples are stored as
//! `f32` bits in `AtomicU32`s: plain loads and stores on x86 and ARM, no locks, no
//! `unsafe`. A track longer than its header said (MP3s joined, the first one's header kept)
//! gets the frames that found no room in a second buffer once its decoding ends. All of it
//! is kept in RAM ([`Resident`]).
//!
//! A stem being separated is written in parts from anywhere in the track, islands that grow
//! and meet ([`TrackAudio::pending`]): a frame can be read once the block of
//! [`PENDING_BLOCK`] frames around it is all written, which the writer counts after the
//! samples (release) and readers check before them (acquire).

use crate::resident::Resident;
use crate::{DecodeStream, MediaError};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

/// Room for more frames than the header announces (it can be slightly off).
const HEADER_MARGIN_S: usize = 2;

/// The frames of a [`TrackAudio::pending`] track that become readable together.
pub const PENDING_BLOCK: usize = 1024;

pub struct TrackAudio {
    sample_rate: u32,
    /// Interleaved stereo samples (`f32` bits).
    samples: Resident<AtomicU32>,
    /// The samples that follow those of a full `samples`, set once at the end of the decoding.
    more: OnceLock<Resident<AtomicU32>>,
    /// The end of the frames that can be read.
    decoded: AtomicUsize,
    /// The track's length: while decoding, the header's estimate or, past it, one more than
    /// the frames decoded; exact afterwards.
    frames: AtomicUsize,
    /// For a [`TrackAudio::pending`] track, the frames written of each block of
    /// [`PENDING_BLOCK`]; empty for the others.
    filled: Resident<AtomicU32>,
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
            more: OnceLock::new(),
            decoded: AtomicUsize::new(frames),
            frames: AtomicUsize::new(frames),
            filled: Resident::new(Box::new([])),
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
            more: OnceLock::new(),
            decoded: AtomicUsize::new(0),
            frames: AtomicUsize::new(frames),
            filled: Resident::new(Box::new([])),
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

    /// Whether no frame can be read: of a [`Self::pending`] track, no block all written yet.
    pub fn is_blank(&self) -> bool {
        if self.is_pending() {
            return self.unfilled.load(Ordering::Acquire) == self.filled.len();
        }
        self.decoded_frames() == 0
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

    /// The frames that can be read so far, for reading many of them (two atomic loads per
    /// view rather than per frame, three for a pending track; one more per frame of a pending
    /// track not complete).
    #[inline]
    pub fn view(&self) -> TrackView<'_> {
        if self.is_pending() && !self.is_complete() {
            return TrackView { samples: &self.samples, more: &[], filled: Some(&self.filled) };
        }
        let end = self.decoded_frames() * 2;
        let more = self
            .more
            .get()
            .map_or(&[][..], |more| &more[..end.saturating_sub(self.samples.len()).min(more.len())]);
        TrackView { samples: &self.samples[..end.min(self.samples.len())], more, filled: None }
    }

    /// Calls `f` with the samples that can be read, interleaved, `block_frames` frames at a
    /// time.
    pub fn for_each_block(&self, block_frames: usize, mut f: impl FnMut(&[f32])) {
        let block_len = block_frames.max(1) * 2;
        let mut block = Vec::with_capacity(block_len);
        let more = self.more.get().map_or(&[][..], |more| &more[..]);
        for sample in self.samples.iter().chain(more).take(self.readable().end * 2) {
            block.push(f32::from_bits(sample.load(Ordering::Relaxed)));
            if block.len() == block_len {
                f(&block);
                block.clear();
            }
        }
        if !block.is_empty() {
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

    /// Appends interleaved frames, as many as the buffer has room for: how many. Single
    /// writer.
    fn append(&self, interleaved: &[f32]) -> usize {
        let start = self.decoded.load(Ordering::Relaxed);
        let room = self.samples.len() / 2 - start;
        let frames = (interleaved.len() / 2).min(room);
        for (slot, &value) in self.samples[start * 2..(start + frames) * 2].iter().zip(interleaved) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        // the length before the frames, so that the track is not complete while decoding
        self.frames.fetch_max(start + frames + 1, Ordering::Release);
        self.decoded.store(start + frames, Ordering::Release);
        frames
    }

    /// Ends the decoding, the interleaved frames that found no room in the buffer, `rest`,
    /// after it: the length becomes exact.
    fn finish(&self, rest: Vec<f32>) {
        let mut end = self.decoded.load(Ordering::Relaxed);
        let frames = rest.len() / 2;
        if frames > 0
            && self.more.set(rest.into_iter().map(|s| AtomicU32::new(s.to_bits())).collect()).is_ok()
        {
            end += frames;
        }
        // the length first, so that the track is complete with its length exact
        self.frames.store(end, Ordering::Release);
        self.decoded.store(end, Ordering::Release);
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
    /// The readable frames past `samples`, of a track longer than its header said.
    more: &'a [AtomicU32],
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
        let frame = match k.checked_sub(self.samples.len()) {
            None => self.samples.get(k..k + 2),
            Some(k) => self.more.get(k..k + 2),
        };
        match frame {
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
    /// The frames decoded once the buffer was full, interleaved.
    rest: Vec<f32>,
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
            let audio = Arc::new(crate::decode_file(path)?);
            return Ok(Self { audio, stream: None, rest: Vec::new() });
        }
        let audio = Arc::new(TrackAudio::empty(stream.sample_rate(), expected));
        let mut track = Self { audio, stream: None, rest: Vec::new() };
        let first = (FIRST_FRAMES_S * stream.sample_rate() as f64) as usize;
        let mut chunk = Vec::new();
        while track.audio.decoded_frames() < first {
            chunk.clear();
            if !stream.next_chunk(&mut chunk)? {
                // a header and no audio after it, which decode_file refuses too
                if track.audio.decoded_frames() == 0 {
                    return Err(MediaError::NoAudio);
                }
                track.finish();
                return Ok(track);
            }
            track.append(&chunk);
        }
        track.stream = Some(stream);
        Ok(track)
    }

    pub fn audio(&self) -> &Arc<TrackAudio> {
        &self.audio
    }

    /// Decodes the rest of the track while `go_on` says so, asked between packets, each one
    /// readable in [`Self::audio`] by then (those past the room its header gave, at the end): a
    /// track no longer wanted ends where its decoding was left. A file damaged further on keeps
    /// what was decoded, and the error says why it stops there.
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
                        Ok(true) => self.append(&chunk),
                        Ok(false) => break Ok(()),
                        Err(e) => break Err(e),
                    }
                }
            }
            None => Ok(()),
        };
        self.finish();
        (self.audio, result)
    }

    /// Appends interleaved frames to the track; those that find no room wait for the end.
    fn append(&mut self, interleaved: &[f32]) {
        let appended = if self.rest.is_empty() { self.audio.append(interleaved) } else { 0 };
        self.rest.extend_from_slice(&interleaved[appended * 2..]);
    }

    fn finish(&mut self) {
        self.audio.finish(std::mem::take(&mut self.rest));
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
        t.finish(Vec::new());
        assert_eq!((t.frames(), t.decoded_frames()), (3, 3), "the length becomes exact");
        assert!(t.is_complete());

        let mut seen = Vec::new();
        t.for_each_block(2, |block| seen.push(block.to_vec()));
        assert_eq!(seen, [vec![0.1, 0.2, 0.3, 0.4], vec![0.5, 0.6]]);
    }

    #[test]
    fn a_pending_track_is_blank_until_a_block_is_all_written() {
        let t = TrackAudio::pending(10, 2 * PENDING_BLOCK);
        assert!(t.is_blank());
        t.publish(PENDING_BLOCK, &[0.5; 2 * 10]);
        assert!(t.is_blank(), "a block written in part");
        t.publish(PENDING_BLOCK + 10, &vec![0.5; 2 * (PENDING_BLOCK - 10)]);
        assert!(!t.is_blank());
        assert!(!TrackAudio::from_interleaved(10, vec![0.5, 0.5]).is_blank());
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
    fn frames_beyond_the_buffer_follow_it_once_decoded() {
        let t = TrackAudio::empty(1, 1);
        let room = 1 + HEADER_MARGIN_S;
        let frames: Vec<f32> = (0..20).map(|i| (i / 2) as f32).collect();
        assert_eq!(t.append(&frames), room);
        assert_eq!(t.frame(room as isize), (0.0, 0.0), "not decoded yet");
        assert!(!t.is_complete(), "the decoding goes on");
        t.finish(frames[room * 2..].to_vec());
        assert_eq!((t.frames(), t.decoded_frames()), (10, 10));
        assert_eq!(t.frame(room as isize), (room as f32, room as f32));
        assert_eq!(t.view().get(9), Some((9.0, 9.0)));
        assert_eq!(t.view().get(10), None);
        let mut seen = Vec::new();
        t.for_each_block(4, |block| seen.extend_from_slice(block));
        assert_eq!(seen, frames);
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
                t.finish(Vec::new());
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
