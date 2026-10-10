//! Real-time DJ engine.
//!
//! * [`create`] builds an [`Engine`] (audio thread) and its [`EngineController`]
//!   (control side), connected by lock-free queues.
//! * Each deck plays a [`prisme_media::TrackAudio`] through a playhead that can move at
//!   any speed and direction, driven either internally (play / pitch) or by a timecode
//!   record ([`VinylMode::Absolute`] or [`VinylMode::Relative`]).
//! * Decks with a beat grid can follow a leader deck's tempo and beats (`sync`).
//! * The mixer applies channel volumes, an equal-power crossfader and the master gain.
//!
//! Real-time rules: the audio thread never allocates, locks or frees memory. Objects it
//! replaces (tracks, decoders) are handed back to the controller to be freed there.

mod command;
mod deck;
mod engine;
mod grid;
mod looping;
mod mixer;
mod playhead;
mod scope;
mod status;
mod sync;
mod vinyl;

pub use command::{Command, CrossfaderSide, DeckId, Garbage, VinylMode};
pub use engine::{create, Engine, EngineConfig, EngineController};
pub use grid::BeatGrid;
pub use looping::LoopRegion;
pub use mixer::{MixMode, OutputRouting};
pub use status::{DeckStatus, EngineStatus, LoopStatus, SyncStatus, VinylStatus, MAX_DECKS};
