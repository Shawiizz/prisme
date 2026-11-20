//! The waveform of a loaded track on its way to the page: whole at once when the library
//! has it; otherwise computed as the track is decoded and sent in pieces as they come, the
//! page drawing the start of the track while the rest is read. Each has a lock of its own,
//! which the pieces are sent under: never the decks' tracks', which the controllers wait on.

use crate::dto::waveform_bytes;
use crate::state::{lock, AppState};
use prisme_analysis::{Waveform, WaveformBuilder};
use prisme_media::TrackAudio;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};

/// How often the slices computed meanwhile go to the page while the track is decoded.
const PIECE_EVERY: Duration = Duration::from_millis(40);

/// The waveform of a loaded track, reached from the track and from what computes it.
pub struct TrackWaveform(Arc<Mutex<Shape>>);

enum Shape {
    /// All of it: from the library, or once the track is all decoded.
    Whole(Waveform),
    /// While the track is decoded: the slices so far, of about `slices`, and the pages
    /// watching it, which get each piece that follows.
    Growing { so_far: Waveform, slices: usize, watchers: Vec<Channel> },
}

impl TrackWaveform {
    pub fn growing(slices_per_second: u32) -> Self {
        Self::of(Shape::Growing { so_far: Waveform::new(slices_per_second), slices: 0, watchers: Vec::new() })
    }

    pub fn whole(waveform: Waveform) -> Self {
        Self::of(Shape::Whole(waveform))
    }

    fn of(shape: Shape) -> Self {
        Self(Arc::new(Mutex::new(shape)))
    }

    /// The same waveform, reached again (not a copy: see [`Clone`]).
    pub fn same(&self) -> Self {
        Self(self.0.clone())
    }

    pub fn is_whole(&self) -> bool {
        matches!(*lock(&self.0), Shape::Whole(_))
    }

    /// What `read` reads of the whole waveform, once it is known.
    pub fn read_whole<R>(&self, read: impl FnOnce(&Waveform) -> R) -> Option<R> {
        match &*lock(&self.0) {
            Shape::Whole(w) => Some(read(w)),
            Shape::Growing { .. } => None,
        }
    }

    /// Sends what is known to `channel`, then, while it grows, each piece.
    pub fn watch(&self, channel: Channel) {
        match &mut *lock(&self.0) {
            Shape::Whole(w) => send(&channel, w, 0..w.len(), w.len()),
            Shape::Growing { so_far, slices, watchers } => {
                send(&channel, so_far, 0..so_far.len(), *slices);
                watchers.push(channel);
            }
        }
    }

    /// Adds the slices `range` of `from`, of a track of about `slices`, and sends them.
    fn grow(&self, from: &Waveform, range: Range<usize>, slices: usize) {
        if let Shape::Growing { so_far, slices: expected, watchers } = &mut *lock(&self.0) {
            let start = so_far.len();
            so_far.extend_from(from, range);
            *expected = slices;
            for channel in watchers.iter() {
                send(channel, so_far, start..so_far.len(), slices);
            }
        }
    }

    /// The whole waveform, its last slices sent.
    fn complete(&self, whole: Waveform) {
        let mut shape = lock(&self.0);
        if let Shape::Growing { so_far, watchers, .. } = &*shape {
            for channel in watchers.iter() {
                send(channel, &whole, so_far.len()..whole.len(), whole.len());
            }
        }
        *shape = Shape::Whole(whole);
    }
}

/// A copy for an instant double: the pages showing the double watch it on their own.
impl Clone for TrackWaveform {
    fn clone(&self) -> Self {
        Self::of(match &*lock(&self.0) {
            Shape::Whole(w) => Shape::Whole(w.clone()),
            Shape::Growing { so_far, slices, .. } => {
                Shape::Growing { so_far: so_far.clone(), slices: *slices, watchers: Vec::new() }
            }
        })
    }
}

fn send(channel: &Channel, w: &Waveform, range: Range<usize>, slices: usize) {
    let _ = channel.send(InvokeResponseBody::Raw(waveform_bytes(w, range, slices)));
}

/// The waveforms of the decks playing `audio` (a load and its instant doubles), read from the
/// tracks under their lock, let go before the waveforms are.
fn waveforms_of(state: &AppState, audio: &Arc<TrackAudio>) -> Vec<TrackWaveform> {
    let tracks = lock(&state.tracks);
    let playing = tracks.iter().flatten().filter(|t| Arc::ptr_eq(&t.audio, audio));
    playing.map(|t| t.waveform.same()).collect()
}

/// Computes the waveform of a track as it is decoded, for the decks playing it.
pub struct WaveformFeed {
    builder: WaveformBuilder,
    /// The slices given to the decks.
    given: usize,
    given_at: Option<Instant>,
}

impl WaveformFeed {
    pub fn new(audio: &TrackAudio, slices_per_second: u32) -> Self {
        Self {
            builder: WaveformBuilder::new(audio.sample_rate(), slices_per_second),
            given: 0,
            given_at: None,
        }
    }

    /// Reads what was decoded since; now and then, gives the new slices to the decks playing
    /// `audio`.
    pub fn follow(&mut self, audio: &Arc<TrackAudio>, state: &AppState) {
        self.builder.read(audio);
        if self.given_at.is_some_and(|at| at.elapsed() < PIECE_EVERY) {
            return;
        }
        self.given_at = Some(Instant::now());
        let waveform = self.builder.waveform();
        let (range, slices) = (self.given..waveform.len(), self.builder.slices_of(audio.frames()));
        for growing in waveforms_of(state, audio) {
            growing.grow(waveform, range.clone(), slices.max(range.end));
        }
        self.given = range.end;
    }

    /// The whole waveform once `audio` is all decoded, given to the decks playing it.
    pub fn finish(mut self, audio: &Arc<TrackAudio>, state: &AppState) -> Waveform {
        self.builder.read(audio);
        let whole = self.builder.finish();
        for growing in waveforms_of(state, audio) {
            growing.complete(whole.clone());
        }
        whole
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waveform(peaks: &[u8]) -> Waveform {
        Waveform {
            slices_per_second: 10,
            peak: peaks.to_vec(),
            low: peaks.to_vec(),
            mid: peaks.to_vec(),
            high: peaks.to_vec(),
        }
    }

    #[test]
    fn a_growing_waveform_becomes_whole_and_a_double_is_watched_apart() {
        let w = TrackWaveform::growing(10);
        assert!(!w.is_whole());
        let computed = waveform(&[1, 2, 3, 4]);
        w.grow(&computed, 0..2, 5);
        w.grow(&computed, 2..4, 5);
        let double = w.clone();
        match &*lock(&double.0) {
            Shape::Growing { so_far, slices, watchers } => {
                assert_eq!((so_far, *slices, watchers.len()), (&computed, 5, 0));
            }
            Shape::Whole(_) => panic!("still growing"),
        }
        let whole = waveform(&[1, 2, 3, 4, 5]);
        w.same().complete(whole.clone());
        assert_eq!(w.read_whole(Waveform::clone), Some(whole));
        assert!(!double.is_whole(), "a copy of its own");
    }
}
