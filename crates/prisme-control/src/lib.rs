//! MIDI controllers: what their messages say, and which of the application's actions
//! each control drives.
//!
//! The actions are only names here (`target`): the application performs them. This
//! crate turns a control's messages into a value for its action, and the state of an
//! action back into the message that lights the control.

mod mapping;
mod message;

pub use mapping::{light, Encoding, Mapping, Mappings};
pub use message::{parse, Control, ControlKind, Event};
