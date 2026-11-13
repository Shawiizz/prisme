use crate::dto::{HistoryPlayDto, HistorySessionDto};
use crate::error::AppResult;
use crate::library_service::LibraryService;
use prisme_library::{HistoryPlay, SessionId};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

type Library<'a> = State<'a, Arc<LibraryService>>;

#[tauri::command(async)]
pub fn history_sessions(library: Library<'_>) -> AppResult<Vec<HistorySessionDto>> {
    Ok(library.db.history_sessions()?.into_iter().map(HistorySessionDto::from).collect())
}

#[tauri::command(async)]
pub fn history_plays(library: Library<'_>, id: i64) -> AppResult<Vec<HistoryPlayDto>> {
    Ok(library.db.history_plays(SessionId(id))?.iter().map(HistoryPlayDto::from).collect())
}

#[tauri::command]
pub fn delete_history_session(library: Library<'_>, id: i64) -> AppResult<()> {
    Ok(library.db.delete_history_session(SessionId(id))?)
}

/// Writes the tracklist of a session as CSV (UTF-8 with a BOM, so that Excel reads the
/// accents right).
#[tauri::command(async)]
pub fn export_history(library: Library<'_>, id: i64, path: PathBuf) -> AppResult<()> {
    let plays = library.db.history_plays(SessionId(id))?;
    std::fs::write(path, format!("\u{feff}{}", tracklist_csv(&plays, CsvFormat::of_this_computer())))?;
    Ok(())
}

/// The separators of a CSV file: on Windows those of the regional settings, as Excel writes
/// and reads it (`;` and `,` in French); elsewhere the usual ones, which the spreadsheets
/// there ask about as they open the file.
#[derive(Clone, Copy)]
struct CsvFormat {
    separator: char,
    decimal: char,
}

impl CsvFormat {
    const ENGLISH: Self = Self { separator: ',', decimal: '.' };

    #[cfg(windows)]
    fn of_this_computer() -> Self {
        use windows_sys::Win32::Globalization::{LOCALE_SDECIMAL, LOCALE_SLIST};
        match (locale_char(LOCALE_SLIST), locale_char(LOCALE_SDECIMAL)) {
            (Some(separator), Some(decimal)) if separator != decimal && !"\"\r\n".contains(separator) => {
                Self { separator, decimal }
            }
            _ => Self::ENGLISH,
        }
    }

    #[cfg(not(windows))]
    fn of_this_computer() -> Self {
        Self::ENGLISH
    }
}

/// A setting of the user's regional settings that is a single character.
#[cfg(windows)]
fn locale_char(setting: u32) -> Option<char> {
    use windows_sys::Win32::Globalization::GetLocaleInfoEx;
    let mut text = [0u16; 8];
    // SAFETY: the length given is that of the buffer; no name is the user's locale
    let written = unsafe { GetLocaleInfoEx(std::ptr::null(), setting, text.as_mut_ptr(), text.len() as i32) };
    let text = String::from_utf16(&text[..usize::try_from(written).ok()?.checked_sub(1)?]).ok()?;
    let mut chars = text.chars();
    chars.next().filter(|_| chars.next().is_none())
}

fn tracklist_csv(plays: &[HistoryPlay], format: CsvFormat) -> String {
    let separator = format.separator.to_string();
    let mut csv = ["#", "Started", "Deck", "Artist", "Title", "Album", "BPM", "Key", "File"].join(&separator);
    csv.push_str("\r\n");
    let start = plays.first().map_or(0, |p| p.played_at_ms);
    for (i, p) in plays.iter().enumerate() {
        let record = p.record.as_ref();
        let analysis = record.map(|r| r.effective_analysis());
        let tags = record.map(|r| &r.tags);
        let title = tags.and_then(|t| t.title.clone()).unwrap_or_else(|| file_stem(&p.path));
        let fields = [
            (i + 1).to_string(),
            elapsed((p.played_at_ms - start).max(0)),
            (p.deck + 1).to_string(),
            tags.and_then(|t| t.artist.clone()).unwrap_or_default(),
            title,
            tags.and_then(|t| t.album.clone()).unwrap_or_default(),
            analysis
                .and_then(|a| a.tempo)
                .map(|t| format!("{:.1}", t.bpm).replace('.', &format.decimal.to_string()))
                .unwrap_or_default(),
            analysis.and_then(|a| a.key).map(|k| k.key.camelot()).unwrap_or_default(),
            p.path.clone(),
        ];
        let line: Vec<String> = fields.iter().map(|f| csv_field(f, format.separator)).collect();
        csv.push_str(&line.join(&separator));
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

fn csv_field(value: &str, separator: char) -> String {
    if value.contains([separator, '"', '\n', '\r']) {
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

    fn plays() -> [HistoryPlay; 2] {
        let tags = TrackTags {
            title: Some("Say \"Yes\"".into()),
            artist: Some("A, B".into()),
            ..TrackTags::default()
        };
        let tempo = prisme_analysis::Tempo { bpm: 124.04, first_beat_s: 0.0, confidence: 1.0 };
        let analysis = prisme_analysis::TrackAnalysis { tempo: Some(tempo), key: None };
        let record = TrackRecord::analyzed(
            Path::new("C:/Music/yes.mp3"),
            FileStamp { size: 1, modified_ms: 1 },
            200.0,
            analysis,
            tags,
        );
        [
            HistoryPlay { path: record.path.clone(), deck: 0, played_at_ms: 1_000_000, record: Some(record) },
            HistoryPlay {
                path: "C:/Music/untagged.mp3".into(),
                deck: 1,
                played_at_ms: 4_725_000,
                record: None,
            },
        ]
    }

    #[test]
    fn the_tracklist_lists_the_plays_from_the_start_of_the_set() {
        assert_eq!(
            tracklist_csv(&plays(), CsvFormat::ENGLISH),
            "#,Started,Deck,Artist,Title,Album,BPM,Key,File\r\n\
             1,0:00:00,1,\"A, B\",\"Say \"\"Yes\"\"\",,124.0,,C:/Music/yes.mp3\r\n\
             2,1:02:05,2,,untagged,,,,C:/Music/untagged.mp3\r\n"
        );
    }

    #[test]
    fn a_french_spreadsheet_gets_its_own_separators() {
        let french = CsvFormat { separator: ';', decimal: ',' };
        assert_eq!(
            tracklist_csv(&plays(), french),
            "#;Started;Deck;Artist;Title;Album;BPM;Key;File\r\n\
             1;0:00:00;1;A, B;\"Say \"\"Yes\"\"\";;124,0;;C:/Music/yes.mp3\r\n\
             2;1:02:05;2;;untagged;;;;C:/Music/untagged.mp3\r\n"
        );
    }

    #[test]
    fn the_separators_of_this_computer_are_read() {
        let format = CsvFormat::of_this_computer();
        assert_ne!(format.separator, format.decimal);
    }
}
