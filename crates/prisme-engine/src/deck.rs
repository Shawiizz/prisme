//! A deck: a track, a playhead and what drives it (internal play or the record).

use crate::command::{CrossfaderSide, VinylMode};
use crate::playhead::Playhead;
use crate::status::DeckStatus;
use crate::vinyl::VinylControl;
use prisme_timecode::TimecodeDecoder;

/// In absolute mode, a gap larger than this (s) is a needle drop: jump.
const JUMP_THRESHOLD_S: f64 = 0.1;
/// Smaller gaps are closed smoothly by adjusting the speed: correction per second of gap.
const PULL_PER_S: f64 = 5.0;
/// Largest speed correction used to close a gap.
const MAX_CORRECTION: f64 = 0.05;

pub(crate) struct Deck {
    pub(crate) playhead: Playhead,
    pub(crate) playing: bool,
    pub(crate) pitch: f64,
    pub(crate) vinyl: VinylControl,
    pub(crate) volume: f32,
    pub(crate) side: CrossfaderSide,
    correction: f64,
    speed: f64,
}

impl Deck {
    pub(crate) fn new(decoder: Box<TimecodeDecoder>) -> Self {
        Self {
            playhead: Playhead::new(),
            playing: false,
            pitch: 1.0,
            vinyl: VinylControl::new(decoder),
            volume: 1.0,
            side: CrossfaderSide::Center,
            correction: 0.0,
            speed: 0.0,
        }
    }

    /// Processes one block: `input` is the turntable signal of this deck (stereo),
    /// `out` receives the deck audio (stereo, same number of frames).
    pub(crate) fn process(&mut self, input: &[f32], out: &mut [f32], out_rate: f64) {
        // Always decode, so that the signal indicator works even with vinyl control off.
        let drive = self.vinyl.process(input);

        let speed = if self.vinyl.enabled {
            drive.speed + self.correction
        } else if self.playing {
            self.pitch
        } else {
            0.0
        };
        self.playhead.render(out, out_rate, speed);
        self.speed = speed;

        self.correction = 0.0;
        if self.vinyl.enabled && self.vinyl.mode == VinylMode::Absolute && self.playhead.is_loaded() {
            if let Some(target) = drive.target_position_s {
                let gap = target - self.playhead.position_s();
                if gap.abs() > JUMP_THRESHOLD_S {
                    self.playhead.seek_s(target);
                } else {
                    self.correction = (gap * PULL_PER_S).clamp(-MAX_CORRECTION, MAX_CORRECTION);
                }
            }
        }
    }

    pub(crate) fn status(&self) -> DeckStatus {
        DeckStatus {
            loaded: self.playhead.is_loaded(),
            playing: self.playing,
            position_s: self.playhead.position_s(),
            duration_s: self.playhead.duration_s(),
            speed: self.speed,
            vinyl: self.vinyl.status(),
        }
    }
}
