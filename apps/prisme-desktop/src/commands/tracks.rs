use crate::dto::{SeparationDto, TrackInfoDto};
use crate::state::{lock, AppState};
use prisme_engine::Stem;
use tauri::ipc::{Channel, Response};
use tauri::State;

/// Sends the waveform of the track on `deck` to `channel` (`dto::waveform_bytes`): what is
/// known of it at once, then, while the track is decoded, what changed of it as it grows. Nothing
/// once another track was loaded.
#[tauri::command]
pub fn watch_waveform(state: State<'_, AppState>, deck: usize, load_id: u64, channel: Channel) {
    let waveform = lock(&state.tracks).get(deck).and_then(|t| {
        let t = t.as_ref().filter(|t| t.load_id == load_id)?;
        Some(t.waveform.same())
    });
    if let Some(waveform) = waveform {
        waveform.watch(channel);
    }
}

/// Where each part of the separated track on `deck` plays (`prisme_analysis::stem_activity`), as
/// its lane shows it (`stem_activity::shown`): the number of parts, then a row for the vocals
/// and the instrumental, or for the vocals, drums, bass and other instruments, one after the
/// other; empty until the track is all separated.
#[tauri::command(async)]
pub fn stem_activity(state: State<'_, AppState>, deck: usize, load_id: u64) -> Response {
    let parts = state.with_loaded(deck, load_id, |t| {
        let stems: &[Stem] = match SeparationDto::of(|s| t.stems[s as usize].is_some()) {
            SeparationDto::None => return None,
            SeparationDto::Vocals => &[Stem::Vocals],
            SeparationDto::Full => &Stem::ALL,
        };
        let stems: Option<Vec<_>> = stems.iter().map(|&s| t.stems[s as usize].clone()).collect();
        Some((t.audio.clone(), stems?))
    });
    let rows = parts.flatten().and_then(|(audio, stems)| {
        prisme_analysis::stem_activity(&audio, &stems.iter().map(|s| &**s).collect::<Vec<_>>())
    });
    let shown: Option<Vec<u8>> = rows.map(|rows| {
        let lanes = rows.iter().flatten().map(|&level| prisme_analysis::stem_activity::shown(level));
        std::iter::once(rows.len() as u8).chain(lanes).collect()
    });
    Response::new(shown.unwrap_or_default())
}

/// Tracks currently loaded on the decks (to rebuild the UI after a reload).
#[tauri::command]
pub fn loaded_tracks(state: State<'_, AppState>) -> Vec<Option<TrackInfoDto>> {
    lock(&state.tracks).iter().enumerate().map(|(deck, t)| t.as_ref().map(|t| t.info(deck))).collect()
}
