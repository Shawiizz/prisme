//! Real-time DJ engine.
//!
//! * [`create`] builds an [`Engine`] (audio thread) and its [`EngineController`]
//!   (control side), connected by lock-free queues.
//! * Each deck plays a [`prisme_media::TrackAudio`] through a playhead that can move at
//!   any speed and direction, driven either internally (play / pitch) or by a timecode
//!   record ([`VinylMode::Absolute`] or [`VinylMode::Relative`]).
//! * Decks with a beat grid can follow a leader deck's tempo and beats (`sync`).
//! * A deck can keep its pitch when its speed changes (`keylock`), has a chain of beat
//!   effects timed on its tempo and a sampler playing through its channel, and can play its
//!   input as it comes instead of its track (passthrough).
//! * The software mixer gives each deck a channel (trim, EQ with kills, filter, fader),
//!   applies the crossfader, mixes in a microphone and an aux line, and gives the master its
//!   gain, a limiter and a balance, and a copy to the booth.
//! * A track of the library can be heard in the headphones alone before it goes on a deck
//!   (`preview`).
//!
//! Real-time rules: the audio thread never allocates, locks or frees memory. Objects it
//! replaces (tracks, decoders) are handed back to the controller to be freed there.

mod command;
mod deck;
mod engine;
mod grid;
mod keylock;
mod live;
mod looping;
mod mixer;
mod playhead;
mod preview;
mod recording;
mod sampler;
mod scope;
mod status;
mod stems;
mod strip;
mod sync;
mod tempo;
mod vinyl;

pub use command::{Command, CrossfaderSide, DeckId, Garbage, RecordSource, Rpm, VinylMode};
pub use deck::JOG_TURN_S;
pub use engine::{create, Engine, EngineConfig, EngineController};
pub use grid::{BeatGrid, BEATS_PER_BAR};
pub use live::{InputChannels, LiveSource};
pub use looping::LoopRegion;
pub use mixer::{side_gain, CrossfaderCurve, MixMode, OutputRouting};
pub use prisme_dsp::{db_to_gain, gain_to_db, Band, EffectKind, FX_SLOTS, PARAM2_DEFAULT};
pub use sampler::SAMPLE_SLOTS;
pub use status::{
    DeckStatus, EngineStatus, LoopStatus, PreviewStatus, RecordingStatus, SyncStatus, VinylStatus, MAX_DECKS,
};
pub use stems::{Parts, Stem, StemMix, StemTracks};
