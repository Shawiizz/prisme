//! Vinyl control of a deck: reads the record through the timecode decoder and tells the
//! deck how fast to play and, in absolute mode, where it should be.

use crate::command::VinylMode;
use crate::status::VinylStatus;
use prisme_timecode::TimecodeDecoder;

/// What the record asks the deck to do for the block just decoded.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VinylDrive {
    pub(crate) speed: f64,
    /// Where the track should be at the end of the block (absolute mode, once locked).
    pub(crate) target_position_s: Option<f64>,
}

pub(crate) struct VinylControl {
    decoder: Box<TimecodeDecoder>,
    pub(crate) enabled: bool,
    pub(crate) mode: VinylMode,
    /// Position of the track start on the record (seconds).
    pub(crate) lead_in_s: f64,
}

impl VinylControl {
    pub(crate) fn new(decoder: Box<TimecodeDecoder>) -> Self {
        Self { decoder, enabled: false, mode: VinylMode::Absolute, lead_in_s: 0.0 }
    }

    /// Decodes one block of the turntable signal (interleaved stereo).
    pub(crate) fn process(&mut self, input: &[f32]) -> VinylDrive {
        self.decoder.process(input);
        let target_position_s = match self.mode {
            VinylMode::Absolute => self.decoder.position_ms().map(|ms| ms / 1000.0 - self.lead_in_s),
            VinylMode::Relative => None,
        };
        VinylDrive { speed: self.decoder.speed(), target_position_s }
    }

    /// Installs a new decoder and returns the old one (to be freed off the audio thread).
    pub(crate) fn replace_decoder(&mut self, decoder: Box<TimecodeDecoder>) -> Box<TimecodeDecoder> {
        std::mem::replace(&mut self.decoder, decoder)
    }

    pub(crate) fn status(&self) -> VinylStatus {
        let s = self.decoder.status();
        VinylStatus {
            enabled: self.enabled,
            mode: self.mode,
            signal: s.signal,
            record_speed: s.speed,
            record_position_s: s.position_ms.map(|ms| ms / 1000.0),
        }
    }
}
