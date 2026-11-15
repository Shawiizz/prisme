//! What comes in live through the sound card besides the turntables: a microphone and an aux
//! line, mixed into the master of the software mixer. While the microphone is on, talkover
//! ducks the music under the voice.

use prisme_dsp::{db_to_gain, Smoothed};

/// How fast a live input fades in and out, and follows its level (s).
const LEVEL_GLIDE_S: f32 = 0.02;
/// Talkover: how fast the music ducks as the microphone opens, and comes back after it (s).
const DUCK_S: f32 = 0.1;
const RETURN_S: f32 = 0.6;
/// How far the music ducks until told otherwise (dB).
const DEFAULT_DEPTH_DB: f32 = -18.0;
/// The ducked gain is where it goes this close to it (0.01 dB from full level).
const SETTLED: f32 = 1e-3;

/// The input channels a live source comes in on: one, heard in the middle, or a stereo pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputChannels {
    /// The first (0-based).
    pub first: usize,
    pub stereo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveSource {
    Mic,
    Aux,
}

pub(crate) struct LiveInput {
    channels: Option<InputChannels>,
    level: f32,
    on: bool,
    cue: bool,
    /// Gain into the master: the level while on.
    main: Smoothed,
    /// Gain into the headphones: the level while cued, on or not (to check it before opening it).
    cued: Smoothed,
    audio: Vec<f32>,
    /// Loudest sample at its level since the last status.
    pub(crate) peak: f32,
}

impl LiveInput {
    pub(crate) fn new(max_block_frames: usize, sample_rate: f32) -> Self {
        Self {
            channels: None,
            level: 1.0,
            on: false,
            cue: false,
            main: Smoothed::new(0.0, LEVEL_GLIDE_S, sample_rate),
            cued: Smoothed::new(0.0, LEVEL_GLIDE_S, sample_rate),
            audio: vec![0.0; max_block_frames * 2],
            peak: 0.0,
        }
    }

    pub(crate) fn set_channels(&mut self, channels: Option<InputChannels>) {
        self.channels = channels;
    }

    pub(crate) fn set_level(&mut self, gain: f32) {
        self.level = gain.max(0.0);
        self.follow();
    }

    pub(crate) fn set_on(&mut self, on: bool) {
        self.on = on;
        self.follow();
    }

    pub(crate) fn set_cue(&mut self, cue: bool) {
        self.cue = cue;
        self.follow();
    }

    fn follow(&mut self) {
        self.main.set(if self.on { self.level } else { 0.0 });
        self.cued.set(if self.cue { self.level } else { 0.0 });
    }

    /// Heard in the master: on, with channels to come in on.
    pub(crate) fn is_on(&self) -> bool {
        self.on && self.channels.is_some()
    }

    /// Takes a block of `frames` out of the interleaved input, from frame `offset` (silence
    /// where its channels are absent).
    pub(crate) fn read(&mut self, input: &[f32], in_ch: usize, offset: usize, frames: usize) {
        let audio = &mut self.audio[..frames * 2];
        let Some(channels) = self.channels else {
            audio.fill(0.0);
            return;
        };
        let right = usize::from(channels.stereo);
        let mut peak = 0.0f32;
        for (i, frame) in audio.chunks_exact_mut(2).enumerate() {
            let k = (offset + i) * in_ch + channels.first;
            let present = channels.first + right < in_ch && k + right < input.len();
            let (l, r) = if present { (input[k], input[k + right]) } else { (0.0, 0.0) };
            frame[0] = l;
            frame[1] = r;
            peak = peak.max(l.abs()).max(r.abs());
        }
        self.peak = self.peak.max(peak * self.level);
    }

    /// Adds the block read into the master bus.
    pub(crate) fn mix_into(&mut self, bus: &mut [f32]) {
        add_gliding(&self.audio[..bus.len()], bus, &mut self.main);
    }

    /// Adds the block read into the headphones' bus.
    pub(crate) fn cue_into(&mut self, bus: &mut [f32]) {
        add_gliding(&self.audio[..bus.len()], bus, &mut self.cued);
    }
}

fn add_gliding(audio: &[f32], bus: &mut [f32], gain: &mut Smoothed) {
    if gain.is_settled() && gain.target() == 0.0 {
        return;
    }
    for (b, a) in bus.chunks_exact_mut(2).zip(audio.chunks_exact(2)) {
        let g = gain.tick();
        b[0] += a[0] * g;
        b[1] += a[1] * g;
    }
}

/// The music ducking under the microphone.
pub(crate) struct Talkover {
    pub(crate) enabled: bool,
    /// Gain of the music while the microphone is on.
    pub(crate) depth: f32,
    gain: f32,
    down: f32,
    up: f32,
}

impl Talkover {
    pub(crate) fn new(sample_rate: f32) -> Self {
        // reaches 99 % of the way in the time given, as `Smoothed` does
        let coef = |time_s: f32| 1.0 - (-4.6 / (time_s * sample_rate)).exp();
        Self {
            enabled: false,
            depth: db_to_gain(DEFAULT_DEPTH_DB),
            gain: 1.0,
            down: coef(DUCK_S),
            up: coef(RETURN_S),
        }
    }

    /// Ducks `music` while the microphone is on, brings it back after.
    pub(crate) fn duck(&mut self, music: &mut [f32], mic_on: bool) {
        let target = if self.enabled && mic_on { self.depth.clamp(0.0, 1.0) } else { 1.0 };
        if self.gain == 1.0 && target == 1.0 {
            return;
        }
        let coef = if target < self.gain { self.down } else { self.up };
        for frame in music.chunks_exact_mut(2) {
            self.gain += (target - self.gain) * coef;
            // closer, the slow return's steps fall below the precision of an `f32` near 1
            if (self.gain - target).abs() < SETTLED {
                self.gain = target;
            }
            frame[0] *= self.gain;
            frame[1] *= self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    #[test]
    fn a_mono_input_is_heard_in_the_middle_a_pair_as_it_is() {
        let input = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let mut live = LiveInput::new(2, RATE);
        live.set_channels(Some(InputChannels { first: 2, stereo: false }));
        live.read(&input, 4, 0, 2);
        assert_eq!(live.audio, [0.3, 0.3, 0.7, 0.7]);
        live.set_channels(Some(InputChannels { first: 2, stereo: true }));
        live.read(&input, 4, 0, 2);
        assert_eq!(live.audio, [0.3, 0.4, 0.7, 0.8]);
        live.set_channels(Some(InputChannels { first: 3, stereo: true }));
        live.read(&input, 4, 0, 2);
        assert_eq!(live.audio, [0.0; 4], "a pair cut by the channel count is silence");
    }

    #[test]
    fn it_fades_in_and_out_without_a_click() {
        let mut live = LiveInput::new(4_800, RATE);
        live.set_channels(Some(InputChannels { first: 0, stereo: true }));
        live.read(&[1.0; 9_600], 2, 0, 4_800);
        live.set_on(true);
        let mut bus = vec![0.0; 9_600];
        live.mix_into(&mut bus);
        assert!(bus[0] < 0.05, "no jump: {}", bus[0]);
        assert!((bus[9_598] - 1.0).abs() < 1e-3);
        live.set_on(false);
        let mut bus = vec![0.0; 9_600];
        live.mix_into(&mut bus);
        assert!(bus[0] > 0.95 && bus[9_598] < 1e-3);
    }

    #[test]
    fn the_music_ducks_under_the_microphone_and_comes_back() {
        let mut talkover = Talkover::new(RATE);
        talkover.enabled = true;
        talkover.depth = 0.25;
        let block = |talkover: &mut Talkover, mic_on: bool| {
            let mut music = vec![1.0; 9_600];
            talkover.duck(&mut music, mic_on);
            music
        };
        let ducked = block(&mut talkover, true);
        assert!(ducked[0] > 0.9, "smoothly");
        let ducked = block(&mut talkover, true);
        assert!((ducked[9_598] - 0.25).abs() < 1e-3, "{}", ducked[9_598]);
        let back = block(&mut talkover, false);
        assert!(back[0] < 0.3, "smoothly");
        let back = (0..10).map(|_| block(&mut talkover, false)).last().unwrap();
        assert!((back[9_598] - 1.0).abs() < 1e-3, "{}", back[9_598]);

        talkover.enabled = false;
        assert_eq!(block(&mut talkover, true)[0], 1.0, "talkover off: the music stays");
    }
}
