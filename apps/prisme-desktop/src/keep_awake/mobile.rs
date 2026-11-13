//! Android and iOS: the application itself keeps the screen on for as long as it is in
//! front (its activity asks for it), whether the audio runs or not.

pub struct KeepAwake;

impl KeepAwake {
    pub fn new() -> Option<Self> {
        Some(Self)
    }
}

/// A phone runs the application in front at full speed.
pub fn run_at_full_speed() -> bool {
    true
}
