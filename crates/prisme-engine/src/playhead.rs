//! Reading a track at any speed, like a needle on a record.

use crate::looping::LoopRegion;
use prisme_media::TrackAudio;
use std::sync::Arc;

/// A read position in a track and the interpolation to read between samples.
pub(crate) struct Playhead {
    track: Option<Arc<TrackAudio>>,
    /// Position in source frames.
    position: f64,
    /// Active loop, in source frames.
    loop_frames: Option<(f64, f64)>,
}

impl Playhead {
    pub(crate) fn new() -> Self {
        Self { track: None, position: 0.0, loop_frames: None }
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

    /// Makes the playhead wrap around `region` (`None`: no loop).
    pub(crate) fn set_loop(&mut self, region: Option<LoopRegion>) {
        let rate = self.sample_rate();
        self.loop_frames = region.filter(|r| r.length_s() > 0.0).map(|r| (r.start_s * rate, r.end_s * rate));
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
    /// tempo) at the output rate `out_rate`. Uses 4-point Catmull-Rom interpolation;
    /// outside the track it plays silence (the needle can go before the start).
    pub(crate) fn render(&mut self, out: &mut [f32], out_rate: f64, speed: f64) {
        let Some(track) = self.track.as_ref() else {
            out.fill(0.0);
            self.position += speed * (out.len() / 2) as f64; // keeps moving, harmless
            return;
        };
        let step = speed * track.sample_rate() as f64 / out_rate;
        let view = track.view();
        for frame in out.chunks_exact_mut(2) {
            let i = self.position.floor();
            let t = (self.position - i) as f32;
            let i = i as isize;
            let (l0, r0) = view.frame(i - 1);
            let (l1, r1) = view.frame(i);
            let (l2, r2) = view.frame(i + 1);
            let (l3, r3) = view.frame(i + 2);
            frame[0] = catmull_rom(l0, l1, l2, l3, t);
            frame[1] = catmull_rom(r0, r1, r2, r3, t);
            self.position = wrap_in_loop(self.position, self.position + step, self.loop_frames);
        }
    }
}

/// Wraps when the playhead crosses an edge of the loop from inside, in either
/// direction; a playhead outside the loop is left alone.
#[inline]
fn wrap_in_loop(before: f64, after: f64, loop_frames: Option<(f64, f64)>) -> f64 {
    match loop_frames {
        Some((start, end)) if (start..end).contains(&before) && after >= end => after - (end - start),
        Some((start, end)) if (start..end).contains(&before) && after < start => after + (end - start),
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
        p.render(&mut out, 48_000.0, 0.5);
        // a linear ramp is reproduced exactly by Catmull-Rom
        assert_eq!(out, [10.0, -10.0, 10.5, -10.5, 11.0, -11.0, 11.5, -11.5]);
        assert!((p.position_s() * 48_000.0 - 12.0).abs() < 1e-9);
    }

    #[test]
    fn wraps_around_the_loop_both_ways() {
        let mut p = Playhead::new();
        p.load(ramp(48_000));
        p.set_loop(Some(LoopRegion { start_s: 10.0 / 48_000.0, end_s: 20.0 / 48_000.0 }));
        p.seek_s(18.0 / 48_000.0);
        let mut out = [0.0f32; 8];
        p.render(&mut out, 48_000.0, 1.0);
        assert_eq!([out[0], out[2], out[4], out[6]], [18.0, 19.0, 10.0, 11.0]);

        p.seek_s(11.0 / 48_000.0);
        p.render(&mut out, 48_000.0, -1.0);
        assert_eq!([out[0], out[2], out[4], out[6]], [11.0, 10.0, 19.0, 18.0]);
    }

    #[test]
    fn a_playhead_outside_the_loop_is_not_pulled_in() {
        let mut p = Playhead::new();
        p.load(ramp(100));
        p.set_loop(Some(LoopRegion { start_s: 10.0 / 48_000.0, end_s: 20.0 / 48_000.0 }));
        p.seek_s(30.0 / 48_000.0);
        let mut out = [0.0f32; 4];
        p.render(&mut out, 48_000.0, 1.0);
        assert_eq!([out[0], out[2]], [30.0, 31.0]);
    }

    #[test]
    fn plays_backwards_and_silence_outside() {
        let mut p = Playhead::new();
        p.load(ramp(10));
        let mut out = [0.0f32; 4];
        p.render(&mut out, 48_000.0, -1.0);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[2], 0.0, "before the start of the track is silence");
    }
}
