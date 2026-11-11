//! A track's separated stems beside it, so that the deck can mute any of them. What no stem
//! holds (the other instruments) is the track minus the stems, so the deck plays
//! `other_gain * track + Σ (stem_gain - other_gain) * stem`, mixed as the frames are read:
//! the interpolation and the key lock see a single signal.
//!
//! A track separated into its vocals alone has no drums nor bass: they stay in the other
//! instruments and follow their gain. So does a stem where it is not separated yet (the
//! stems of a track being separated come as they are made); where none is, the track plays
//! whole.

use prisme_media::{TrackAudio, TrackView};
use std::sync::Arc;

/// Gains move by this much per frame read: a toggle fades in about 10 ms instead of clicking.
const RAMP_STEP: f32 = 1.0 / 512.0;

/// The stems a track can be separated into, besides the other instruments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stem {
    Vocals = 0,
    Drums = 1,
    Bass = 2,
}

impl Stem {
    pub const ALL: [Stem; 3] = [Stem::Vocals, Stem::Drums, Stem::Bass];

    /// As the separation models name it.
    pub fn name(self) -> &'static str {
        match self {
            Stem::Vocals => "vocals",
            Stem::Drums => "drums",
            Stem::Bass => "bass",
        }
    }
}

/// The separated stems of a track, frame for frame at its rate, indexed by [`Stem`].
pub type StemTracks = [Option<Arc<TrackAudio>>; 3];

/// Which parts of the track a deck plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StemMix {
    pub vocals: bool,
    pub drums: bool,
    pub bass: bool,
    pub other: bool,
}

impl Default for StemMix {
    fn default() -> Self {
        Self { vocals: true, drums: true, bass: true, other: true }
    }
}

pub(crate) struct Stems {
    tracks: StemTracks,
    gains: [Ramp; 3],
    other: Ramp,
}

impl Stems {
    pub(crate) fn new() -> Self {
        Self {
            tracks: Default::default(),
            gains: [Ramp::at(1.0), Ramp::at(1.0), Ramp::at(1.0)],
            other: Ramp::at(1.0),
        }
    }

    pub(crate) fn set_mix(&mut self, mix: StemMix) {
        for (ramp, on) in self.gains.iter_mut().zip([mix.vocals, mix.drums, mix.bass]) {
            ramp.target = gain(on);
        }
        self.other.target = gain(mix.other);
    }

    /// Sets the stems of the loaded track and returns the previous ones, to be freed elsewhere.
    pub(crate) fn set_tracks(&mut self, tracks: StemTracks) -> StemTracks {
        std::mem::replace(&mut self.tracks, tracks)
    }

    /// Forgets the stems and plays the whole track again (a new track is loaded).
    pub(crate) fn clear(&mut self) -> StemTracks {
        let tracks = std::mem::take(&mut self.tracks);
        *self = Self::new();
        tracks
    }

    pub(crate) fn reader<'a>(&'a mut self, track: &'a TrackAudio) -> Reader<'a> {
        let mixing = !(self.other.is_one() && self.gains.iter().all(Ramp::is_one));
        let stems =
            std::array::from_fn(|i| self.tracks[i].as_deref().filter(|_| mixing).map(TrackAudio::view));
        Reader {
            track: track.view(),
            stems,
            gains: &mut self.gains,
            other: &mut self.other,
            sample_rate: track.sample_rate(),
        }
    }
}

/// Reads the track with the stems mixed in.
pub(crate) struct Reader<'a> {
    track: TrackView<'a>,
    /// All `None` while everything plays fully: the track as it is.
    stems: [Option<TrackView<'a>>; 3],
    gains: &'a mut [Ramp; 3],
    other: &'a mut Ramp,
    sample_rate: u32,
}

impl Reader<'_> {
    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frame `i` with the current gains; silence outside the track.
    #[inline]
    pub(crate) fn frame(&self, i: isize) -> (f32, f32) {
        let (l, r) = self.track.frame(i);
        let other = self.other.value;
        let (mut ml, mut mr) = (other * l, other * r);
        let mut separated = false;
        for (stem, ramp) in self.stems.iter().zip(self.gains.iter()) {
            if let Some((sl, sr)) = stem.and_then(|s| s.get(i)) {
                let delta = ramp.value - other;
                ml += delta * sl;
                mr += delta * sr;
                separated = true;
            }
        }
        if separated {
            (ml, mr)
        } else {
            (l, r)
        }
    }

    /// Moves the gains one frame towards their targets.
    #[inline]
    pub(crate) fn advance(&mut self) {
        if self.stems.iter().any(Option::is_some) {
            self.gains.iter_mut().for_each(Ramp::step);
            self.other.step();
        }
    }
}

struct Ramp {
    value: f32,
    target: f32,
}

impl Ramp {
    fn at(value: f32) -> Self {
        Self { value, target: value }
    }

    fn is_one(&self) -> bool {
        self.value == 1.0 && self.target == 1.0
    }

    #[inline]
    fn step(&mut self) {
        let d = self.target - self.value;
        self.value = if d.abs() <= RAMP_STEP { self.target } else { self.value + RAMP_STEP.copysign(d) };
    }
}

fn gain(on: bool) -> f32 {
    if on {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(value: f32, frames: usize) -> Arc<TrackAudio> {
        Arc::new(TrackAudio::from_interleaved(48_000, vec![value; frames * 2]))
    }

    fn settle(stems: &mut Stems, track: &TrackAudio) -> (f32, f32) {
        let mut reader = stems.reader(track);
        for _ in 0..1000 {
            reader.advance();
        }
        reader.frame(0)
    }

    fn mix(vocals: bool, drums: bool, bass: bool, other: bool) -> StemMix {
        StemMix { vocals, drums, bass, other }
    }

    /// A track of 1.0: vocals 0.5, drums 0.25, bass 0.125, the rest 0.125.
    fn separated() -> (Arc<TrackAudio>, Stems) {
        let mut stems = Stems::new();
        stems.set_tracks([Some(constant(0.5, 10)), Some(constant(0.25, 10)), Some(constant(0.125, 10))]);
        (constant(1.0, 10), stems)
    }

    #[test]
    fn each_stem_mutes_its_part_of_the_track() {
        let (track, mut stems) = separated();
        assert_eq!(settle(&mut stems, &track), (1.0, 1.0));
        let cases = [
            (mix(false, true, true, true), 0.5),
            (mix(true, false, true, true), 0.75),
            (mix(true, true, false, true), 0.875),
            (mix(true, true, true, false), 0.875),
            (mix(false, true, false, false), 0.25),
            (mix(false, false, false, true), 0.125),
            (mix(false, false, false, false), 0.0),
        ];
        for (m, level) in cases {
            stems.set_mix(m);
            assert_eq!(settle(&mut stems, &track), (level, level), "{m:?}");
        }
    }

    #[test]
    fn without_drums_and_bass_they_follow_the_other_instruments() {
        let mut stems = Stems::new();
        stems.set_tracks([Some(constant(0.25, 10)), None, None]);
        let track = constant(0.75, 10);
        stems.set_mix(mix(false, true, true, true));
        assert_eq!(settle(&mut stems, &track), (0.5, 0.5));
        stems.set_mix(mix(true, true, true, false));
        assert_eq!(settle(&mut stems, &track), (0.25, 0.25));
        stems.set_mix(mix(true, false, false, true));
        assert_eq!(settle(&mut stems, &track), (0.75, 0.75));
    }

    #[test]
    fn stems_play_where_they_are_separated_already() {
        let track = constant(1.0, 10);
        let vocals = Arc::new(TrackAudio::pending(48_000, 10));
        vocals.publish(4, &[0.5; 4]);
        let drums = Arc::new(TrackAudio::pending(48_000, 10));
        drums.publish(5, &[0.25; 2]);
        let mut stems = Stems::new();
        stems.set_tracks([Some(vocals), Some(drums), None]);
        stems.set_mix(mix(false, false, true, true));
        let mut reader = stems.reader(&track);
        for _ in 0..1000 {
            reader.advance();
        }
        assert_eq!(reader.frame(3), (1.0, 1.0), "nothing separated: the whole track");
        assert_eq!(reader.frame(4), (0.5, 0.5), "the drums not separated yet stay");
        assert_eq!(reader.frame(5), (0.25, 0.25), "both muted");
    }

    #[test]
    fn a_toggle_fades_instead_of_jumping() {
        let (track, mut stems) = separated();
        stems.set_mix(mix(true, false, false, false));
        let mut reader = stems.reader(&track);
        let mut last = reader.frame(0).0;
        for _ in 0..600 {
            reader.advance();
            let now = reader.frame(0).0;
            assert!((now - last).abs() <= 0.5 * RAMP_STEP + 1e-6, "{last} -> {now}");
            last = now;
        }
        assert_eq!(last, 0.5);
    }

    #[test]
    fn without_stems_the_track_plays_whatever_the_mix() {
        let track = constant(0.75, 10);
        let mut stems = Stems::new();
        stems.set_mix(mix(false, false, true, true));
        assert_eq!(settle(&mut stems, &track), (0.75, 0.75));
    }

    #[test]
    fn clearing_returns_the_stems_and_plays_the_whole_track() {
        let (_, mut stems) = separated();
        stems.set_mix(mix(false, true, true, true));
        assert!(stems.clear().iter().all(Option::is_some));
        assert!(stems.tracks.iter().all(Option::is_none));
        assert!(stems.other.is_one() && stems.gains.iter().all(Ramp::is_one));
    }
}
