//! A deck's sampler: short sounds on pads, played through the deck's channel (its EQ,
//! effects and fader, or its own outputs to a hardware mixer). A trigger waits for the
//! next beat when the deck plays on its grid with quantize on.

use prisme_media::TrackAudio;
use std::sync::Arc;

pub const SAMPLE_SLOTS: usize = 8;

#[derive(Default)]
struct Voice {
    sample: Option<Arc<TrackAudio>>,
    /// Source frame being played; `None` when silent.
    position: Option<f64>,
    /// Frames to wait before starting (quantized trigger).
    wait: usize,
}

#[derive(Default)]
pub(crate) struct Sampler {
    voices: [Voice; SAMPLE_SLOTS],
    pub(crate) gain: f32,
}

impl Sampler {
    pub(crate) fn new() -> Self {
        Self { gain: 1.0, ..Default::default() }
    }

    /// Puts a sound in a slot; returns the one it replaces (to be freed elsewhere).
    pub(crate) fn set(&mut self, slot: usize, sample: Option<Arc<TrackAudio>>) -> Option<Arc<TrackAudio>> {
        let voice = self.voices.get_mut(slot)?;
        voice.position = None;
        std::mem::replace(&mut voice.sample, sample)
    }

    /// Starts a slot from the beginning after `wait` frames (again if it is playing).
    pub(crate) fn trigger(&mut self, slot: usize, wait: usize) {
        if let Some(voice) = self.voices.get_mut(slot).filter(|v| v.sample.is_some()) {
            voice.position = Some(0.0);
            voice.wait = wait;
        }
    }

    pub(crate) fn stop(&mut self, slot: usize) {
        if let Some(voice) = self.voices.get_mut(slot) {
            voice.position = None;
        }
    }

    /// Slots playing or about to, as bits.
    pub(crate) fn playing(&self) -> u8 {
        self.voices.iter().enumerate().fold(0, |bits, (i, v)| bits | (u8::from(v.position.is_some()) << i))
    }

    /// Adds the playing sounds to interleaved stereo at `out_rate`.
    pub(crate) fn render(&mut self, out: &mut [f32], out_rate: f64) {
        for voice in &mut self.voices {
            let (Some(sample), Some(mut position)) = (voice.sample.as_ref(), voice.position) else {
                continue;
            };
            let step = sample.sample_rate() as f64 / out_rate;
            let view = sample.view();
            let length = sample.frames() as f64;
            let start = voice.wait.min(out.len() / 2);
            voice.wait -= start;
            for frame in out.chunks_exact_mut(2).skip(start) {
                if position >= length {
                    break;
                }
                // linear interpolation: the sample plays at its own pitch at any rate
                let i = position.floor();
                let t = (position - i) as f32;
                let (a, b) = (view.frame(i as isize), view.frame(i as isize + 1));
                frame[0] += (a.0 + (b.0 - a.0) * t) * self.gain;
                frame[1] += (a.1 + (b.1 - a.1) * t) * self.gain;
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
    fn a_trigger_plays_the_sound_once() {
        let mut s = Sampler::new();
        s.set(2, Some(sound(100)));
        s.trigger(2, 0);
        assert_eq!(s.playing(), 0b100);
        let mut out = vec![0.0f32; 128 * 2];
        s.render(&mut out, 48_000.0);
        assert!(out[..200].iter().all(|v| (*v - 0.5).abs() < 1e-6), "the sound");
        assert!(out[200..].iter().all(|v| *v == 0.0), "then nothing");
        assert_eq!(s.playing(), 0);
    }

    #[test]
    fn a_quantized_trigger_waits() {
        let mut s = Sampler::new();
        s.set(0, Some(sound(1_000)));
        s.trigger(0, 150);
        let mut out = vec![0.0f32; 128 * 2];
        s.render(&mut out, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0), "still waiting");
        let mut out = vec![0.0f32; 128 * 2];
        s.render(&mut out, 48_000.0);
        assert_eq!(out[2 * 21], 0.0);
        assert!((out[2 * 22] - 0.5).abs() < 1e-6, "starts 150 frames after the trigger");
    }

    #[test]
    fn empty_slots_do_nothing_and_stop_stops() {
        let mut s = Sampler::new();
        s.trigger(1, 0);
        assert_eq!(s.playing(), 0);
        s.set(1, Some(sound(10_000)));
        s.trigger(1, 0);
        s.stop(1);
        assert_eq!(s.playing(), 0);
        assert!(s.set(1, None).is_some(), "the old sound is handed back");
    }
}
