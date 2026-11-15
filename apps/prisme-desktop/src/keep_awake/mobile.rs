//! Android and iOS: nothing to hold here. On Android the activity keeps the screen on while a
//! deck plays, as the page tells it (`MainActivity.kt`).

/// A phone runs the application in front at full speed.
pub fn run_at_full_speed() -> bool {
    true
}
