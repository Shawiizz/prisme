//! Export to rekordbox and to playlist files (see `prisme_import`).

use crate::dto::ExportSummaryDto;
use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::picked;
use prisme_import::export::{list_path, Export};
use prisme_import::{m3u, rekordbox};
use prisme_library::{CrateId, DbError, LibraryDb, SessionId, TrackEntry};
use serde::Deserialize;
use std::io::{BufWriter, Write};
use std::sync::Arc;
use tauri::State;
use tauri_plugin_dialog::FilePath;

type Library<'a> = State<'a, Arc<LibraryService>>;

/// A list of the library: a crate (a smart one with the tracks its rules pick now), the
/// prepare list or a set of the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ExportedList"))]
pub enum ListDto {
    Crate { id: i64 },
    Prepare,
    Session { id: i64 },
}

/// A list of an export to rekordbox, and where it goes there.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ExportList"))]
pub struct ExportListDto {
    pub list: ListDto,
    pub folder: Option<String>,
    pub name: String,
}

/// Writes a list as an M3U playlist (`.m3u8`).
#[tauri::command(async)]
pub fn export_m3u(library: Library<'_>, list: ListDto, path: FilePath) -> AppResult<()> {
    let playlist = m3u::write(&entries(&library.db, list)?);
    picked::create(path)?.write_all(playlist.as_bytes())?;
    Ok(())
}

/// Writes the lists, and every track of the library with `whole_library`, as rekordbox's XML.
#[tauri::command(async)]
pub fn export_rekordbox(
    library: Library<'_>,
    whole_library: bool,
    lists: Vec<ExportListDto>,
    path: FilePath,
) -> AppResult<ExportSummaryDto> {
    let lists = lists
        .into_iter()
        .map(|l| Ok((list_path(l.folder.as_deref(), &l.name), entries(&library.db, l.list)?)))
        .collect::<Result<Vec<_>, DbError>>()?;
    let export = Export::gather(&library.db, whole_library, lists)?;
    let mut file = BufWriter::new(picked::create(path)?);
    rekordbox::write(&export, &mut file)?;
    file.flush()?;
    Ok(ExportSummaryDto { tracks: export.tracks.len(), lists: export.lists.len() })
}

fn entries(db: &LibraryDb, list: ListDto) -> Result<Vec<TrackEntry>, DbError> {
    match list {
        ListDto::Crate { id } => db.crate_tracks(CrateId(id)),
        ListDto::Prepare => db.prepare_list(),
        ListDto::Session { id } => Ok(db
            .history_plays(SessionId(id))?
            .into_iter()
            .map(|p| TrackEntry { path: p.path, record: p.record })
            .collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_are_named_as_the_interface_sends_them() {
        let lists: Vec<ExportListDto> = serde_json::from_str(
            r#"[{"list":{"kind":"crate","id":3},"folder":null,"name":"A"},
                {"list":{"kind":"prepare"},"folder":null,"name":"Prepare"},
                {"list":{"kind":"session","id":7},"folder":"History","name":"2026-11-14 22:00"}]"#,
        )
        .unwrap();
        let kinds: Vec<_> = lists.iter().map(|l| l.list).collect();
        assert_eq!(kinds, [ListDto::Crate { id: 3 }, ListDto::Prepare, ListDto::Session { id: 7 }]);
        assert_eq!(lists[2].folder.as_deref(), Some("History"));
    }
}
