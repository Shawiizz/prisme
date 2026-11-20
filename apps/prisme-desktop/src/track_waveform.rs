//! The waveform of a loaded track on its way to the page: whole at once when the library
//! has it; otherwise computed as the track is decoded and sent as it grows, from where it
//! changed, the page drawing the start of the track while the rest is read. Each has a lock of
//! its own, which the pieces are sent under: never the decks' tracks', which the controllers
//! wait on.

use crate::dto::{waveform_bytes, whole_waveform_bytes};
use crate::state::{lock, AppState};
use prisme_analysis::envelope::Envelope;
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
    /// While the track is decoded: the slices so far, of about `slices`, their envelope, and
    /// the pages watching it, which get what changed of it each time it grows.
    Growing { so_far: Waveform, envelope: Envelope, slices: usize, watchers: Vec<Channel> },
}

impl TrackWaveform {
    pub fn growing(slices_per_second: u32) -> Self {
        let so_far = Waveform::new(slices_per_second);
        Self::of(Shape::Growing { so_far, envelope: Envelope::default(), slices: 0, watchers: Vec::new() })
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

    /// Sends what is known to `channel`, then, while it grows, what changed of it each time.
    pub fn watch(&self, channel: Channel) {
        match &mut *lock(&self.0) {
            Shape::Whole(w) => send(&channel, whole_waveform_bytes(w)),
            Shape::Growing { so_far, envelope, slices, watchers } => {
                send(&channel, waveform_bytes(so_far, envelope.heights(), *slices, 0));
                watchers.push(channel);
            }
        }
    }

    /// Adds the slices `range` of `computed`, of a track of about `slices`, and sends what
    /// changed.
    fn grow(&self, computed: &Waveform, range: Range<usize>, slices: usize) {
        if let Shape::Growing { so_far, envelope, slices: expected, watchers } = &mut *lock(&self.0) {
            so_far.extend_from(computed, range);
            let from = envelope.grow(so_far);
            *expected = slices;
            if !watchers.is_empty() {
                let bytes = waveform_bytes(so_far, envelope.heights(), slices, from);
                for channel in watchers.iter() {
                    send(channel, bytes.clone());
                }
            }
        }
    }

    /// The whole waveform, sent: its heights set against all of it.
    fn complete(&self, whole: Waveform) {
        let mut shape = lock(&self.0);
        if let Shape::Growing { watchers, .. } = &*shape {
            let bytes = whole_waveform_bytes(&whole);
            for channel in watchers.iter() {
                send(channel, bytes.clone());
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
            Shape::Growing { so_far, envelope, slices, .. } => Shape::Growing {
                so_far: so_far.clone(),
                envelope: envelope.clone(),
                slices: *slices,
                watchers: Vec::new(),
            },
        })
    }
}

fn send(channel: &Channel, bytes: Vec<u8>) {
    let _ = channel.send(InvokeResponseBody::Raw(bytes));
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
    /// `audio`: `PIECE_EVERY` after the last were given, however long giving them took.
    pub fn follow(&mut self, audio: &Arc<TrackAudio>, state: &AppState) {
        self.builder.read(audio);
        if self.given_at.is_some_and(|at| at.elapsed() < PIECE_EVERY) {
            return;
        }
        let waveform = self.builder.waveform();
        let (range, slices) = (self.given..waveform.len(), self.builder.slices_of(audio.frames()));
        for growing in waveforms_of(state, audio) {
            growing.grow(waveform, range.clone(), slices.max(range.end));
        }
        self.given = range.end;
        self.given_at = Some(Instant::now());
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
            Shape::Growing { so_far, envelope, slices, watchers } => {
                assert_eq!((so_far, *slices, watchers.len()), (&computed, 5, 0));
                assert_eq!(envelope.heights().len(), 4);
            }
            Shape::Whole(_) => panic!("still growing"),
        }
        let whole = waveform(&[1, 2, 3, 4, 5]);
        w.same().complete(whole.clone());
        assert_eq!(w.read_whole(Waveform::clone), Some(whole));
        assert!(!double.is_whole(), "a copy of its own");
    }
}
