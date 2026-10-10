//! Key lock: the tempo changes, the pitch does not. The track is read at the deck's
//! speed and goes through a time-stretcher (Signalsmith Stretch) that plays it back at
//! the original pitch, or transposed by whole semitones (key shift).
//!
//! The stretcher delays what it is given; it is fed ahead of the playhead by that delay
//! so that what is heard is where the playhead is. After a jump the feed starts again
//! from the new place; in a loop it wraps with the playhead. Out of
//! [`KeyLock::applies`] (scratching, backwards, very slow or fast) the deck plays
//! without it.

use prisme_media::TrackAudio;
use signalsmith_stretch::Stretch;

const MIN_SPEED: f64 = 0.5;
const MAX_SPEED: f64 = 2.0;
/// A feed further than this from where it should be means the playhead jumped.
const JUMP_FRAMES: f64 = 64.0;

pub(crate) struct KeyLock {
    stretch: Stretch,
    /// Interleaved input, sized for the longest block at the highest speed.
    input: Vec<f32>,
    /// Source frame (fractional) of the next input.
    feed: f64,
    engaged: bool,
    key_shift: i8,
    /// Track sample rate and key shift the transposition was set for.
    transposed_for: (u32, i8),
}

impl KeyLock {
    pub(crate) fn new(out_rate: u32, max_block_frames: usize) -> Self {
        let stretch = Stretch::preset_cheaper(2, out_rate);
        let longest = (max_block_frames as f64 * MAX_SPEED * 2.0) as usize + 2;
        let frames = longest.max(stretch.input_latency() * 2);
        Self {
            stretch,
            input: vec![0.0; frames * 2],
            feed: 0.0,
            engaged: false,
            key_shift: 0,
            transposed_for: (out_rate, 0),
        }
    }

    /// Whether the key lock can play at `speed`.
    pub(crate) fn applies(speed: f64) -> bool {
        (MIN_SPEED..=MAX_SPEED).contains(&speed)
    }

    pub(crate) fn key_shift(&self) -> i8 {
        self.key_shift
    }

    pub(crate) fn set_key_shift(&mut self, semitones: i8) {
        self.key_shift = semitones;
    }

    /// Stops following the playhead (the deck plays without the key lock).
    pub(crate) fn disengage(&mut self) {
        self.engaged = false;
    }

    /// Renders `out` from `track` at `speed`, the playhead being at `position` (source
    /// frames) in the loop `loop_frames`; returns where the playhead is afterwards, and
    /// by how much a loop made it jump back.
    pub(crate) fn render(
        &mut self,
        track: &TrackAudio,
        position: f64,
        loop_frames: Option<(f64, f64)>,
        out: &mut [f32],
        out_rate: f64,
        speed: f64,
    ) -> f64 {
        let ratio = track.sample_rate() as f64 / out_rate;
        let transposition = (track.sample_rate(), self.key_shift);
        if transposition != self.transposed_for {
            // the track's samples are played at the output rate: bring its pitch back
            let factor = ratio * 2f64.powf(f64::from(self.key_shift) / 12.0);
            self.stretch.set_transpose_factor(factor as f32, None);
            self.transposed_for = transposition;
        }
        let lead = self.stretch.input_latency() as f64 + self.stretch.output_latency() as f64 * speed * ratio;
        let wanted = position + lead;
        if !self.engaged || (self.feed - wanted).abs() > JUMP_FRAMES {
            self.start_at(track, wanted, loop_frames, speed);
        }
        let out_frames = out.len() / 2;
        let advance = out_frames as f64 * speed * ratio;
        let start = self.feed.floor();
        let frames = ((self.feed + advance).floor() - start) as usize;
        let input = &mut self.input[..frames * 2];
        read(track, start as isize, loop_frames, input);
        self.stretch.process(&*input, &mut *out);
        self.feed += advance;

        let after = crate::playhead::wrap_in_loop(position, position + advance, loop_frames);
        // the playhead wrapped: the feed, ahead of it, wraps with it
        self.feed += after - (position + advance);
        after
    }

    /// Restarts the stretcher with what comes before `feed`.
    fn start_at(&mut self, track: &TrackAudio, feed: f64, loop_frames: Option<(f64, f64)>, speed: f64) {
        self.stretch.reset();
        let preroll = self.stretch.input_latency();
        let input = &mut self.input[..preroll * 2];
        read(track, feed.floor() as isize - preroll as isize, loop_frames, input);
        self.stretch.seek(&*input, speed);
        self.feed = feed.floor();
        self.engaged = true;
    }
}

/// Reads interleaved frames from `start`; frames past the end of an active loop come
/// from its start, as the playhead will play them.
fn read(track: &TrackAudio, start: isize, loop_frames: Option<(f64, f64)>, out: &mut [f32]) {
    let view = track.view();
    for (i, frame) in out.chunks_exact_mut(2).enumerate() {
        let mut index = start + i as isize;
        if let Some((loop_start, loop_end)) = loop_frames {
            let (s, e) = (loop_start as isize, loop_end as isize);
            if e > s && index >= e {
                index = s + (index - s) % (e - s);
            }
        }
        let (l, r) = view.frame(index);
        frame[0] = l;
        frame[1] = r;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const RATE: u32 = 48_000;

    fn sine(freq: f32, seconds: f32) -> TrackAudio {
        let frames = (seconds * RATE as f32) as usize;
        let samples = (0..frames).flat_map(|i| [(TAU * freq * i as f32 / RATE as f32).sin(); 2]).collect();
        TrackAudio::from_interleaved(RATE, samples)
    }

    /// Plays `seconds` at `speed`; returns the left channel and the final position.
    fn play(lock: &mut KeyLock, track: &TrackAudio, speed: f64, seconds: f64) -> (Vec<f32>, f64) {
        let mut position = RATE as f64;
        let mut heard = Vec::new();
        let mut block = vec![0.0f32; 256 * 2];
        for _ in 0..(seconds * RATE as f64 / 256.0) as usize {
            position = lock.render(track, position, None, &mut block, RATE as f64, speed);
            heard.extend(block.chunks_exact(2).map(|f| f[0]));
        }
        (heard, position)
    }

    /// Frequency from the rising zero crossings of the second half.
    fn frequency(signal: &[f32]) -> f64 {
        let half = &signal[signal.len() / 2..];
        let crossings = half.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        crossings as f64 / (half.len() as f64 / RATE as f64)
    }

    #[test]
    fn faster_but_same_pitch() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        let (heard, position) = play(&mut lock, &track, 1.08, 2.0);
        let f = frequency(&heard);
        assert!((f - 440.0).abs() < 3.0, "{f} Hz");
        let travelled = position - RATE as f64;
        assert!((travelled / (2.0 * RATE as f64) - 1.08).abs() < 0.01, "{travelled}");
    }

    #[test]
    fn a_jump_restarts_at_the_new_place() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        play(&mut lock, &track, 1.0, 0.5);
        let mut block = vec![0.0f32; 512];
        let after = lock.render(&track, 4.0 * RATE as f64, None, &mut block, RATE as f64, 1.0);
        assert!((after - (4.0 * RATE as f64 + 256.0)).abs() < 1e-6);
        assert!(
            (lock.feed - after - lock.stretch.input_latency() as f64 - lock.stretch.output_latency() as f64)
                .abs()
                < 2.0
        );
    }

    #[test]
    fn the_feed_wraps_with_the_loop() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        let loop_frames = Some((RATE as f64, RATE as f64 * 1.25));
        let mut position = RATE as f64;
        let mut block = vec![0.0f32; 512];
        for _ in 0..2_000 {
            position = lock.render(&track, position, loop_frames, &mut block, RATE as f64, 1.0);
            assert!((RATE as f64..RATE as f64 * 1.25).contains(&position));
        }
        let lead = lock.stretch.input_latency() as f64 + lock.stretch.output_latency() as f64;
        assert!((lock.feed - (position + lead)).abs() < 2.0, "no restart at each wrap");
    }

    #[test]
    fn a_key_shift_transposes_whatever_the_speed() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        lock.set_key_shift(12);
        let f = frequency(&play(&mut lock, &track, 1.0, 2.0).0);
        assert!((f - 880.0).abs() < 6.0, "{f} Hz");
        lock.set_key_shift(-2);
        let f = frequency(&play(&mut lock, &track, 1.08, 2.0).0);
        assert!((f - 392.0).abs() < 3.0, "{f} Hz");
    }

    #[test]
    fn speeds_it_applies_to() {
        assert!(KeyLock::applies(1.0) && KeyLock::applies(0.92) && KeyLock::applies(1.5));
        assert!(!KeyLock::applies(-1.0) && !KeyLock::applies(0.1) && !KeyLock::applies(3.0));
    }
}
