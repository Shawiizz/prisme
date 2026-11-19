//! Reading a track at any speed, like a needle on a record.

use crate::keylock::KeyLock;
use crate::looping::LoopRegion;
use crate::stems::{Parts, StemMix, StemTracks, Stems};
use prisme_media::TrackAudio;
use std::sync::Arc;

/// A read position in a track and the interpolation to read between samples.
pub(crate) struct Playhead {
    track: Option<Arc<TrackAudio>>,
    /// Position in source frames.
    position: f64,
    /// Active loop, in source frames.
    loop_frames: Option<(f64, f64)>,
    /// At the end, the track goes on from its start.
    repeat: bool,
    stems: Stems,
}

impl Playhead {
    pub(crate) fn new() -> Self {
        Self { track: None, position: 0.0, loop_frames: None, repeat: false, stems: Stems::new() }
    }

    /// Loads a track at its start and returns the previous one (to be freed elsewhere).
    pub(crate) fn load(&mut self, track: Arc<TrackAudio>) -> Option<Arc<TrackAudio>> {
        self.position = 0.0;
        self.loop_frames = None;
        self.track.replace(track)
    }

    pub(crate) fn eject(&mut self) -> Option<Arc<TrackAudio>> {
        self.position = 0.0;
        self.loop_frames = None;
        self.track.take()
    }

    /// Sets the stems of the loaded track; returns the previous ones (to be freed elsewhere).
    pub(crate) fn set_stems(&mut self, stems: StemTracks) -> StemTracks {
        self.stems.set_tracks(stems)
    }

    /// Takes the stems away and plays the whole track again, before another track is loaded.
    pub(crate) fn take_stems(&mut self) -> StemTracks {
        self.stems.clear()
    }

    /// The parts of the track separated where it plays ([`Stems::separated_at`]).
    pub(crate) fn stems_here(&self) -> u8 {
        self.stems.separated_at(self.position as isize)
    }

    pub(crate) fn set_stem_mix(&mut self, mix: StemMix) {
        self.stems.set_mix(mix);
    }

    /// Renders `parts` apart from the rest, where `render` is given room for them.
    pub(crate) fn set_apart(&mut self, parts: Parts) {
        self.stems.set_apart(parts);
    }

    /// Makes the playhead wrap around `region` (`None`: no loop).
    pub(crate) fn set_loop(&mut self, region: Option<LoopRegion>) {
        let rate = self.sample_rate();
        self.loop_frames = region.filter(|r| r.length_s() > 0.0).map(|r| (r.start_s * rate, r.end_s * rate));
    }

    /// At the end, the track goes on from its start, seamlessly.
    pub(crate) fn set_repeat(&mut self, repeat: bool) {
        self.repeat = repeat;
    }

    /// The loop the playhead wraps around: the one set, else with repeat the whole track.
    fn wrap_frames(&self) -> Option<(f64, f64)> {
        let whole = self.track.as_ref().filter(|_| self.repeat).map(|t| (0.0, t.frames() as f64));
        self.loop_frames.or(whole)
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.track.is_some()
    }

    pub(crate) fn sample_rate(&self) -> f64 {
        self.track.as_ref().map_or(44_100.0, |t| t.sample_rate() as f64)
    }

    pub(crate) fn duration_s(&self) -> f64 {
        self.track.as_ref().map_or(0.0, |t| t.duration_s())
    }

    pub(crate) fn position_s(&self) -> f64 {
        self.position / self.sample_rate()
    }

    pub(crate) fn seek_s(&mut self, seconds: f64) {
        self.position = seconds * self.sample_rate();
    }

    /// Renders interleaved stereo into `out` while moving at `speed` (1.0 = original
    /// tempo) at the output rate `out_rate`; with `apart`, the parts set apart go there and
    /// the rest to `out`. Uses 4-point Catmull-Rom interpolation; outside the track it plays
    /// silence (the needle can go before the start).
    pub(crate) fn render(&mut self, out: &mut [f32], apart: Option<&mut [f32]>, out_rate: f64, speed: f64) {
        self.render_with(out, apart, out_rate, || speed);
    }

    /// Like [`Playhead::render`], at the speed `speed` gives for each frame in turn: a speed
    /// changing within the block, as a turntable's motor brings it up or down.
    pub(crate) fn render_with(
        &mut self,
        out: &mut [f32],
        mut apart: Option<&mut [f32]>,
        out_rate: f64,
        mut speed: impl FnMut() -> f64,
    ) {
        let Some(track) = self.track.as_ref() else {
            out.fill(0.0);
            if let Some(apart) = apart {
                apart.fill(0.0);
            }
            // keeps moving, harmless
            for _ in 0..out.len() / 2 {
                self.position += speed();
            }
            return;
        };
        let ratio = track.sample_rate() as f64 / out_rate;
        let wrap = self.wrap_frames();
        let mut source = self.stems.reader(track);
        for (n, frame) in out.chunks_exact_mut(2).enumerate() {
            let i = self.position.floor();
            let t = (self.position - i) as f32;
            let i = i as isize;
            match apart.as_deref_mut() {
                None => {
                    let [p0, p1, p2, p3] = [-1, 0, 1, 2].map(|d| source.frame(i + d));
                    frame[0] = catmull_rom(p0.0, p1.0, p2.0, p3.0, t);
                    frame[1] = catmull_rom(p0.1, p1.1, p2.1, p3.1, t);
                }
                Some(apart) => {
                    let [p0, p1, p2, p3] = [-1, 0, 1, 2].map(|d| source.frame_apart(i + d));
                    let [l, r, al, ar] = std::array::from_fn(|c| catmull_rom(p0[c], p1[c], p2[c], p3[c], t));
                    (frame[0], frame[1]) = (l, r);
                    (apart[2 * n], apart[2 * n + 1]) = (al, ar);
                }
            }
            source.advance();
            self.position = wrap_in_loop(self.position, self.position + speed() * ratio, wrap);
        }
    }

    /// Like [`Playhead::render`], through the key lock: the pitch stays.
    pub(crate) fn render_key_locked(
        &mut self,
        key_lock: &mut KeyLock,
        out: &mut [f32],
        apart: Option<&mut [f32]>,
        out_rate: f64,
        speed: f64,
    ) {
        let wrap = self.wrap_frames();
        let Some(track) = self.track.as_ref() else { return self.render(out, apart, out_rate, speed) };
        let mut source = self.stems.reader(track);
        self.position = key_lock.render(&mut source, self.position, wrap, out, apart, speed);
    }
}

/// Wraps when the playhead crosses an edge of the loop from inside, in either
/// direction, as many times round as it went; a playhead outside the loop is left alone.
#[inline]
pub(crate) fn wrap_in_loop(before: f64, after: f64, loop_frames: Option<(f64, f64)>) -> f64 {
    match loop_frames {
        Some((start, end)) if (start..end).contains(&before) && !(start..end).contains(&after) => {
            start + (after - start).rem_euclid(end - start)
        }
        _ => after,
    }
}

#[inline]
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    0.5 * (2.0 * p1
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: usize) -> Arc<TrackAudio> {
        let samples = (0..frames).flat_map(|i| [i as f32, -(i as f32)]).collect();
        Arc::new(TrackAudio::from_interleaved(48_000, samples))
    }

    #[test]
    fn plays_at_original_speed_and_interpolates() {
        let mut p = Playhead::new();
        p.load(ramp(100));
        p.seek_s(10.0 / 48_000.0);
        let mut out = [0.0f32; 8];
        p.render(&mut out, None, 48_000.0, 0.5);
        // a linear ramp is reproduced exactly by Catmull-Rom
        assert_eq!(out, [10.0, -10.0, 10.5, -10.5, 11.0, -11.0, 11.5, -11.5]);
        assert!((p.position_s() * 48_000.0 - 12.0).abs() < 1e-9);
    }

    #[test]
    fn the_speed_can_change_at_every_frame() {
        let mut p = Playhead::new();
        p.load(ramp(100));
        p.seek_s(10.0 / 48_000.0);
        let mut out = [0.0f32; 8];
        let mut speeds = [0.0, 1.0, 2.0, 3.0].into_iter();
        p.render_with(&mut out, None, 48_000.0, || speeds.next().unwrap());
        assert_eq!([out[0], out[2], out[4], out[6]], [10.0, 10.0, 11.0, 13.0]);
        assert!((p.position_s() * 48_000.0 - 16.0).abs() < 1e-9);
    }

    #[test]
    fn wraps_around_the_loop_both_ways() {
        let mut p = Playhead::new();
        p.load(ramp(48_000));
        p.set_loop(Some(LoopRegion { start_s: 10.0 / 48_000.0, end_s: 20.0 / 48_000.0 }));
        p.seek_s(18.0 / 48_000.0);
        let mut out = [0.0f32; 8];
        p.render(&mut out, None, 48_000.0, 1.0);
        assert_eq!([out[0], out[2], out[4], out[6]], [18.0, 19.0, 10.0, 11.0]);

        p.seek_s(11.0 / 48_000.0);
        p.render(&mut out, None, 48_000.0, -1.0);
        assert_eq!([out[0], out[2], out[4], out[6]], [11.0, 10.0, 19.0, 18.0]);
    }

    #[test]
    fn with_repeat_the_end_goes_on_from_the_start() {
        let mut p = Playhead::new();
        p.load(ramp(100));
        p.set_repeat(true);
        p.seek_s(98.0 / 48_000.0);
        let mut out = [0.0f32; 8];
        p.render(&mut out, None, 48_000.0, 1.0);
        assert_eq!([out[0], out[2], out[4], out[6]], [98.0, 99.0, 0.0, 1.0]);
        p.set_repeat(false);
        p.seek_s(98.0 / 48_000.0);
        p.render(&mut out, None, 48_000.0, 1.0);
        assert_eq!([out[4], out[6]], [0.0, 0.0], "silence past the end");
        assert!(p.position_s() * 48_000.0 > 100.0);
    }

    #[test]
    fn a_step_longer_than_the_loop_goes_round_it_as_many_times() {
        let wrapped = |before, after| wrap_in_loop(before, after, Some((10.0, 20.0)));
        assert_eq!(wrapped(15.0, 37.0), 17.0);
        assert_eq!(wrapped(15.0, -7.0), 13.0);
        assert_eq!(wrapped(19.0, 20.0), 10.0);
    }

    #[test]
    fn a_playhead_outside_the_loop_is_not_pulled_in() {
        let mut p = Playhead::new();
        p.load(ramp(100));
        p.set_loop(Some(LoopRegion { start_s: 10.0 / 48_000.0, end_s: 20.0 / 48_000.0 }));
        p.seek_s(30.0 / 48_000.0);
        let mut out = [0.0f32; 4];
        p.render(&mut out, None, 48_000.0, 1.0);
        assert_eq!([out[0], out[2]], [30.0, 31.0]);
    }

    #[test]
    fn plays_backwards_and_silence_outside() {
        let mut p = Playhead::new();
        p.load(ramp(10));
        let mut out = [0.0f32; 4];
        p.render(&mut out, None, 48_000.0, -1.0);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[2], 0.0, "before the start of the track is silence");
    }
}
