//! A track's separated stems beside it, so that the deck can mute any of them. What no stem
//! holds (the other instruments) is the track minus the stems, so the deck plays
//! `other_gain * track + Σ (stem_gain - other_gain) * stem`, mixed as the frames are read:
//! the interpolation and the key lock see a single signal.
//!
//! A track separated into its vocals alone has no drums nor bass: they stay in the other
//! instruments and follow their gain. So does a stem where it is not separated yet (the
//! stems of a track being separated come as they are made); where none is, the track plays
//! whole. Crossing from one to the other fades in about 10 ms, as a gain does.

use prisme_media::{TrackAudio, TrackView, PENDING_BLOCK};
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

impl StemMix {
    /// The gains of the vocals, drums, bass and other instruments.
    pub(crate) fn parts(self) -> [f32; 4] {
        [self.vocals, self.drums, self.bass, self.other]
    }

    pub(crate) fn from_parts([vocals, drums, bass, other]: [f32; 4]) -> Self {
        Self { vocals, drums, bass, other }
    }
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

    /// The parts separated at `frame`, a bit each: vocals, drums, bass, then the other
    /// instruments, there as soon as any stem is.
    pub(crate) fn separated_at(&self, frame: isize) -> u8 {
        let there =
            |track: &Option<Arc<TrackAudio>>| track.as_ref().is_some_and(|t| t.view().get(frame).is_some());
        let bits =
            self.tracks.iter().enumerate().filter(|(_, t)| there(t)).fold(0, |bits, (i, _)| bits | 1 << i);
        if bits == 0 {
            0
        } else {
            bits | 1 << Stem::ALL.len()
        }
    }

    pub(crate) fn reader<'a>(&'a mut self, track: &'a TrackAudio) -> Reader<'a> {
        let mixing = !(self.other.is_one() && self.gains.iter().all(Ramp::is_one));
        self.settle_presence |= !mixing;
        let stems: [_; 3] =
            std::array::from_fn(|i| self.tracks[i].as_deref().filter(|_| mixing).map(TrackAudio::view));
        Reader {
            track: track.view(),
            mixing: stems.iter().any(Option::is_some),
            stems,
            gains: &mut self.gains,
            other: &mut self.other,
            present: &mut self.present,
            separated: &mut self.separated,
            settle_presence: &mut self.settle_presence,
            last: Cell::new(0),
            seen: None,
            sample_rate: track.sample_rate(),
        }
    }
}

/// Reads the track with the stems mixed in.
pub(crate) struct Reader<'a> {
    track: TrackView<'a>,
    /// All `None` while everything plays fully: the track as it is.
    stems: [Option<TrackView<'a>>; 3],
    /// Some stem is mixed in.
    mixing: bool,
    gains: &'a mut [Ramp; 3],
    other: &'a mut Ramp,
    present: &'a mut [Ramp; 3],
    separated: &'a mut Ramp,
    settle_presence: &'a mut bool,
    /// The last frame read: where the deck plays, for the presence of the stems.
    last: Cell<isize>,
    /// The block of [`PENDING_BLOCK`] frames where the presence of the stems was last looked
    /// at, and which were there: it changes block by block.
    seen: Option<(isize, [bool; 3])>,
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
        if !self.mixing {
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
        if !self.mixing {
            return;
        }
        self.gains.iter_mut().for_each(Ramp::step);
        self.other.step();
        let settle = std::mem::take(self.settle_presence);
        let there = self.there();
        for (present, there) in self.present.iter_mut().zip(there) {
            present.move_to(if there { 1.0 } else { 0.0 }, settle);
        }
        self.separated.move_to(if there.contains(&true) { 1.0 } else { 0.0 }, settle);
    }

    /// Which stems are separated where the deck plays, looked at again in each new block.
    fn there(&mut self) -> [bool; 3] {
        let at = self.last.get();
        let block = at.div_euclid(PENDING_BLOCK as isize);
        match self.seen {
            Some((seen, there)) if seen == block => there,
            _ => {
                let there = self.stems.map(|s| s.is_some_and(|s| s.get(at).is_some()));
                self.seen = Some((block, there));
                there
            }
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
        const B: usize = PENDING_BLOCK;
        let track = constant(1.0, 4 * B);
        // the vocals in two islands, the drums in the second one only
        let vocals = Arc::new(TrackAudio::pending(48_000, 4 * B));
        vocals.publish(B, &vec![0.5; 2 * B]);
        vocals.publish(3 * B, &vec![0.5; 2 * B]);
        let drums = Arc::new(TrackAudio::pending(48_000, 4 * B));
        drums.publish(3 * B, &vec![0.25; 2 * B]);
        let mut stems = Stems::new();
        stems.set_tracks([Some(vocals), Some(drums), None]);
        stems.set_mix(mix(false, false, true, true));
        let at = |i: usize| i as isize;
        assert_eq!(settle_at(&mut stems, &track, at(0)), (1.0, 1.0), "nothing separated: the whole track");
        assert_eq!(settle_at(&mut stems, &track, at(B)), (0.5, 0.5), "the drums not separated yet stay");
        assert_eq!(settle_at(&mut stems, &track, at(2 * B)), (1.0, 1.0), "between the islands");
        assert_eq!(settle_at(&mut stems, &track, at(3 * B)), (0.25, 0.25), "both muted");
    }

    #[test]
    fn the_parts_separated_are_told_where_the_deck_plays() {
        const B: usize = PENDING_BLOCK;
        let vocals = Arc::new(TrackAudio::pending(48_000, 4 * B));
        vocals.publish(B, &vec![0.5; 2 * B]);
        vocals.publish(3 * B, &vec![0.5; 2 * B]);
        let drums = Arc::new(TrackAudio::pending(48_000, 4 * B));
        drums.publish(3 * B, &vec![0.25; 2 * B]);
        let mut stems = Stems::new();
        assert_eq!(stems.separated_at(0), 0, "no stems");
        stems.set_tracks([Some(vocals), Some(drums), None]);
        let at = |i: usize| stems.separated_at(i as isize);
        assert_eq!(at(0), 0, "nothing separated");
        assert_eq!(at(B), 0b1001, "the vocals, and so the rest");
        assert_eq!(at(2 * B), 0, "between the islands");
        assert_eq!(at(3 * B), 0b1011, "the vocals and the drums");
        assert_eq!(stems.separated_at(-1), 0, "before the track");
    }

    #[test]
    fn playing_into_the_separated_part_fades_into_the_mix() {
        const B: usize = PENDING_BLOCK;
        let track = constant(1.0, 4 * B);
        let vocals = Arc::new(TrackAudio::pending(48_000, 4 * B));
        vocals.publish(2 * B, &vec![0.5; 4 * B]);
        let mut stems = Stems::new();
        stems.set_tracks([Some(vocals), None, None]);
        stems.set_mix(mix(false, true, true, true));
        let before = (2 * B - 1) as isize;
        settle_at(&mut stems, &track, before);
        let mut reader = stems.reader(&track);
        let mut last = reader.frame(before).0;
        assert_eq!(last, 1.0, "the vocals not separated yet play");
        for i in 2 * B..4 * B {
            let now = reader.frame(i as isize).0;
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
