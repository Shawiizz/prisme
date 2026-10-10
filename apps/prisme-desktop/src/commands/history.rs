use crate::dto::{HistoryPlayDto, HistorySessionDto};
use crate::error::AppResult;
use crate::library_service::LibraryService;
use prisme_library::{HistoryPlay, SessionId};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

type Library<'a> = State<'a, Arc<LibraryService>>;

#[tauri::command]
pub fn history_sessions(library: Library<'_>) -> AppResult<Vec<HistorySessionDto>> {
    Ok(library.db.history_sessions()?.into_iter().map(HistorySessionDto::from).collect())
}

#[tauri::command]
pub fn history_plays(library: Library<'_>, id: i64) -> AppResult<Vec<HistoryPlayDto>> {
    Ok(library.db.history_plays(SessionId(id))?.iter().map(HistoryPlayDto::from).collect())
}

#[tauri::command]
pub fn delete_history_session(library: Library<'_>, id: i64) -> AppResult<()> {
    Ok(library.db.delete_history_session(SessionId(id))?)
}

/// Writes the tracklist of a session as CSV (UTF-8 with a BOM, so that Excel reads the
/// accents right).
#[tauri::command]
pub fn export_history(library: Library<'_>, id: i64, path: PathBuf) -> AppResult<()> {
    let plays = library.db.history_plays(SessionId(id))?;
    std::fs::write(path, format!("\u{feff}{}", tracklist_csv(&plays)))?;
    Ok(())
}

fn tracklist_csv(plays: &[HistoryPlay]) -> String {
    let mut csv = String::from("#,Started,Deck,Artist,Title,Album,BPM,Key,File\r\n");
    let start = plays.first().map_or(0, |p| p.played_at_ms);
    for (i, p) in plays.iter().enumerate() {
        let record = p.record.as_ref();
        let tags = record.map(|r| &r.tags);
        let title = tags.and_then(|t| t.title.clone()).unwrap_or_else(|| file_stem(&p.path));
        let fields = [
            (i + 1).to_string(),
            elapsed((p.played_at_ms - start).max(0)),
            (p.deck + 1).to_string(),
            tags.and_then(|t| t.artist.clone()).unwrap_or_default(),
            title,
            tags.and_then(|t| t.album.clone()).unwrap_or_default(),
            record.and_then(|r| r.analysis.tempo).map(|t| format!("{:.1}", t.bpm)).unwrap_or_default(),
            record.and_then(|r| r.analysis.key).map(|k| k.key.camelot()).unwrap_or_default(),
            p.path.clone(),
        ];
        let line: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        csv.push_str(&line.join(","));
        csv.push_str("\r\n");
    }
    csv
}

/// `h:mm:ss` from the start of the set.
fn elapsed(ms: i64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn file_stem(path: &str) -> String {
    Path::new(path).file_stem().map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned())
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_library::{FileStamp, TrackRecord};
    use prisme_media::TrackTags;

    #[test]
    fn the_tracklist_lists_the_plays_from_the_start_of_the_set() {
        let tags = TrackTags {
            title: Some("Say \"Yes\"".into()),
            artist: Some("A, B".into()),
            ..TrackTags::default()
        };
        let analysis = prisme_analysis::TrackAnalysis { tempo: None, key: None };
        let record = TrackRecord::analyzed(
            Path::new("C:/Music/yes.mp3"),
            FileStamp { size: 1, modified_ms: 1 },
            200.0,
            analysis,
            tags,
        );
        let plays = [
            HistoryPlay { path: record.path.clone(), deck: 0, played_at_ms: 1_000_000, record: Some(record) },
            HistoryPlay {
                path: "C:/Music/untagged.mp3".into(),
                deck: 1,
                played_at_ms: 4_725_000,
                record: None,
            },
        ];
        assert_eq!(
            tracklist_csv(&plays),
            "#,Started,Deck,Artist,Title,Album,BPM,Key,File\r\n\
             1,0:00:00,1,\"A, B\",\"Say \"\"Yes\"\"\",,,,C:/Music/yes.mp3\r\n\
             2,1:02:05,2,,untagged,,,,C:/Music/untagged.mp3\r\n"
        );
    }
}
