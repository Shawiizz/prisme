//! A track's separated stems beside it, so that the deck can mute any of them. What no stem
//! holds (the other instruments) is the track minus the stems, so the deck plays
//! `other_gain * track + Σ (stem_gain - other_gain) * stem`, mixed as the frames are read:
//! the interpolation and the key lock see a single signal.
//!
//! A track separated into its vocals alone has no drums nor bass: they stay in the other
//! instruments and follow their gain. So does a stem where it is not separated yet (the
//! stems of a track being separated come as they are made); where none is, the track plays
//! whole. Crossing from one to the other fades in about 10 ms, as a gain does.

use prisme_media::{TrackAudio, TrackView};
use std::cell::Cell;
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

/// How loud a deck plays each part of the track, from 0 (muted) to 1 (as in the track).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StemMix {
    pub vocals: f32,
    pub drums: f32,
    pub bass: f32,
    pub other: f32,
}

impl Default for StemMix {
    fn default() -> Self {
        Self { vocals: 1.0, drums: 1.0, bass: 1.0, other: 1.0 }
    }
}

pub(crate) struct Stems {
    tracks: StemTracks,
    gains: [Ramp; 3],
    other: Ramp,
    /// How much each stem is there where the deck plays: 0 where it is not separated yet.
    present: [Ramp; 3],
    /// How much any stem is: 0 where none is, and the track plays whole.
    separated: Ramp,
    /// Every part played as in the track until now: the presence takes the truth at once,
    /// as it changes nothing heard yet.
    settle_presence: bool,
}

impl Stems {
    pub(crate) fn new() -> Self {
        Self {
            tracks: Default::default(),
            gains: [Ramp::at(1.0), Ramp::at(1.0), Ramp::at(1.0)],
            other: Ramp::at(1.0),
            present: [Ramp::at(0.0), Ramp::at(0.0), Ramp::at(0.0)],
            separated: Ramp::at(0.0),
            settle_presence: true,
        }
    }

    pub(crate) fn set_mix(&mut self, mix: StemMix) {
        for (ramp, gain) in self.gains.iter_mut().zip([mix.vocals, mix.drums, mix.bass]) {
            ramp.target = gain.clamp(0.0, 1.0);
        }
        self.other.target = mix.other.clamp(0.0, 1.0);
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
        self.settle_presence |= !mixing;
        let stems =
            std::array::from_fn(|i| self.tracks[i].as_deref().filter(|_| mixing).map(TrackAudio::view));
        Reader {
            track: track.view(),
            stems,
            gains: &mut self.gains,
            other: &mut self.other,
            present: &mut self.present,
            separated: &mut self.separated,
            settle_presence: &mut self.settle_presence,
            last: Cell::new(0),
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
    present: &'a mut [Ramp; 3],
    separated: &'a mut Ramp,
    settle_presence: &'a mut bool,
    /// The last frame read: where the deck plays, for the presence of the stems.
    last: Cell<isize>,
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
        if self.stems.iter().all(Option::is_none) {
            return (l, r);
        }
        self.last.set(i);
        // the other instruments at their gain where stems are, the whole track where none is
        let other = 1.0 + self.separated.value * (self.other.value - 1.0);
        let (mut ml, mut mr) = (other * l, other * r);
        for ((stem, gain), present) in self.stems.iter().zip(self.gains.iter()).zip(self.present.iter()) {
            if let Some((sl, sr)) = stem.and_then(|s| s.get(i)) {
                let delta = present.value * (gain.value - other);
                ml += delta * sl;
                mr += delta * sr;
            }
        }
        (ml, mr)
    }

    /// Moves the gains one frame towards their targets, and the presence of the stems
    /// towards what is separated where the deck plays.
    #[inline]
    pub(crate) fn advance(&mut self) {
        if self.stems.iter().all(Option::is_none) {
            return;
        }
        self.gains.iter_mut().for_each(Ramp::step);
        self.other.step();
        let at = self.last.get();
        let settle = std::mem::take(self.settle_presence);
        let mut any = false;
        for (stem, present) in self.stems.iter().zip(self.present.iter_mut()) {
            let there = stem.is_some_and(|s| s.get(at).is_some());
            present.move_to(if there { 1.0 } else { 0.0 }, settle);
            any |= there;
        }
        self.separated.move_to(if any { 1.0 } else { 0.0 }, settle);
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

    /// Aims at `target`, reached at once if `now`, else one step closer.
    #[inline]
    fn move_to(&mut self, target: f32, now: bool) {
        self.target = target;
        if now {
            self.value = target;
        } else {
            self.step();
        }
    }

    #[inline]
    fn step(&mut self) {
        let d = self.target - self.value;
        self.value = if d.abs() <= RAMP_STEP { self.target } else { self.value + RAMP_STEP.copysign(d) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(value: f32, frames: usize) -> Arc<TrackAudio> {
        Arc::new(TrackAudio::from_interleaved(48_000, vec![value; frames * 2]))
    }

    fn settle(stems: &mut Stems, track: &TrackAudio) -> (f32, f32) {
        settle_at(stems, track, 0)
    }

    /// The deck playing frame `i` long enough for every gain and presence to settle.
    fn settle_at(stems: &mut Stems, track: &TrackAudio, i: isize) -> (f32, f32) {
        let mut reader = stems.reader(track);
        for _ in 0..1000 {
            reader.frame(i);
            reader.advance();
        }
        reader.frame(i)
    }

    fn mix(vocals: bool, drums: bool, bass: bool, other: bool) -> StemMix {
        let gain = |on: bool| if on { 1.0 } else { 0.0 };
        StemMix { vocals: gain(vocals), drums: gain(drums), bass: gain(bass), other: gain(other) }
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
        assert_eq!(settle_at(&mut stems, &track, 3), (1.0, 1.0), "nothing separated: the whole track");
        assert_eq!(settle_at(&mut stems, &track, 4), (0.5, 0.5), "the drums not separated yet stay");
        assert_eq!(settle_at(&mut stems, &track, 5), (0.25, 0.25), "both muted");
    }

    #[test]
    fn playing_into_the_separated_part_fades_into_the_mix() {
        let track = constant(1.0, 2000);
        let vocals = Arc::new(TrackAudio::pending(48_000, 2000));
        vocals.publish(1000, &[0.5; 2000]);
        let mut stems = Stems::new();
        stems.set_tracks([Some(vocals), None, None]);
        stems.set_mix(mix(false, true, true, true));
        settle_at(&mut stems, &track, 999);
        let mut reader = stems.reader(&track);
        let mut last = reader.frame(999).0;
        assert_eq!(last, 1.0, "the vocals not separated yet play");
        for i in 1000..2000 {
            let now = reader.frame(i).0;
            assert!((now - last).abs() <= 0.5 * RAMP_STEP + 1e-6, "frame {i}: {last} -> {now}");
            reader.advance();
            last = now;
        }
        assert_eq!(last, 0.5, "then they are muted");
    }

    #[test]
    fn a_stem_turned_down_plays_in_proportion() {
        let (track, mut stems) = separated();
        stems.set_mix(StemMix { vocals: 0.5, ..StemMix::default() });
        assert_eq!(settle(&mut stems, &track), (0.75, 0.75));
        stems.set_mix(StemMix { other: 0.0, drums: 0.5, ..StemMix::default() });
        assert_eq!(settle(&mut stems, &track), (0.5 + 0.125 + 0.125, 0.5 + 0.125 + 0.125));
    }

    #[test]
    fn a_toggle_fades_instead_of_jumping() {
        let (track, mut stems) = separated();
        settle(&mut stems, &track);
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
