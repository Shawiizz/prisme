//! Controllers, MIDI and HID: what their messages and reports say, and which of the
//! application's actions each control drives.
//!
//! The actions are only names here (`target`): the application performs them. This
//! crate turns a control's messages into a value for its action, and the state of an
//! action back into the message that lights the control. A controller's [`Profile`] maps a
//! known model before anything is learnt on it.

pub mod hid;
mod id;
mod label;
mod light;
mod listed;
mod mapping;
mod message;
mod pickup;
mod profile;
mod schema;
mod source;
mod target;

pub use id::{Id, SourceId, TargetId};
pub use label::{Label, Param};
pub use light::{light, nearest, LightValue, LightValues, OFF, ON};
pub use listed::{beats_text, listed_targets, CUT_BEATS, ROLL_BEATS};
pub use mapping::{
    Chord, Encoding, FaderSet, Hold, Mapping, Mappings, Modes, ProfileControl, Reader, Seen, Shift, Shifts,
    ShownRoom, StepCarry, When, SHIFT,
};
pub use message::{parse, Control, Event, Midi};
pub use pickup::Pickup;
pub use profile::{
    Display, DisplayPart, InitMessage, KeepAlive, LaterMessage, Meter, MeterLed, MeterOn, ModeGroup,
    ModeSelect, Output, Profile, ProfileLight, JOG_STEPS_PER_TURN, PROFILE_FORMAT,
};
pub use schema::{profile_schema, ListedTarget, SCHEMA_URL};
pub use source::{Measure, Source};
pub use target::{
    AutoDjAction, Band, Beats, DeckAction, DeckSwitch, Effect, FxAction, GeneralAction, Kind, LibraryAction,
    MixerAction, MixerSwitch, Owner, SampleAction, Side, StemPart, Target, Turn, Unknown, CUTS, FX_KNOBS,
    FX_SLOTS, HOT_CUES, MAX_DECKS, ROLLS, SAMPLE_SLOTS, SLICES,
};
