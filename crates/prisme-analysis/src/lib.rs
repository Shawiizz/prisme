//! Track analysis, run off the audio thread when a track is loaded.
//!
//! * [`waveform`]: amplitude and low / mid / high energy per time slice, for drawing
//!   colored waveforms.

pub mod waveform;

pub use waveform::{compute_waveform, Waveform};
