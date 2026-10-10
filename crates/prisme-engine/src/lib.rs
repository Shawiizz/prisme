//! Real-time DJ engine.
//!
//! * [`create`] builds an [`Engine`] (audio thread) and its [`EngineController`]
//!   (control side), connected by lock-free queues.
//! * Each deck plays a [`prisme_media::TrackAudio`] through a playhead that can move at
//!   any speed and direction, driven either internally (play / pitch) or by a timecode
//!   record ([`VinylMode::Absolute`] or [`VinylMode::Relative`]).
//! * The mixer applies channel volumes, an equal-power crossfader and the master gain.
//!
//! Real-time rules: the audio thread never allocates, locks or frees memory. Objects it
//! replaces (tracks, decoders) are handed back to the controller to be freed there.

mod command;
mod deck;
mod engine;
mod mixer;
mod playhead;
mod status;
mod vinyl;

pub use command::{Command, CrossfaderSide, DeckId, Garbage, VinylMode};
pub use engine::{create, Engine, EngineConfig, EngineController};
pub use status::{DeckStatus, EngineStatus, VinylStatus, MAX_DECKS};
