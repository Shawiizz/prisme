//! A report to help understand a problem.

use crate::audio_host::AudioSession;
use crate::controllers::Controllers;
use crate::dto::AudioHealthDto;
use crate::error::AppResult;
use crate::logging;
use crate::midi_clock::MidiClock;
use crate::state::{lock, AppState};
use serde::Serialize;
use std::fmt::Write;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

const LOG_LINES: usize = 400;

/// Writes the versions, the audio setup and its health, the controllers and the end of
/// the log to `path`.
#[tauri::command]
pub fn save_diagnostics(
    app: AppHandle,
    state: State<'_, AppState>,
    controllers: State<'_, Controllers>,
    clock: State<'_, MidiClock>,
    path: PathBuf,
) -> AppResult<()> {
    let mut report = format!(
        "Prisme {} on {} {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    section(&mut report, "Audio settings saved", &state.preferences.get().audio);
    match lock(&state.session).as_ref() {
        Some(AudioSession { settings, info, stats, .. }) => {
            section(&mut report, "Audio running with", settings);
            section(&mut report, "Audio devices", info);
            section(&mut report, "Audio health", &AudioHealthDto::from(&**stats));
        }
        None => report.push_str("\nAudio not running\n"),
    }
    let setup = controllers.setup();
    let _ = writeln!(
        report,
        "\nControllers in use: {:?}\nMappings: {}\nMIDI clock out: {:?}",
        setup.enabled,
        setup.mappings.all().len(),
        clock.output_name()
    );
    let log = app.path().app_log_dir().ok().and_then(|dir| logging::latest_file(&dir));
    match log.and_then(|file| std::fs::read_to_string(&file).ok().map(|text| (file, text))) {
        Some((file, text)) => {
            let _ = write!(report, "\nEnd of {}:\n{}", file.display(), logging::tail(&text, LOG_LINES));
        }
        None => report.push_str("\nNo log file\n"),
    }
    std::fs::write(path, report)?;
    Ok(())
}

fn section(report: &mut String, title: &str, value: &impl Serialize) {
    let json = serde_json::to_string_pretty(value).unwrap_or_else(|e| e.to_string());
    let _ = write!(report, "\n{title}:\n{json}\n");
}
