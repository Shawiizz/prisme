//! A track's separated vocals beside it, so that the deck can mute either stem. The
//! instrumental being the track minus its vocals, the deck plays
//! `instrumental_gain * track + (vocals_gain - instrumental_gain) * vocals`, mixed as the
//! frames are read: the interpolation and the key lock see a single signal.

use prisme_media::{TrackAudio, TrackView};
use std::sync::Arc;

/// Gains move by this much per frame read: a toggle fades in about 10 ms instead of clicking.
const RAMP_STEP: f32 = 1.0 / 512.0;

/// Which stems a deck plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StemMix {
    pub vocals: bool,
    pub instrumental: bool,
}

impl Default for StemMix {
    fn default() -> Self {
        Self { vocals: true, instrumental: true }
    }
}

pub(crate) struct Stems {
    vocals: Option<Arc<TrackAudio>>,
    instrumental_gain: Ramp,
    vocals_gain: Ramp,
}

impl Stems {
    pub(crate) fn new() -> Self {
        Self { vocals: None, instrumental_gain: Ramp::at(1.0), vocals_gain: Ramp::at(1.0) }
    }

    pub(crate) fn set_mix(&mut self, mix: StemMix) {
        self.instrumental_gain.target = gain(mix.instrumental);
        self.vocals_gain.target = gain(mix.vocals);
    }

    /// Sets the vocals of the loaded track (frame for frame, at its rate) and returns the
    /// previous ones, to be freed elsewhere.
    pub(crate) fn set_vocals(&mut self, vocals: Option<Arc<TrackAudio>>) -> Option<Arc<TrackAudio>> {
        std::mem::replace(&mut self.vocals, vocals)
    }

    /// Forgets the vocals and plays both stems again (a new track is loaded).
    pub(crate) fn clear(&mut self) -> Option<Arc<TrackAudio>> {
        let vocals = self.vocals.take();
        *self = Self::new();
        vocals
    }

    pub(crate) fn reader<'a>(&'a mut self, track: &'a TrackAudio) -> Reader<'a> {
        let vocals = self
            .vocals
            .as_deref()
            .filter(|_| !(self.instrumental_gain.is_one() && self.vocals_gain.is_one()))
            .map(TrackAudio::view);
        Reader {
            track: track.view(),
            vocals,
            instrumental_gain: &mut self.instrumental_gain,
            vocals_gain: &mut self.vocals_gain,
            sample_rate: track.sample_rate(),
        }
    }
}

/// Reads the track with the stems mixed in.
pub(crate) struct Reader<'a> {
    track: TrackView<'a>,
    /// `None` while both stems play fully: the track as it is.
    vocals: Option<TrackView<'a>>,
    instrumental_gain: &'a mut Ramp,
    vocals_gain: &'a mut Ramp,
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
        match &self.vocals {
            None => (l, r),
            Some(vocals) => {
                let (vl, vr) = vocals.frame(i);
                let inst = self.instrumental_gain.value;
                let delta = self.vocals_gain.value - inst;
                (inst * l + delta * vl, inst * r + delta * vr)
            }
        }
    }

    /// Moves the gains one frame towards their targets.
    #[inline]
    pub(crate) fn advance(&mut self) {
        if self.vocals.is_some() {
            self.instrumental_gain.step();
            self.vocals_gain.step();
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

    #[test]
    fn the_stems_mute_their_part_of_the_track() {
        let track = constant(0.75, 10);
        let mut stems = Stems::new();
        stems.set_vocals(Some(constant(0.25, 10)));
        assert_eq!(settle(&mut stems, &track), (0.75, 0.75));
        stems.set_mix(StemMix { vocals: false, instrumental: true });
        assert_eq!(settle(&mut stems, &track), (0.5, 0.5));
        stems.set_mix(StemMix { vocals: true, instrumental: false });
        assert_eq!(settle(&mut stems, &track), (0.25, 0.25));
        stems.set_mix(StemMix { vocals: false, instrumental: false });
        assert_eq!(settle(&mut stems, &track), (0.0, 0.0));
    }

    #[test]
    fn a_toggle_fades_instead_of_jumping() {
        let track = constant(0.75, 10);
        let mut stems = Stems::new();
        stems.set_vocals(Some(constant(0.25, 10)));
        stems.set_mix(StemMix { vocals: true, instrumental: false });
        let mut reader = stems.reader(&track);
        let mut last = reader.frame(0).0;
        for _ in 0..600 {
            reader.advance();
            let now = reader.frame(0).0;
            assert!((now - last).abs() <= 0.5 * RAMP_STEP + 1e-6, "{last} -> {now}");
            last = now;
        }
        assert_eq!(last, 0.25);
    }

    #[test]
    fn without_vocals_the_track_plays_whatever_the_mix() {
        let track = constant(0.75, 10);
        let mut stems = Stems::new();
        stems.set_mix(StemMix { vocals: false, instrumental: true });
        assert_eq!(settle(&mut stems, &track), (0.75, 0.75));
    }

    #[test]
    fn clearing_returns_the_vocals_and_plays_both_stems() {
        let mut stems = Stems::new();
        stems.set_vocals(Some(constant(0.25, 10)));
        stems.set_mix(StemMix { vocals: false, instrumental: true });
        assert!(stems.clear().is_some());
        assert!(stems.vocals.is_none());
        assert!(stems.instrumental_gain.is_one() && stems.vocals_gain.is_one());
    }
}
