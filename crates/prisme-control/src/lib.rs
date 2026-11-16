//! MIDI controllers: what their messages say, and which of the application's actions
//! each control drives.
//!
//! The actions are only names here (`target`): the application performs them. This
//! crate turns a control's messages into a value for its action, and the state of an
//! action back into the message that lights the control. A controller's [`Profile`] maps a
//! known model before anything is learnt on it.

mod light;
mod mapping;
mod message;
mod pickup;
mod profile;
mod schema;

pub use light::{light, nearest, LightValue, LightValues, OFF, ON};
pub use mapping::{
    selected_mode, Chord, Encoding, FaderSet, Hold, Mapping, Mappings, Modes, ProfileControl, Reader, Shift,
    Shifts, When, MODE, SHIFT,
};
pub use message::{parse, Control, ControlKind, Event};
pub use pickup::Pickup;
pub use profile::{
    Display, DisplayPart, InitMessage, KeepAlive, LaterMessage, Meter, ModeGroup, ModeSelect, Output,
    Profile, ProfileLight, JOG_STEPS_PER_TURN, PROFILE_FORMAT,
};
pub use schema::{profile_schema, Target, SCHEMA_URL};
