//! In debug builds, the command the round trip from the interface to the backend is timed with:
//! one that does nothing, called over CDP (`window.__TAURI_INTERNALS__.invoke('ping')`) as many
//! times as wanted, beside `perform_action` given a light's target, which does nothing either
//! once it has gone through the actions' order and lanes.

/// Answers at once.
#[tauri::command]
pub fn ping() {}
