//! A deck's sampler: short sounds on pads, played through the deck's channel (its EQ,
//! effects and fader, or its own outputs to a hardware mixer). A trigger waits for the
//! next beat when the deck plays on its grid with quantize on.

use crate::mixer::Ramp;
use prisme_media::TrackAudio;
use std::sync::Arc;

pub const SAMPLE_SLOTS: usize = 16;

#[derive(Default)]
struct Voice {
    sample: Option<Arc<TrackAudio>>,
    /// Source frame being played; `None` when silent.
    position: Option<f64>,
    /// Frames to wait before starting (quantized trigger).
    wait: usize,
    /// Of this sound alone: how hard its pad was hit.
    gain: f32,
}

pub(crate) struct Sampler {
    voices: [Voice; SAMPLE_SLOTS],
    pub(crate) gain: f32,
    /// The gain over the block, gliding to `gain`.
    level: Ramp,
}

impl Sampler {
    pub(crate) fn new() -> Self {
        Self { voices: Default::default(), gain: 1.0, level: Ramp::new(1.0) }
    }

    /// Puts a sound in a slot; returns the one it replaces, or the one given for a slot
    /// that does not exist (to be freed elsewhere either way).
    pub(crate) fn set(&mut self, slot: usize, sample: Option<Arc<TrackAudio>>) -> Option<Arc<TrackAudio>> {
        let Some(voice) = self.voices.get_mut(slot) else { return sample };
        voice.position = None;
        std::mem::replace(&mut voice.sample, sample)
    }

    /// Starts a slot from the beginning after `wait` frames (again if it is playing), at `gain`
    /// times the sampler's level.
    pub(crate) fn trigger(&mut self, slot: usize, wait: usize, gain: f32) {
        if let Some(voice) = self.voices.get_mut(slot).filter(|v| v.sample.is_some()) {
            voice.position = Some(0.0);
            voice.wait = wait;
            voice.gain = gain;
        }
    }

    pub(crate) fn stop(&mut self, slot: usize) {
        if let Some(voice) = self.voices.get_mut(slot) {
            voice.position = None;
        }
    }

    /// Slots playing or about to, as bits.
    pub(crate) fn playing(&self) -> u16 {
        self.voices.iter().enumerate().fold(0, |bits, (i, v)| bits | (u16::from(v.position.is_some()) << i))
    }

    /// Adds the playing sounds to interleaved stereo at `out_rate`.
    pub(crate) fn render(&mut self, out: &mut [f32], out_rate: f64) {
        self.level.next(self.gain);
        let (level, frames) = (self.level, out.len() / 2);
        for voice in &mut self.voices {
            let (Some(sample), Some(mut position)) = (voice.sample.as_ref(), voice.position) else {
                continue;
            };
            let step = sample.sample_rate() as f64 / out_rate;
            let view = sample.view();
            let length = sample.frames() as f64;
            let start = voice.wait.min(frames);
            voice.wait -= start;
            for (at, frame) in out.chunks_exact_mut(2).enumerate().skip(start) {
                if position >= length {
                    break;
                }
                // linear interpolation: the sample plays at its own pitch at any rate
                let i = position.floor();
                let t = (position - i) as f32;
                let (a, b) = (view.frame(i as isize), view.frame(i as isize + 1));
                let gain = level.at(at, frames) * voice.gain;
                frame[0] += (a.0 + (b.0 - a.0) * t) * gain;
                frame[1] += (a.1 + (b.1 - a.1) * t) * gain;
                position += step;
            }
            voice.position = (position < length).then_some(position);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sound(frames: usize) -> Arc<TrackAudio> {
        Arc::new(TrackAudio::from_interleaved(48_000, vec![0.5; frames * 2]))
    }

    #[test]
    fn a_sound_for_a_slot_that_does_not_exist_comes_back_to_be_freed() {
        let mut s = Sampler::new();
        let given = sound(10);
        let back = s.set(SAMPLE_SLOTS, Some(given.clone())).expect("given back");
        assert!(Arc::ptr_eq(&back, &given));
    }

    #[test]
    fn a_trigger_plays_the_sound_once() {
        let mut s = Sampler::new();
        s.set(2, Some(sound(100)));
        s.trigger(2, 0, 1.0);
        assert_eq!(s.playing(), 0b100);
        let mut out = vec![0.0f32; 128 * 2];
        s.render(&mut out, 48_000.0);
        assert!(out[..200].iter().all(|v| (*v - 0.5).abs() < 1e-6), "the sound");
        assert!(out[200..].iter().all(|v| *v == 0.0), "then nothing");
        assert_eq!(s.playing(), 0);
    }

    #[test]
    fn the_last_slot_plays_and_says_so() {
        let mut s = Sampler::new();
        s.set(SAMPLE_SLOTS - 1, Some(sound(100)));
        s.trigger(SAMPLE_SLOTS - 1, 0, 1.0);
        assert_eq!(s.playing(), 1 << 15);
    }

    #[test]
    fn a_quantized_trigger_waits() {
        let mut s = Sampler::new();
        s.set(0, Some(sound(1_000)));
        s.trigger(0, 150, 1.0);
        let mut out = vec![0.0f32; 128 * 2];
        s.render(&mut out, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0), "still waiting");
        let mut out = vec![0.0f32; 128 * 2];
        s.render(&mut out, 48_000.0);
        assert_eq!(out[2 * 21], 0.0);
        assert!((out[2 * 22] - 0.5).abs() < 1e-6, "starts 150 frames after the trigger");
    }

    #[test]
    fn a_soft_hit_plays_its_own_sound_softly_and_no_other() {
        let mut s = Sampler::new();
        s.gain = 0.8;
        // reached over a block
        s.render(&mut [0.0; 64 * 2], 48_000.0);
        s.set(0, Some(sound(100)));
        s.set(1, Some(sound(100)));
        s.trigger(0, 0, 0.25);
        let mut out = vec![0.0f32; 64 * 2];
        s.render(&mut out, 48_000.0);
        assert!((out[0] - 0.5 * 0.8 * 0.25).abs() < 1e-6, "as hard as its pad was hit");
        s.trigger(1, 0, 1.0);
        let mut out = vec![0.0f32; 64 * 2];
        s.render(&mut out, 48_000.0);
        assert!((out[0] - 0.5 * 0.8 * 1.25).abs() < 1e-6, "the next one at the sampler's level");
        assert_eq!(s.gain, 0.8);
    }

    #[test]
    fn its_level_glides_over_a_block_rather_than_click() {
        let mut s = Sampler::new();
        s.set(0, Some(sound(1_000)));
        s.trigger(0, 0, 1.0);
        s.render(&mut [0.0; 64 * 2], 48_000.0);
        s.gain = 0.0;
        let mut out = [0.0f32; 64 * 2];
        s.render(&mut out, 48_000.0);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        assert!(left[0] > 0.49, "from where it was: {}", left[0]);
        assert!(left.windows(2).all(|w| w[0] - w[1] < 0.01 && w[1] <= w[0]), "{left:?}");
        assert!(left[63] < 0.01);
    }

    #[test]
    fn empty_slots_do_nothing_and_stop_stops() {
        let mut s = Sampler::new();
        s.trigger(1, 0, 1.0);
        assert_eq!(s.playing(), 0);
        s.set(1, Some(sound(10_000)));
        s.trigger(1, 0, 1.0);
        s.stop(1);
        assert_eq!(s.playing(), 0);
        assert!(s.set(1, None).is_some(), "the old sound is handed back");
    }
}
