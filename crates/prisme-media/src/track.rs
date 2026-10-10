//! A decoded track, playable while it is being decoded.
//!
//! The samples live in a buffer sized from the file header and never reallocated. The
//! decoding thread writes past [`TrackAudio::decoded_frames`] and then publishes the new
//! count (release); readers see only what was published (acquire). Samples are stored as
//! `f32` bits in `AtomicU32`s: plain loads and stores on x86 and ARM, no locks, no
//! `unsafe`.

use crate::{DecodeStream, MediaError};
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

/// Room for more frames than the header announces (it can be slightly off).
const HEADER_MARGIN_S: usize = 2;

pub struct TrackAudio {
    sample_rate: u32,
    /// Interleaved stereo samples (`f32` bits).
    samples: Box<[AtomicU32]>,
    /// Frames written so far.
    decoded: AtomicUsize,
    /// The track's length: the header's estimate while decoding, exact afterwards.
    frames: AtomicUsize,
}

impl TrackAudio {
    /// A complete track from interleaved stereo samples.
    pub fn from_interleaved(sample_rate: u32, samples: Vec<f32>) -> Self {
        assert!(samples.len() % 2 == 0, "stereo data must have an even number of samples");
        let frames = samples.len() / 2;
        Self {
            sample_rate,
            samples: samples.into_iter().map(|s| AtomicU32::new(s.to_bits())).collect(),
            decoded: AtomicUsize::new(frames),
            frames: AtomicUsize::new(frames),
        }
    }

    fn empty(sample_rate: u32, expected_frames: usize) -> Self {
        let capacity = expected_frames + HEADER_MARGIN_S * sample_rate as usize;
        Self {
            sample_rate,
            samples: (0..capacity * 2).map(|_| AtomicU32::new(0)).collect(),
            decoded: AtomicUsize::new(0),
            frames: AtomicUsize::new(expected_frames),
        }
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

    /// Frames that can be read.
    pub fn decoded_frames(&self) -> usize {
        self.decoded.load(Ordering::Acquire)
    }

    pub fn is_complete(&self) -> bool {
        self.decoded_frames() >= self.frames()
    }

    /// Frame `i` as (left, right); silence outside the track or not decoded yet.
    #[inline]
    pub fn frame(&self, i: isize) -> (f32, f32) {
        self.view().frame(i)
    }

    /// The frames decoded so far, for reading many of them (one atomic load per view
    /// rather than per frame).
    #[inline]
    pub fn view(&self) -> TrackView<'_> {
        TrackView { samples: &self.samples[..self.decoded_frames() * 2] }
    }

    /// Calls `f` with the decoded samples, interleaved, `block_frames` frames at a time.
    pub fn for_each_block(&self, block_frames: usize, mut f: impl FnMut(&[f32])) {
        let mut block = Vec::with_capacity(block_frames * 2);
        let decoded = self.decoded_frames();
        for start in (0..decoded).step_by(block_frames.max(1)) {
            let end = (start + block_frames).min(decoded);
            block.clear();
            block.extend(
                self.samples[start * 2..end * 2].iter().map(|s| f32::from_bits(s.load(Ordering::Relaxed))),
            );
            f(&block);
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
            .field("decoded", &self.decoded_frames())
            .finish()
    }
}

/// Decoded frames of a track, as they were when the view was taken.
#[derive(Clone, Copy)]
pub struct TrackView<'a> {
    samples: &'a [AtomicU32],
}

impl TrackView<'_> {
    /// Frame `i` as (left, right); silence outside the view.
    #[inline]
    pub fn frame(&self, i: isize) -> (f32, f32) {
        let k = (i as usize).wrapping_mul(2);
        match self.samples.get(k..k.wrapping_add(2)) {
            Some([l, r]) if i >= 0 => {
                (f32::from_bits(l.load(Ordering::Relaxed)), f32::from_bits(r.load(Ordering::Relaxed)))
            }
            _ => (0.0, 0.0),
        }
    }
}

/// A track whose decoding has started: [`LoadingTrack::audio`] can be played right away
/// and [`LoadingTrack::decode_rest`] (on a worker thread) completes it.
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

    /// Decodes the rest of the track. A file damaged further on keeps what was decoded,
    /// and the error says why it stops there.
    pub fn decode_rest(mut self) -> (Arc<TrackAudio>, Result<(), MediaError>) {
        let result = match self.stream.take() {
            Some(mut stream) => {
                let mut chunk = Vec::new();
                loop {
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
