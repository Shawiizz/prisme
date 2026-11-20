//! What the application's actions decide, apart from the engine, the files and the windows
//! they drive: the scales of the knobs, the parts of a separated track, the phase and the keys
//! of two decks, how a control's moves reach its action (soft takeover, encoder steps, faders
//! over a few steps or silent at an end), the keys held of a row, the tempo tapped.

pub mod input;
pub mod key;
pub mod scales;
pub mod stems;
pub mod sync;
