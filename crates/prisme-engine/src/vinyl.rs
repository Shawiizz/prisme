//! Vinyl control of a deck: reads the record through the timecode decoder and tells the
//! deck how fast to play and, in absolute mode, where it should be.
//!
//! When the needle reaches the end of the timecode while the record plays, the deck
//! plays on by itself at the record's last speed rather than stop with the music. In
//! relative mode the record takes over again once the needle is back before the end
//! and the platter turns; in absolute mode (where that would jump the track) only once
//! the vinyl control is set again or another track is loaded.

use crate::command::VinylMode;
use crate::status::VinylStatus;
use prisme_timecode::TimecodeDecoder;

/// Slowest forward speed at which reaching the end of the record counts as playing.
const PLAYING_SPEED: f64 = 0.5;
/// A record read backwards at play speed for this long, without a break: its left and
/// right channels are probably crossed (a turntable's reverse switch could do it too, so
/// it only makes the UI ask).
const CROSSED_AFTER_S: f64 = 5.0;
const CROSSED_SPEED: std::ops::RangeInclusive<f64> = -1.15..=-0.85;

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
    run_out: RunOut,
    reversed_frames: usize,
    crossed_after_frames: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RunOut {
    /// The record was not seen before its end since the track was loaded or taken back.
    Unarmed,
    /// Seen before the end: reaching the end while playing makes the deck play on.
    Armed,
    /// Played past the end: the deck plays on by itself at this speed.
    PlayingOn(f64),
}

impl VinylControl {
    pub(crate) fn new(decoder: Box<TimecodeDecoder>, sample_rate: u32) -> Self {
        Self {
            decoder,
            enabled: false,
            mode: VinylMode::Absolute,
            lead_in_s: 0.0,
            run_out: RunOut::Unarmed,
            reversed_frames: 0,
            crossed_after_frames: (CROSSED_AFTER_S * f64::from(sample_rate)) as usize,
        }
    }

    /// Decodes one block of the turntable signal (interleaved stereo).
    pub(crate) fn process(&mut self, input: &[f32]) -> VinylDrive {
        self.decoder.process(input);
        let (speed, position_ms) = (self.decoder.speed(), self.decoder.position_ms());
        self.reversed_frames = if CROSSED_SPEED.contains(&speed) {
            self.reversed_frames.saturating_add(input.len() / 2)
        } else {
            0
        };
        if let Some(speed) = self.run_out(speed, position_ms) {
            return VinylDrive { speed, target_position_s: None };
        }
        let target_position_s = match self.mode {
            VinylMode::Absolute => self.decoder.position_ms().map(|ms| ms / 1000.0 - self.lead_in_s),
            VinylMode::Relative => None,
        };
        VinylDrive { speed: self.decoder.speed(), target_position_s }
    }

    /// The speed to play on at past the end of the record, if the deck does.
    fn run_out(&mut self, speed: f64, position_ms: Option<f64>) -> Option<f64> {
        let format = self.decoder.format();
        let end_ms = f64::from(format.safe) * format.cycle_ms();
        let before_end = position_ms.is_some_and(|p| p < end_ms);
        let past_end = position_ms.is_some_and(|p| p >= end_ms);
        let playing = speed >= PLAYING_SPEED;
        self.run_out = match self.run_out {
            RunOut::PlayingOn(_) if self.mode == VinylMode::Relative && before_end && playing => {
                RunOut::Armed
            }
            RunOut::Unarmed | RunOut::Armed if before_end => RunOut::Armed,
            RunOut::Armed if self.enabled && playing && past_end => RunOut::PlayingOn(speed),
            state => state,
        };
        match self.run_out {
            RunOut::PlayingOn(speed) => Some(speed),
            _ => None,
        }
    }

    /// The record drives the deck again (after the end of the record was reached).
    pub(crate) fn take_back(&mut self) {
        self.run_out = RunOut::Unarmed;
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
            run_out: matches!(self.run_out, RunOut::PlayingOn(_)),
            crossed: self.reversed_frames >= self.crossed_after_frames,
            signal: s.signal,
            level: s.level as f32,
            record_speed: s.speed,
            record_position_s: s.position_ms.map(|ms| ms / 1000.0),
        }
    }
}
