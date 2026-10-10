//! Messages between the control side (UI, controllers) and the audio thread.

use prisme_media::TrackAudio;
use prisme_timecode::TimecodeDecoder;
use std::sync::Arc;

/// Index of a deck (0-based).
pub type DeckId = usize;

/// How a deck's speed and position are driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VinylMode {
    /// The record drives the speed and the absolute position (needle drops move the track).
    #[default]
    Absolute,
    /// The record drives the speed only.
    Relative,
}

/// Which side of the crossfader a deck is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossfaderSide {
    Left,
    #[default]
    Center,
    Right,
}

/// Commands sent to the engine. Building them may allocate; executing them never does.
pub enum Command {
    LoadTrack { deck: DeckId, track: Arc<TrackAudio> },
    Eject { deck: DeckId },
    /// Internal play (used when vinyl control is off).
    SetPlaying { deck: DeckId, playing: bool },
    /// Tempo ratio for internal play (1.0 = original tempo).
    SetPitch { deck: DeckId, ratio: f64 },
    Seek { deck: DeckId, seconds: f64 },
    SetVinylEnabled { deck: DeckId, enabled: bool },
    SetVinylMode { deck: DeckId, mode: VinylMode },
    /// Position of the start of the track on the record, in seconds (lead-in).
    SetLeadIn { deck: DeckId, seconds: f64 },
    /// Replaces a deck's timecode decoder (another format, swapped channels...). Build it
    /// on the control side: it allocates.
    ReplaceDecoder { deck: DeckId, decoder: Box<TimecodeDecoder> },
    SetVolume { deck: DeckId, gain: f32 },
    SetCrossfaderSide { deck: DeckId, side: CrossfaderSide },
    /// -1.0 = full left, 1.0 = full right.
    SetCrossfader { position: f32 },
    SetMasterGain { gain: f32 },
}

/// Objects the audio thread hands back so that they are freed on the control side.
pub enum Garbage {
    Track(Arc<TrackAudio>),
    Decoder(Box<TimecodeDecoder>),
}
