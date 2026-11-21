//! The library as it is browsed: its lists, its rows, its folders.

use super::{KeyDto, RowStemsDto};
use prisme_library::browse::{Column, Focus, List, Pick, Scope};
use prisme_library::{CrateId, SessionId};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The list browsed, as the interface names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "TrackSource"))]
pub enum SourceDto {
    Folder { path: String },
    Prepare,
    Duplicates,
    Elsewhere,
    Crate { id: i64 },
    History { id: i64 },
}

impl From<&List> for SourceDto {
    fn from(source: &List) -> Self {
        match source {
            List::Folder(path) => Self::Folder { path: path.display().to_string() },
            List::Prepare => Self::Prepare,
            List::Duplicates => Self::Duplicates,
            List::Elsewhere => Self::Elsewhere,
            List::Crate(id) => Self::Crate { id: id.0 },
            List::History(id) => Self::History { id: id.0 },
        }
    }
}

impl From<SourceDto> for List {
    fn from(source: SourceDto) -> Self {
        match source {
            SourceDto::Folder { path } => Self::Folder(path.into()),
            SourceDto::Prepare => Self::Prepare,
            SourceDto::Duplicates => Self::Duplicates,
            SourceDto::Elsewhere => Self::Elsewhere,
            SourceDto::Crate { id } => Self::Crate(CrateId(id)),
            SourceDto::History { id } => Self::History(SessionId(id)),
        }
    }
}

/// Declares an enum of the interface mirroring one of `prisme_library::browse`, its variants
/// named alike, with the conversions both ways.
macro_rules! mirrored {
    ($(#[$meta:meta])* $name:ident = $of:ty { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub enum $name {
            $($variant),+
        }

        impl From<$of> for $name {
            fn from(value: $of) -> Self {
                match value {
                    $(<$of>::$variant => Self::$variant),+
                }
            }
        }

        impl From<$name> for $of {
            fn from(value: $name) -> Self {
                match value {
                    $($name::$variant => Self::$variant),+
                }
            }
        }
    };
}

mirrored!(
    #[cfg_attr(test, derive(ts_rs::TS), ts(rename = "SearchScope"))]
    ScopeDto = Scope { List, Library }
);

mirrored!(
    #[cfg_attr(test, derive(ts_rs::TS), ts(rename = "SortColumn"))]
    ColumnDto = Column { Order, Title, Artist, Album, Genre, Year, Bpm, Key, Rating, Plays, Mix }
);

mirrored!(
    #[cfg_attr(test, derive(ts_rs::TS), ts(rename = "RowPick"))]
    PickDto = Pick { Only, Toggle, Range, Context }
);

mirrored!(
    #[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LibraryFocus"))]
    FocusDto = Focus { Tracks, Lists }
);

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LibrarySort"))]
pub struct SortDto {
    pub column: ColumnDto,
    pub descending: bool,
}

/// A list of the side panel a controller points at.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Shelf"))]
pub enum ShelfDto {
    Prepare,
    Crate { id: i64 },
}

/// The library as the interface shows it: the list browsed and the rows it draws, those of the
/// window it asked for (`library_viewport`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LibraryView"))]
pub struct LibraryViewDto {
    /// Moved by every change of what is shown and of the window: the highest is the latest.
    pub seq: u64,
    pub source: SourceDto,
    /// The folder shown.
    pub folder: Option<FolderViewDto>,
    pub scope: ScopeDto,
    /// The search applied.
    pub filter: String,
    pub sort: SortDto,
    /// The list has an order of its own, shown in its rows.
    pub ordered: bool,
    /// Its rows can be dragged to another place.
    pub reorderable: bool,
    /// How many rows are shown.
    pub count: usize,
    /// Where the cursor is among the rows shown.
    pub cursor: Option<usize>,
    /// Moved each time the cursor goes to another row (not when rows before it move).
    pub cursor_moves: u64,
    /// Moved each time another list is shown, or the list is searched, sorted or scoped.
    pub listed: u64,
    /// How many tracks the actions on the selection take.
    pub selected: usize,
    pub focus: FocusDto,
    pub pointed: ShelfDto,
    /// The deck the tracks are compared with.
    pub reference: usize,
    /// Why the music folder shown at start could not be read.
    pub unread: Option<crate::error::ErrorDto>,
    pub window: LibraryWindowDto,
}

/// The folder shown: the music folder it is in, the folder above inside it, and the folders in
/// it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "FolderView"))]
pub struct FolderViewDto {
    pub path: String,
    pub root: Option<String>,
    pub up: Option<String>,
    pub folders: Vec<FolderDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Folder"))]
pub struct FolderDto {
    pub name: String,
    pub path: String,
}

impl FolderDto {
    pub fn of(path: &Path) -> Self {
        let name =
            path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        Self { name, path: path.display().to_string() }
    }
}

/// The rows of the window, from its `first`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LibraryWindow"))]
pub struct LibraryWindowDto {
    pub seq: u64,
    pub first: usize,
    pub rows: Vec<LibraryRowDto>,
}

/// A row of the library as it is drawn: its track, what the library knows of it, and how it
/// goes with the rest of the application.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LibraryRow"))]
pub struct LibraryRowDto {
    pub path: String,
    /// When it was played, in a set: a track played twice is two rows.
    pub played_at_ms: Option<i64>,
    /// Its place in its list, from 1, in a list with an order of its own.
    pub position: Option<usize>,
    /// Its title tag, else its file name without the extension.
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u16>,
    pub bpm: Option<f64>,
    pub key: Option<KeyDto>,
    /// 0xRRGGBB
    pub color: Option<u32>,
    /// 0 (not rated) to 5.
    pub rating: u8,
    pub plays: usize,
    /// Its analysis is known (until then its values are waited for).
    pub analysed: bool,
    /// Its file could not be decoded.
    pub failed: bool,
    pub stems: RowStemsDto,
    pub selected: bool,
    /// Played in the set going on.
    pub played: bool,
    /// Under none of the music folders.
    pub elsewhere: bool,
    /// In the headphones' preview, or about to be.
    pub previewing: bool,
    /// The decks holding it.
    pub decks: Vec<DeckMarkDto>,
    /// Its key mixes with the one heard on the deck the tracks are compared with.
    pub fits: bool,
    /// The pitch that brings it to that deck's tempo, in percent, to a tenth.
    pub pitch: Option<f64>,
    /// Whether the tempo fader reaches it.
    pub reachable: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DeckMark"))]
pub struct DeckMarkDto {
    pub deck: usize,
    pub playing: bool,
}

/// The lists the library keeps, as the side panel shows them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LibraryLists"))]
pub struct LibraryListsDto {
    pub crates: Vec<CrateDto>,
    pub sessions: Vec<HistorySessionDto>,
    /// How many tracks the prepare list holds, and the first: the one the Auto DJ mixes in next.
    pub prepare: usize,
    pub next: Option<NamedTrackDto>,
    /// How many tracks of the library are under none of the music folders.
    pub elsewhere: usize,
}

/// A track by its path, its title, and what tells it apart in a list: its tempo, its key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "NamedTrack"))]
pub struct NamedTrackDto {
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<KeyDto>,
}

impl NamedTrackDto {
    pub fn of(track: &prisme_library::catalogue::Track) -> Self {
        let artist = track.record.as_ref().and_then(|r| r.tags.artist.clone());
        let (bpm, key) = (track.bpm, track.key.map(Into::into));
        Self { path: track.path.clone(), title: track.title.clone(), artist, bpm, key }
    }
}

/// What the menu of tracks can do with them: the selection's, or a row's.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LibraryMenu"))]
pub struct LibraryMenuDto {
    /// How many tracks it acts on, and the first.
    pub count: usize,
    pub first: Option<NamedTrackDto>,
    /// They can be taken out of the list shown, this one.
    pub removable: Option<SourceDto>,
    /// Those of them under none of the music folders: all of them, or none.
    pub elsewhere: usize,
    /// The folder they share, all from elsewhere, which may join the music folders unless it
    /// holds one (`holds`).
    pub folder: Option<String>,
    pub holds: Option<String>,
    /// The genre a genre given to them starts with: the one they share, else their folder's name.
    pub genre: String,
    /// How many the installed models have stems to add to.
    pub separable: usize,
}

/// The rules of a smart crate as the DJ typed them, checked before they are kept
/// ([`SmartRulesDto::checked`]).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartRulesDto {
    pub match_all: bool,
    pub rules: Vec<RuleDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuleDto {
    Text { field: prisme_library::TextField, contains: String },
    Bpm { min: Option<f64>, max: Option<f64> },
    Year { min: Option<f64>, max: Option<f64> },
    Key { camelot: Vec<String> },
}

impl SmartRulesDto {
    /// The rules, refused with the first that cannot be: a crate without one, a year not whole,
    /// a key the Camelot wheel does not have.
    pub fn checked(self) -> crate::error::AppResult<prisme_library::SmartRules> {
        use crate::error::AppError;
        use prisme_library::Rule;
        if self.rules.is_empty() {
            return Err(AppError::NoRule);
        }
        let year = |at: usize, year: Option<f64>| -> crate::error::AppResult<Option<u16>> {
            match year {
                None => Ok(None),
                Some(y) if y.fract() == 0.0 && (0.0..=f64::from(u16::MAX)).contains(&y) => Ok(Some(y as u16)),
                Some(_) => Err(AppError::RuleYear(at)),
            }
        };
        let rules = self.rules.into_iter().enumerate().map(|(at, rule)| {
            Ok::<_, AppError>(match rule {
                RuleDto::Text { field, contains } => Rule::Text { field, contains },
                RuleDto::Bpm { min, max } => Rule::Bpm { min, max },
                RuleDto::Year { min, max } => Rule::Year { min: year(at, min)?, max: year(at, max)? },
                RuleDto::Key { camelot } => {
                    let key = |code: String| {
                        prisme_analysis::Key::from_camelot(&code)
                            .map(|key| key.camelot().to_owned())
                            .ok_or(AppError::RuleKey { rule: at, key: code })
                    };
                    Rule::Key { camelot: camelot.into_iter().map(key).collect::<Result<_, _>>()? }
                }
            })
        });
        Ok(prisme_library::SmartRules { match_all: self.match_all, rules: rules.collect::<Result<_, _>>()? })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Crate"))]
pub struct CrateDto {
    pub id: i64,
    pub name: String,
    /// The rules of a smart crate.
    pub rules: Option<prisme_library::SmartRules>,
    pub track_count: usize,
}

/// What an import brought.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ImportSummary"))]
pub struct ImportSummaryDto {
    pub tracks: usize,
    pub cues: usize,
    pub grids: usize,
    pub crates: usize,
    pub missing: usize,
}

impl From<prisme_import::ImportSummary> for ImportSummaryDto {
    fn from(s: prisme_import::ImportSummary) -> Self {
        Self { tracks: s.tracks, cues: s.cues, grids: s.grids, crates: s.crates, missing: s.missing }
    }
}

/// What an export to rekordbox wrote.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ExportSummary"))]
pub struct ExportSummaryDto {
    pub tracks: usize,
    pub lists: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "HistorySession"))]
pub struct HistorySessionDto {
    pub id: i64,
    pub started_at_ms: i64,
    pub play_count: usize,
}

impl From<&prisme_library::HistorySession> for HistorySessionDto {
    fn from(s: &prisme_library::HistorySession) -> Self {
        Self { id: s.id.0, started_at_ms: s.started_at_ms, play_count: s.play_count }
    }
}

impl From<&prisme_library::Crate> for CrateDto {
    fn from(c: &prisme_library::Crate) -> Self {
        Self { id: c.id.0, name: c.name.clone(), rules: c.rules.clone(), track_count: c.track_count }
    }
}

/// How far the analysis of the music folders is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AnalysisProgress"))]
pub struct ProgressDto {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub running: bool,
    /// Held back while a deck plays.
    pub paused: bool,
}

impl From<prisme_library::AnalysisProgress> for ProgressDto {
    fn from(p: prisme_library::AnalysisProgress) -> Self {
        Self { total: p.total, done: p.done, failed: p.failed, running: p.running, paused: p.paused }
    }
}

/// A folder looked into to add it to the music folders: the folders in it, and how many tracks.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DirListing"))]
pub struct DirListingDto {
    pub path: String,
    /// The music folder it is or is in, the deepest.
    pub within: Option<String>,
    pub parent: Option<String>,
    pub folders: Vec<FolderDto>,
    pub tracks: usize,
}

/// The music folders, and how far their analysis is.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "FoldersState"))]
pub struct FoldersStateDto {
    /// The main one first.
    pub folders: Vec<MusicFolderDto>,
    pub progress: ProgressDto,
    /// A rescan the DJ asked for runs.
    pub rescanning: bool,
    /// How many rescans the DJ asked for ended: the library is up to date as one does.
    pub rescans: u64,
    /// The library's database could not be opened: what changes is lost at exit.
    pub in_memory: bool,
}

/// A music folder: the main one, or one added next to it (a USB key, a drive, a sets
/// folder).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "MusicFolder"))]
pub struct MusicFolderDto {
    pub path: String,
    pub main: bool,
    /// The folder is there now: one on a drive unplugged is not, its tracks kept.
    pub present: bool,
}

/// What holds a [`StoragePlaceDto`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum StoragePlaceKind {
    /// The phone's own storage.
    Phone,
    /// A memory card or a USB drive plugged into the phone.
    Removable,
    /// The home folder of the computer.
    Home,
    /// A drive of the computer.
    Drive,
}

/// Where a folder to add to the music folders is looked for first, named by its own folder
/// (`3A2F-1C0B`, `C:`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "StoragePlace"))]
pub struct StoragePlaceDto {
    pub kind: StoragePlaceKind,
    pub name: String,
    pub path: String,
}
