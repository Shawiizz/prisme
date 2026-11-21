//! The library as the DJ browses it, held here whole (`prisme_library::browse::Browser`) and
//! drawn by the interface a window of rows at a time (`dto::LibraryViewDto`): the list shown,
//! read from the database, the folders and the history as it is opened and as they change; the
//! edits of the library, on the tracks they name or on those selected; and the browsing the
//! controllers and the keys do ([`Performing::browse`]).

use super::Performing;
use crate::dto::{
    CrateDto, DeckMarkDto, FolderDto, FolderViewDto, HistorySessionDto, LibraryListsDto, LibraryRowDto,
    LibraryViewDto, LibraryWindowDto, NamedTrackDto, RowStemsDto, SeparationDto, ShelfDto, SortDto,
    SourceDto,
};
use crate::error::{AppError, AppResult, ErrorDto};
use crate::folders;
use crate::state::{lock, AppState};
use crate::stems_service::stored_separation;
use crate::stems_service::{self, Queued, StemsService};
use prisme_actions::input::Given;
use prisme_actions::view::View;
use prisme_control::LibraryAction;
use prisme_library::browse::{Browser, Column, Entry, Focus, Pick, Shelf, Source, PAGE};
use prisme_library::catalogue::{Track, TrackId, Tracks};
use prisme_library::mix::{Compare, MixReference, Pitch};
use prisme_library::{adopt, CrateId, LibraryDb, Search, SessionId, SmartRules, TrackRecord};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::Manager;
use tracing::{error, warn};

/// A set that started longer ago than this is over: its tracks are no longer played tonight.
const TONIGHT_MS: i64 = 12 * 3600 * 1000;

/// The library as browsed, with what its rows show beside the browser: the folders in the folder
/// shown, how far each track listed is separated, and what they show of the decks.
#[derive(Default)]
pub struct Library {
    pub browser: Browser,
    pub folders: Vec<PathBuf>,
    separations: HashMap<TrackId, SeparationDto>,
    /// As the clock last read it (see [`Around`]).
    pub around: Around,
    /// Why the music folder shown at start could not be read, until a list is read.
    unread: Option<ErrorDto>,
    /// When the interface asked for the window shown, on its clock.
    window_asked: f64,
}

impl Library {
    /// The rows of a list `read`, among the library's, its folders shown: read at last.
    fn took(&mut self, read: Read) -> Vec<Entry> {
        self.folders = read.folders;
        self.unread = None;
        self.browser.entries(read.rows)
    }

    /// The interface's window of `count` rows from the `first`, asked at `asked` on its clock:
    /// one asked before the window shown, come late, does not take its place.
    pub fn ask_window(&mut self, first: usize, count: usize, asked: f64) {
        if asked >= self.window_asked {
            self.window_asked = asked;
            self.browser.set_window(first, count);
        }
    }

    /// The track at `path` now separated as far as `separation`.
    pub fn separated(&mut self, path: &str, separation: SeparationDto) {
        let id = self.browser.track_id(path);
        self.separations.insert(id, separation);
    }

    /// The list browsed as the interface shows it, the separations of its window as known (see
    /// [`shown`]).
    pub fn view(&self) -> LibraryViewDto {
        let browser = &self.browser;
        let folder = match browser.source() {
            Source::Folder(path) => Some(FolderViewDto {
                path: path.display().to_string(),
                root: browser.root_of(path).map(|root| root.display().to_string()),
                up: browser.folder_above().map(|up| up.display().to_string()),
                folders: self.folders.iter().map(|folder| FolderDto::of(folder)).collect(),
            }),
            _ => None,
        };
        let sort = browser.sort();
        LibraryViewDto {
            seq: browser.seq(),
            source: SourceDto::from(browser.source()),
            folder,
            scope: browser.scope().into(),
            filter: browser.filter().to_owned(),
            sort: SortDto { column: sort.column.into(), descending: sort.descending },
            ordered: browser.source().ordered(),
            removable: browser.removable(),
            reorderable: browser.reorderable(),
            count: browser.count(),
            cursor: browser.cursor_index(),
            cursor_moves: browser.cursor_moves(),
            listed: browser.listed(),
            selected: browser.selected_count(),
            focus: browser.focus().into(),
            pointed: match browser.pointed() {
                Shelf::Prepare => ShelfDto::Prepare,
                Shelf::Crate(id) => ShelfDto::Crate { id: id.0 },
            },
            reference: effective(browser.reference(), self.around.decks_in_use),
            unread: self.unread.clone(),
            window: self.window(),
        }
    }

    /// The rows of the window the interface asked for (see [`Browser::set_window`]), their
    /// separations as known (see [`shown`]).
    pub fn window(&self) -> LibraryWindowDto {
        let Library { browser, separations, around, .. } = self;
        let rows = browser.window().map(|row| {
            let track = row.track;
            let record = track.record.as_ref();
            let tags = record.map(|r| &r.tags);
            let tag =
                |pick: fn(&prisme_media::TrackTags) -> &Option<String>| tags.and_then(|t| pick(t).clone());
            let separation = separations.get(&row.entry.track).copied().unwrap_or_default();
            let seen = around.sees(track);
            let stems = match seen.separating {
                Some(percent) => RowStemsDto::Separating { fraction: f64::from(percent) / 100.0 },
                None if seen.queued => RowStemsDto::Queued,
                None if record.is_some() && !track.failed() && around.stems.reach > separation => {
                    RowStemsDto::Addable { separation }
                }
                None => RowStemsDto::Separated { separation },
            };
            let decks = (0..around.decks_in_use)
                .filter(|&d| seen.decks & 1 << d != 0)
                .map(|deck| DeckMarkDto { deck, playing: seen.playing & 1 << deck != 0 })
                .collect();
            LibraryRowDto {
                path: track.path.clone(),
                played_at_ms: row.entry.played_at_ms,
                position: row.position,
                title: track.title.clone(),
                artist: tag(|t| &t.artist),
                album: tag(|t| &t.album),
                genre: tag(|t| &t.genre),
                year: tags.and_then(|t| t.year),
                bpm: track.bpm,
                key: track.key.map(Into::into),
                color: record.and_then(|r| r.marks.color),
                rating: record.map_or(0, |r| r.marks.rating),
                plays: record.map_or(0, |r| r.play_count),
                analysed: record.is_some(),
                failed: track.failed(),
                stems,
                selected: row.selected,
                played: row.played,
                elsewhere: row.elsewhere,
                previewing: seen.previewing,
                decks,
                fits: seen.fits,
                pitch: seen.pitch.map(|p| p.percent),
                reachable: seen.pitch.is_some_and(|p| p.reachable),
            }
        });
        let rows = rows.collect();
        LibraryWindowDto { seq: browser.seq(), first: browser.window_first(), rows }
    }

    /// The tracks of the window whose separation is not known yet.
    pub fn unknown(&self) -> Vec<(TrackId, String)> {
        let rows = self.browser.window().filter(|row| !self.separations.contains_key(&row.entry.track));
        rows.map(|row| (row.entry.track, row.track.path.clone())).collect()
    }

    /// What the rows read `around` them from now on: whether the rows of the window show
    /// another thing of it (a tempo, a key heard, a deck holding one of them, a preview).
    pub fn look_around(&mut self, around: &Around) -> bool {
        let moved = self.browser.window().any(|row| self.around.sees(row.track) != around.sees(row.track));
        self.around = around.clone();
        moved
    }

    /// The lists the library keeps, as the side panel shows them.
    pub fn lists(&self) -> LibraryListsDto {
        let browser = &self.browser;
        LibraryListsDto {
            crates: browser.crates().iter().map(CrateDto::from).collect(),
            sessions: browser.sessions().iter().map(HistorySessionDto::from).collect(),
            prepare: browser.prepare().count(),
            next: browser.prepare().next().map(NamedTrackDto::of),
            elsewhere: browser.elsewhere().count(),
        }
    }
}

/// What the rows of the library show of the rest of the application: the deck they are
/// compared with as it sounds (its tempo to a tenth of a BPM, about the tenth of a percent the
/// rows' pitch shows: a tempo fader moving sends them again only that often), the decks holding
/// them, the track previewed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Around {
    pub compare: Compare,
    pub decks: [Option<PathBuf>; prisme_engine::MAX_DECKS],
    pub playing: [bool; prisme_engine::MAX_DECKS],
    pub decks_in_use: usize,
    /// The track previewed, or the one whose file opens to be.
    pub previewed: [Option<String>; 2],
    pub stems: RowsStems,
}

/// What the rows show of the separation: the track separated and how far (in hundredths), the
/// tracks queued, and how far the models installed separate a track.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RowsStems {
    pub current: Option<(PathBuf, u8)>,
    pub queued: Queued,
    pub reach: SeparationDto,
}

impl Around {
    /// The values of the frame `view` the rows read, compared with `reference`.
    pub fn read(&mut self, view: &View, reference: usize) {
        let reference = effective(reference, view.decks_in_use);
        let status = &view.status.decks[reference];
        self.compare = Compare {
            key: prisme_actions::screen::heard_key(view, reference),
            bpm: status.bpm.map(|bpm| (bpm * 10.0).round() / 10.0),
            range: view.decks[reference ^ 1].tempo_range,
        };
        self.playing = std::array::from_fn(|d| view.status.decks[d].playing);
        self.decks_in_use = view.decks_in_use;
    }
}

/// What a row shows of the rest of the application, read around it.
#[derive(PartialEq)]
struct Seen {
    /// The decks holding its track, and those of them playing, a bit each.
    decks: u8,
    playing: u8,
    previewing: bool,
    fits: bool,
    pitch: Option<Pitch>,
    /// Being separated, so far (in hundredths), or queued; and how far the models reach.
    separating: Option<u8>,
    queued: bool,
    reach: SeparationDto,
}

impl Around {
    fn sees(&self, track: &Track) -> Seen {
        let path = Path::new(&track.path);
        let held = (0..self.decks_in_use).filter(|&d| self.decks[d].as_deref() == Some(path));
        let decks = held.fold(0, |bits, d| bits | 1 << d);
        let playing = (0..self.decks_in_use).filter(|&d| self.playing[d]).fold(0, |bits, d| bits | 1 << d);
        Seen {
            decks,
            playing: decks & playing,
            previewing: self.previewed.iter().flatten().any(|p| *p == track.path),
            fits: self.compare.fits(track.key),
            pitch: self.compare.pitch(track.bpm),
            separating: self.stems.current.as_ref().filter(|(current, _)| current == path).map(|(_, p)| *p),
            queued: self.stems.queued.0.contains(path),
            reach: self.stems.reach,
        }
    }
}

/// The deck the tracks are compared with among those in use: deck A for one no longer in use.
pub fn effective(reference: usize, decks_in_use: usize) -> usize {
    if reference < decks_in_use {
        reference
    } else {
        0
    }
}

/// What `look` reads of the library, once the separations of the rows of its window are known:
/// those not known yet are looked for on disk with its lock let go (a drive may be slow).
pub fn shown<T>(state: &AppState, look: impl Fn(&Library) -> T) -> T {
    loop {
        let library = lock(&state.library);
        let unknown = library.unknown();
        if unknown.is_empty() {
            return look(&library);
        }
        drop(library);
        look_for_separations(state, unknown);
    }
}

/// The separations of the `unknown` tracks looked for on disk, the library's lock let go: kept
/// where none was told meanwhile (see [`Library::separated`]).
pub fn look_for_separations(state: &AppState, unknown: Vec<(TrackId, String)>) {
    let found: Vec<_> =
        unknown.into_iter().map(|(id, path)| (id, stored_separation(Path::new(&path)))).collect();
    let mut library = lock(&state.library);
    for (id, separation) in found {
        library.separations.entry(id).or_insert(separation);
    }
}

/// The library read whole, its lists, then the folder shown last (else the main music folder)
/// opened: at start, off the main thread.
pub fn start(state: &AppState, db: &LibraryDb, roots: Vec<PathBuf>) {
    let main = roots[0].clone();
    state.browse(|l| l.browser.set_roots(roots));
    reload(state, db);
    if let Err(e) = lists(state, db) {
        error!("the lists of the library not read: {e}");
    }
    let last = state.preferences.read(|p| p.library.last_folder.clone()).map(PathBuf::from);
    if last.is_none_or(|last| open(state, db, Source::Folder(last)).is_err()) {
        if let Err(e) = open(state, db, Source::Folder(main.clone())) {
            error!("the music folder not listed: {e}");
            let why = ErrorDto::from(&e);
            state.browse(|l| {
                l.unread = Some(why);
                l.browser.show(Source::Folder(main), Vec::new());
            });
        }
    }
}

/// The library read whole again: a scan ended, files went, moved or came back. Its tracks are
/// made with the library's lock let go.
pub fn reload(state: &AppState, db: &LibraryDb) {
    match db.records() {
        Ok(records) => {
            let tracks = Tracks::of(records);
            state.browse(|l| l.browser.load(tracks));
        }
        Err(e) => error!("the library not read: {e}"),
    }
}

/// The lists the library keeps read again: the crates, the sets, the prepare list, and the
/// tracks of the set going on.
pub fn lists(state: &AppState, db: &LibraryDb) -> AppResult<()> {
    let crates = db.crate_list()?;
    let sessions = db.history_sessions()?;
    let prepare: Vec<String> = db.prepare_list()?.into_iter().map(|e| e.path).collect();
    let latest = sessions.iter().max_by_key(|s| s.started_at_ms);
    let tonight = match latest.filter(|s| prisme_library::now_ms() - s.started_at_ms < TONIGHT_MS) {
        Some(set) => db.history_plays(set.id)?.into_iter().map(|p| p.path).collect(),
        None => Vec::new(),
    };
    state.browse(|l| {
        l.browser.set_crates(crates);
        l.browser.set_sessions(sessions);
        l.browser.set_prepare(&prepare);
        l.browser.set_tonight(tonight.iter().map(String::as_str));
    });
    Ok(())
}

/// What the library knows of the tracks at `paths` read again (marks, tags, corrections).
pub fn reread<P: AsRef<Path>>(state: &AppState, db: &LibraryDb, paths: impl IntoIterator<Item = P>) {
    for path in paths {
        match db.get(path.as_ref()) {
            Ok(Some(record)) => state.browse(|l| l.browser.put(record)),
            Ok(None) => {}
            Err(e) => error!("{} not read from the library: {e}", path.as_ref().display()),
        }
    }
}

/// A track analysed, or changed: in its lists at once.
pub fn analysed(state: &AppState, record: TrackRecord) {
    state.browse(|l| l.browser.put(record));
}

/// Shows `source`: the whole list, in its own order if it has one. A folder is the one shown
/// next time Prisme starts.
pub fn open(state: &AppState, db: &LibraryDb, source: Source) -> AppResult<()> {
    let read = read(state, db, &source)?;
    if let Source::Folder(path) = &source {
        let last = Some(path.display().to_string());
        if state.preferences.read(|p| p.library.last_folder != last) {
            state.update_preferences(|p| p.library.last_folder = last);
        }
    }
    state.browse(|l| {
        let rows = l.took(read);
        l.browser.show(source, rows);
    });
    Ok(())
}

/// The folder above the one shown, inside its music folder.
pub fn up(state: &AppState, db: &LibraryDb) -> AppResult<()> {
    let above = lock(&state.library).browser.folder_above();
    match above {
        Some(above) => open(state, db, Source::Folder(above)),
        None => Ok(()),
    }
}

/// The list shown read again, searched, sorted and picked as it was: what it holds changed.
pub fn refresh(state: &AppState, db: &LibraryDb) -> AppResult<()> {
    let source = lock(&state.library).browser.source().clone();
    let read = read(state, db, &source)?;
    state.browse(|l| {
        if *l.browser.source() == source {
            let rows = l.took(read);
            l.browser.refresh(rows);
        }
    });
    Ok(())
}

/// Files came into the folders `changed` or left them: the folder shown among them read again.
pub fn files_changed(state: &AppState, db: &LibraryDb, changed: &[PathBuf]) {
    let shown =
        matches!(lock(&state.library).browser.source(), Source::Folder(path) if changed.contains(path));
    if shown {
        if let Err(e) = refresh(state, db) {
            error!("the folder shown not listed again: {e}");
        }
    }
}

/// The music folders are `roots` now: a folder shown that left them gives way to the main one.
pub fn folders_changed(state: &AppState, db: &LibraryDb, roots: Vec<PathBuf>) {
    let main = roots[0].clone();
    let left = state.browse(|l| {
        l.browser.set_roots(roots);
        matches!(l.browser.source(), Source::Folder(path) if l.browser.root_of(path).is_none())
    });
    if left {
        if let Err(e) = open(state, db, Source::Folder(main)) {
            error!("the music folder not listed: {e}");
        }
    }
}

/// What a list holds, as read: its rows, each a track's path and when it was played (a set's),
/// and the folders in it if it is a folder.
#[derive(Default)]
struct Read {
    rows: Vec<(String, Option<i64>)>,
    folders: Vec<PathBuf>,
}

/// What `source` holds.
fn read(state: &AppState, db: &LibraryDb, source: &Source) -> AppResult<Read> {
    let derived = lock(&state.library).browser.rows_of(source);
    if let Some(entries) = derived {
        let library = lock(&state.library);
        let catalogue = library.browser.catalogue();
        let rows = entries.iter().map(|e| (catalogue.get(e.track).path.clone(), e.played_at_ms)).collect();
        return Ok(Read { rows, ..Read::default() });
    }
    let rows = match source {
        Source::Folder(path) => {
            let roots = lock(&state.library).browser.roots().to_vec();
            let listing = folders::listing(path, &roots)?;
            known(state, db, &listing.files);
            let rows =
                listing.files.into_iter().map(|(file, _)| (file.display().to_string(), None)).collect();
            return Ok(Read { rows, folders: listing.folders });
        }
        Source::Crate(id) => db.crate_tracks(*id)?.into_iter().map(|e| (e.path, None)).collect(),
        Source::History(id) => {
            db.history_plays(*id)?.into_iter().map(|p| (p.path, Some(p.played_at_ms))).collect()
        }
        Source::Prepare | Source::Elsewhere | Source::Duplicates => Vec::new(),
    };
    Ok(Read { rows, ..Read::default() })
}

/// What the library knows of `files` of a folder, those it does not know as they are now read
/// from the database: a folder that moved, or came back under another drive letter, keeps it.
fn known(state: &AppState, db: &LibraryDb, files: &[(PathBuf, prisme_library::FileStamp)]) {
    let unknown: Vec<&(PathBuf, prisme_library::FileStamp)> = {
        let library = lock(&state.library);
        let catalogue = library.browser.catalogue();
        let current = |path: &Path, stamp| {
            let id = catalogue.find(&path.to_string_lossy())?;
            catalogue.get(id).record.as_ref().map(|r| r.is_current(stamp))
        };
        files.iter().filter(|(path, stamp)| current(path, *stamp) != Some(true)).collect()
    };
    for (path, stamp) in unknown {
        if let Err(e) = adopt(db, path, *stamp, Search::Stamp) {
            error!("{} not looked for under another path: {e}", path.display());
        }
        match db.get(path) {
            Ok(Some(record)) if record.is_current(*stamp) => state.browse(|l| l.browser.put(record)),
            Ok(_) => {}
            Err(e) => error!("{} not read from the library: {e}", path.display()),
        }
    }
}

/// The track the cursor is on, which a load or a preview sent now takes.
pub fn selected(state: &AppState) -> Option<PathBuf> {
    lock(&state.library).browser.cursor().map(|track| PathBuf::from(&track.path))
}

/// The tracks `paths` names, else those selected.
pub fn tracks(state: &AppState, paths: Option<Vec<String>>) -> Vec<String> {
    paths.unwrap_or_else(|| lock(&state.library).browser.selected_paths())
}

/// Puts `paths` at the end of the prepare list; how many were not in it. They are separated in
/// the background if the DJ chose so, ready before the set.
pub fn add_to_prepare(
    state: &AppState,
    db: &LibraryDb,
    stems: Option<&StemsService>,
    paths: &[String],
) -> AppResult<usize> {
    let added = db.add_to_prepare(&as_paths(paths))?;
    if let Some(stems) = stems.filter(|_| stems_service::separates_on_load(state)) {
        match stems.enqueue(paths.iter().map(PathBuf::from)) {
            // without a model there is nothing to do, and nothing to tell
            Ok(_) | Err(AppError::NoStemsModel) => {}
            Err(e) => warn!("the prepare list not queued for separation: {e}"),
        }
    }
    prepare_changed(state, db);
    Ok(added)
}

/// Puts `paths` before `before` in the prepare list (`None`: at its end), adding those not in it.
pub fn place_in_prepare(
    state: &AppState,
    db: &LibraryDb,
    paths: &[String],
    before: Option<&str>,
) -> AppResult<()> {
    db.place_in_prepare(&as_paths(paths), before.map(Path::new))?;
    prepare_changed(state, db);
    Ok(())
}

/// The prepare list read again: a track was put in it, played, or taken out.
pub fn prepare_changed(state: &AppState, db: &LibraryDb) {
    match db.prepare_list() {
        Ok(list) => {
            let paths: Vec<String> = list.into_iter().map(|e| e.path).collect();
            state.browse(|l| l.browser.set_prepare(&paths));
        }
        Err(e) => error!("the prepare list not read: {e}"),
    }
}

/// Puts `paths` in the crate `id`, one filled by hand; how many were not in it.
pub fn add_to_crate(state: &AppState, db: &LibraryDb, id: CrateId, paths: &[String]) -> AppResult<usize> {
    filled_by_hand(db, id)?;
    let added = db.add_to_crate(id, &as_paths(paths))?;
    crates_changed(state, db, Some(id))?;
    Ok(added)
}

/// The crates read again, and the crate `changed` shown with them.
pub fn crates_changed(state: &AppState, db: &LibraryDb, changed: Option<CrateId>) -> AppResult<()> {
    let crates = db.crate_list()?;
    let shown = state.browse(|l| {
        l.browser.set_crates(crates);
        changed.is_some_and(|id| *l.browser.source() == Source::Crate(id))
    });
    if shown {
        refresh(state, db)?;
    }
    Ok(())
}

/// Takes `paths` (else the selection) out of the list shown, the prepare list or a crate filled
/// by hand; a cursor on one of them goes to the row after them.
pub fn remove_from_list(state: &AppState, db: &LibraryDb, paths: Option<Vec<String>>) -> AppResult<()> {
    let (source, removable, entries, next) = {
        let library = lock(&state.library);
        let browser = &library.browser;
        let entries = match &paths {
            Some(paths) => browser.rows_with(paths),
            None => browser.selected(),
        };
        let cursor = browser.cursor_entry();
        let next = match cursor {
            Some(cursor) if !entries.contains(&cursor) => Some(cursor),
            _ => browser.neighbour(&entries),
        };
        (browser.source().clone(), browser.removable(), entries, next)
    };
    if !removable || entries.is_empty() {
        return Ok(());
    }
    let paths = paths.unwrap_or_else(|| lock(&state.library).browser.paths_of(&entries));
    match source {
        Source::Prepare => {
            db.remove_from_prepare(&as_paths(&paths))?;
            prepare_changed(state, db);
        }
        Source::Crate(id) => {
            db.remove_from_crate(id, &as_paths(&paths))?;
            crates_changed(state, db, Some(id))?;
        }
        _ => return Ok(()),
    }
    state.browse(|l| l.browser.put_cursor(next));
    Ok(())
}

/// Takes the tracks from elsewhere among `paths` out of the library, their files left as they
/// are; the cursor goes to the row after them. Those from elsewhere shown, all gone, give way to
/// the main music folder.
pub fn remove_from_library(state: &AppState, db: &LibraryDb, paths: Option<Vec<String>>) -> AppResult<()> {
    let (paths, next) = {
        let library = lock(&state.library);
        let browser = &library.browser;
        let paths = match paths {
            Some(paths) => paths,
            None => browser.selected_paths(),
        };
        let elsewhere = browser.elsewhere_among(&paths);
        let next = browser.neighbour(&browser.rows_with(&elsewhere));
        (elsewhere, next)
    };
    if paths.is_empty() {
        return Ok(());
    }
    db.forget(&paths)?;
    let main = state.browse(|l| {
        l.browser.forget(&paths);
        l.browser.put_cursor(next);
        let emptied = *l.browser.source() == Source::Elsewhere && l.browser.elsewhere().next().is_none();
        l.browser.main_root().filter(|_| emptied).cloned()
    });
    if let Some(main) = main {
        open(state, db, Source::Folder(main))?;
    }
    Ok(())
}

/// Colours `paths` (`None` takes the colour off).
pub fn set_color(state: &AppState, db: &LibraryDb, paths: &[String], color: Option<u32>) -> AppResult<()> {
    db.set_color(&as_paths(paths), color)?;
    reread(state, db, paths);
    Ok(())
}

/// Rates the track at `path` `stars` (0 to 5); the stars it has already take its rating off.
pub fn rate(state: &AppState, db: &LibraryDb, path: &str, stars: u8) -> AppResult<()> {
    let rating = if db.marks(Path::new(path))?.rating == stars { 0 } else { stars };
    db.set_rating(&[Path::new(path)], rating)?;
    reread(state, db, [path]);
    Ok(())
}

/// A crate named `name`, shown: its id.
pub fn create_crate(
    state: &AppState,
    db: &LibraryDb,
    name: &str,
    rules: Option<&SmartRules>,
) -> AppResult<CrateId> {
    let name = crate_name(name)?;
    let id = match rules {
        Some(rules) => db.create_smart_crate(name, rules)?,
        None => db.create_crate(name)?,
    };
    crates_changed(state, db, None)?;
    open(state, db, Source::Crate(id))?;
    Ok(id)
}

/// The crate `id` named `name`, and picking its tracks by `rules` if it is a smart one: at once.
pub fn change_crate(
    state: &AppState,
    db: &LibraryDb,
    id: CrateId,
    name: &str,
    rules: Option<&SmartRules>,
) -> AppResult<()> {
    let name = crate_name(name)?;
    db.rename_crate(id, name)?;
    if let Some(rules) = rules {
        db.set_crate_rules(id, rules)?;
    }
    crates_changed(state, db, Some(id))
}

/// Deletes the crate `id`, not its tracks; shown, it gives way to the folder shown last.
pub fn delete_crate(state: &AppState, db: &LibraryDb, id: CrateId) -> AppResult<()> {
    db.delete_crate(id)?;
    crates_changed(state, db, None)?;
    leave(state, db, &Source::Crate(id))
}

/// Deletes the set `id` from the history; shown, it gives way to the folder shown last.
pub fn delete_session(state: &AppState, db: &LibraryDb, id: SessionId) -> AppResult<()> {
    db.delete_history_session(id)?;
    lists(state, db)?;
    leave(state, db, &Source::History(id))
}

/// `source`, if it is shown, gives way to the folder shown last.
fn leave(state: &AppState, db: &LibraryDb, source: &Source) -> AppResult<()> {
    if lock(&state.library).browser.source() != source {
        return Ok(());
    }
    let last = state.preferences.read(|p| p.library.last_folder.clone()).map(PathBuf::from);
    let folder = last.or_else(|| lock(&state.library).browser.main_root().cloned());
    folder.map_or(Ok(()), |folder| open(state, db, Source::Folder(folder)))
}

/// A track was played: the history, its play count, the prepare list it left, and the set shown.
pub fn played(state: &AppState, db: &LibraryDb, path: &Path) -> AppResult<()> {
    reread(state, db, [path]);
    lists(state, db)?;
    if matches!(lock(&state.library).browser.source(), Source::History(_)) {
        refresh(state, db)?;
    }
    Ok(())
}

fn crate_name(name: &str) -> AppResult<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::EmptyCrateName);
    }
    Ok(name)
}

fn filled_by_hand(db: &LibraryDb, id: CrateId) -> AppResult<()> {
    match db.crate_rules(id)? {
        Some(_) => Err(AppError::SmartCrate),
        None => Ok(()),
    }
}

fn as_paths(paths: &[String]) -> Vec<&Path> {
    paths.iter().map(Path::new).collect()
}

impl Performing<'_> {
    /// An action of the library's browsing, given a key's press or an encoder's steps: the knob
    /// moves the cursor through the tracks, or the pointer through the lists.
    pub(super) fn browse(&self, action: LibraryAction, given: Given) -> AppResult<()> {
        let steps = match given {
            Given::Steps(steps) => i64::from(steps),
            Given::Value(_) if given.pressed() => 1,
            Given::Value(_) => return Ok(()),
        };
        let state = self.state;
        let column = |column| state.browse(|l| l.browser.sort_by(column, None));
        match action {
            LibraryAction::Browse => self.knob(steps),
            LibraryAction::Previous => self.knob(-1),
            LibraryAction::Next => self.knob(1),
            LibraryAction::PagePrevious => self.knob(-PAGE),
            LibraryAction::PageNext => self.knob(PAGE),
            LibraryAction::Lists => state.browse(|l| l.browser.point(steps)),
            LibraryAction::SelectAll => state.browse(|l| l.browser.select_all()),
            LibraryAction::ClearSelection => state.browse(|l| l.browser.clear_selection()),
            LibraryAction::Focus => state.browse(|l| {
                let focus = if l.browser.focus() == Focus::Tracks { Focus::Lists } else { Focus::Tracks };
                l.browser.set_focus(focus);
            }),
            LibraryAction::Left => state.browse(|l| l.browser.set_focus(Focus::Lists)),
            LibraryAction::Right => state.browse(|l| l.browser.set_focus(Focus::Tracks)),
            LibraryAction::SortTitle => column(Column::Title),
            LibraryAction::SortArtist => column(Column::Artist),
            LibraryAction::SortBpm => column(Column::Bpm),
            LibraryAction::SortKey => column(Column::Key),
            LibraryAction::ListOpen => return self.open_list(|b| b.open_pointed()),
            LibraryAction::ListNext => return self.open_list(|b| b.next_list(1)),
            LibraryAction::ListPrevious => return self.open_list(|b| b.next_list(-1)),
            LibraryAction::Remove => return self.in_library(|db| remove_from_list(state, db, None)),
            LibraryAction::Prepare => {
                let stems = self.app.map(|app| app.state::<StemsService>());
                return self.in_library(|db| {
                    let paths = tracks(state, None);
                    add_to_prepare(state, db, stems.as_deref(), &paths).map(drop)
                });
            }
            // sent as a load or a list opened (`Actions::sent_as`): nothing to load on
            LibraryAction::Go => {}
            LibraryAction::Search
            | LibraryAction::Load(_)
            | LibraryAction::LoadPlay(_)
            | LibraryAction::LoadFree
            | LibraryAction::Double(_)
            | LibraryAction::Preview
            | LibraryAction::PreviewStop
            | LibraryAction::PreviewSeek => {
                return Err(AppError::NotPerformed(prisme_control::Target::Library(action).to_string()))
            }
        }
        Ok(())
    }

    /// The knob turned by `steps`: over the tracks, or over the lists while they have the focus.
    fn knob(&self, steps: i64) {
        self.state.browse(|l| match l.browser.focus() {
            Focus::Tracks => l.browser.move_cursor(steps),
            Focus::Lists => l.browser.point(steps),
        });
    }

    /// Opens the list `which` names.
    fn open_list(&self, which: impl FnOnce(&mut Browser) -> Source) -> AppResult<()> {
        let source = self.state.browse(|l| which(&mut l.browser));
        self.in_library(|db| open(self.state, db, source))
    }

    /// `edit` done in the library, once it is open.
    fn in_library(&self, edit: impl FnOnce(&LibraryDb) -> AppResult<()>) -> AppResult<()> {
        match self.library {
            Some(db) => edit(db),
            None => Ok(()),
        }
    }
}

/// What `library.go` does now: the list pointed at opened while the knob is on the lists, else
/// the track selected loaded on the first deck the DJ does not hear (none: nothing).
pub fn go(state: &AppState) -> Option<LibraryAction> {
    if lock(&state.library).browser.focus() == Focus::Lists {
        return Some(LibraryAction::ListOpen);
    }
    let decks = state.decks_in_use();
    (0..decks)
        .find(|&deck| !state.deck_status(deck).is_ok_and(|s| prisme_actions::view::plays(&s)))
        .map(LibraryAction::Load)
}

/// The deck `reference` as `view` shows it, holding the track at `path`, which the rows are
/// ranked against while they are sorted by `Mix`; the deck beside it is the one the next track
/// goes on.
pub fn mix_reference(view: &View, reference: usize, path: Option<&Path>) -> MixReference {
    let next = &view.decks[reference ^ 1];
    MixReference {
        bpm: view.status.decks[reference].bpm.or(view.decks[reference].track.and_then(|t| t.bpm)),
        key: prisme_actions::screen::heard_key(view, reference),
        path: path.map(|path| path.display().to_string()),
        range: next.tempo_range,
        key_lock: next.key_lock,
    }
}

/// Picks the row of the track at `path` (played at `played_at_ms` in a set) as `pick` says. A
/// right click or a drag on a row gone from the list meanwhile picks nothing, the selection let
/// go: the menu or the drop acts on no other track.
pub fn select(state: &AppState, path: &str, played_at_ms: Option<i64>, pick: Pick) {
    state.browse(|l| match l.browser.find(path, played_at_ms) {
        Some(entry) => l.browser.select(entry, pick),
        None if pick == Pick::Context => l.browser.put_cursor(None),
        None => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::bench::Bench;
    use prisme_actions::input::Input;
    use prisme_library::{FileStamp, TrackRecord};
    use prisme_media::TrackTags;
    use std::collections::HashSet;
    use std::sync::Arc;

    /// A bench whose library holds tracks `a` to `d` of `C:/Music`, the folder shown, sorted by
    /// title, `d` from elsewhere.
    fn library() -> Bench {
        let bench = Bench::new();
        for (name, title) in [("a", "Alpha"), ("b", "Bravo"), ("c", "Charlie"), ("d", "Delta")] {
            let path =
                if name == "d" { "D:/Downloads/d.mp3".to_string() } else { format!("C:/Music/{name}.mp3") };
            let tags = TrackTags { title: Some(title.into()), ..TrackTags::default() };
            let record = TrackRecord::analyzed(
                Path::new(&path),
                FileStamp { size: 1, modified_ms: 1 },
                60.0,
                Default::default(),
                tags,
            );
            bench.library.save(&record).unwrap();
        }
        start(&bench.state, &bench.library, vec![PathBuf::from("C:/Music")]);
        bench.state.browse(|l| {
            let rows = l.browser.entries(["a", "b", "c"].map(|n| (format!("C:/Music/{n}.mp3"), None)));
            l.browser.show(Source::Folder("C:/Music".into()), rows);
        });
        bench
    }

    #[test]
    fn a_right_click_on_a_row_gone_from_the_list_selects_nothing() {
        let bench = library();
        select(&bench.state, "C:/Music/a.mp3", None, Pick::Only);
        select(&bench.state, "C:/Music/gone.mp3", None, Pick::Toggle);
        assert_eq!(tracks(&bench.state, None), ["C:/Music/a.mp3"], "a click on it: nothing changes");
        select(&bench.state, "C:/Music/gone.mp3", None, Pick::Context);
        assert!(tracks(&bench.state, None).is_empty());
    }

    #[test]
    fn a_window_asked_before_the_one_shown_and_come_late_is_left() {
        let bench = library();
        let mut library = lock(&bench.state.library);
        library.ask_window(2, 1, 2.0);
        library.ask_window(0, 1, 1.0);
        assert_eq!(library.browser.window_first(), 2);
        library.ask_window(0, 1, 3.0);
        assert_eq!(library.browser.window_first(), 0);
    }

    fn titles(bench: &Bench) -> Vec<String> {
        let mut library = lock(&bench.state.library);
        library.browser.set_window(0, 100);
        library.window().rows.into_iter().map(|row| row.title).collect()
    }

    fn pick(bench: &Bench, name: &str, pick: Pick) {
        let path =
            if name == "d" { "D:/Downloads/d.mp3".to_string() } else { format!("C:/Music/{name}.mp3") };
        select(&bench.state, &path, None, pick);
    }

    fn prepared(bench: &Bench) -> Vec<String> {
        bench.library.prepare_list().unwrap().into_iter().map(|e| e.path).collect()
    }

    #[test]
    fn a_music_folder_that_cannot_be_read_at_start_is_shown_empty_with_why() {
        let bench = Bench::new();
        let root = std::env::temp_dir().join(format!("prisme-unread-{}", std::process::id()));
        std::fs::write(&root, b"not a folder").unwrap();
        start(&bench.state, &bench.library, vec![root.clone()]);
        let view = lock(&bench.state.library).view();
        assert!(matches!(view.source, SourceDto::Folder { .. }), "{:?}", view.source);
        assert!(view.unread.is_some_and(|why| !why.code.is_empty()));
        std::fs::remove_file(&root).unwrap();
        std::fs::create_dir(&root).unwrap();
        refresh(&bench.state, &bench.library).unwrap();
        assert_eq!(lock(&bench.state.library).view().unread, None, "read at last");
        std::fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn a_controller_puts_the_selection_in_the_prepare_list_and_takes_it_out_there() {
        let bench = library();
        pick(&bench, "a", Pick::Only);
        pick(&bench, "b", Pick::Toggle);
        bench.press("library.prepare", 1.0).unwrap();
        assert_eq!(prepared(&bench), ["C:/Music/a.mp3", "C:/Music/b.mp3"]);
        assert_eq!(lock(&bench.state.library).lists().prepare, 2, "the side panel follows");
        open(&bench.state, &bench.library, Source::Prepare).unwrap();
        pick(&bench, "a", Pick::Only);
        bench.press("library.remove", 1.0).unwrap();
        assert_eq!(prepared(&bench), ["C:/Music/b.mp3"]);
        assert_eq!(titles(&bench), ["Bravo"]);
        let cursor = lock(&bench.state.library).browser.cursor().map(|t| t.title.clone());
        assert_eq!(cursor.as_deref(), Some("Bravo"), "the cursor on the row after");
    }

    #[test]
    fn a_track_touched_is_taken_out_of_the_list_not_the_selection() {
        let bench = library();
        let paths: Vec<String> = ["a", "b", "c"].map(|n| format!("C:/Music/{n}.mp3")).into();
        add_to_prepare(&bench.state, &bench.library, None, &paths).unwrap();
        open(&bench.state, &bench.library, Source::Prepare).unwrap();
        pick(&bench, "a", Pick::Only);
        remove_from_list(&bench.state, &bench.library, Some(vec!["C:/Music/c.mp3".into()])).unwrap();
        assert_eq!(prepared(&bench), ["C:/Music/a.mp3", "C:/Music/b.mp3"]);
        assert_eq!(titles(&bench), ["Alpha", "Bravo"]);
        let cursor = lock(&bench.state.library).browser.cursor().map(|t| t.title.clone());
        assert_eq!(cursor.as_deref(), Some("Alpha"), "the cursor stays where it was");
    }

    #[test]
    fn the_knob_goes_through_the_lists_and_opens_the_one_pointed_at() {
        let bench = library();
        let warm_up = create_crate(&bench.state, &bench.library, " Warm-up ", None).unwrap();
        add_to_crate(&bench.state, &bench.library, warm_up, &["C:/Music/c.mp3".into()]).unwrap();
        open(&bench.state, &bench.library, Source::Prepare).unwrap();
        bench.press("library.focus", 1.0).unwrap();
        bench.moved("DDJ", "library.browse", Input::Steps { steps: 1.0, step: None }).unwrap();
        bench.press("library.go", 1.0).unwrap();
        let library = lock(&bench.state.library);
        assert_eq!(*library.browser.source(), Source::Crate(warm_up));
        assert_eq!(library.browser.focus(), Focus::Tracks);
        assert_eq!(library.lists().crates[0].name, "Warm-up", "named without its spaces");
    }

    #[test]
    fn a_controller_sorts_and_moves_through_the_rows_shown() {
        let bench = library();
        bench.press("library.sort.title", 1.0).unwrap();
        assert_eq!(titles(&bench), ["Charlie", "Bravo", "Alpha"], "by title again: the other way");
        bench.press("library.next", 1.0).unwrap();
        bench.press("library.page.next", 1.0).unwrap();
        bench.press("library.previous", 1.0).unwrap();
        let cursor = lock(&bench.state.library).browser.cursor().map(|t| t.title.clone());
        assert_eq!(cursor.as_deref(), Some("Bravo"));
    }

    #[test]
    fn tracks_are_placed_in_the_prepare_list_in_one_go_and_the_list_shown_follows() {
        let bench = library();
        let paths: Vec<String> = ["a", "b"].map(|n| format!("C:/Music/{n}.mp3")).into();
        add_to_prepare(&bench.state, &bench.library, None, &paths).unwrap();
        open(&bench.state, &bench.library, Source::Prepare).unwrap();
        place_in_prepare(
            &bench.state,
            &bench.library,
            &["C:/Music/c.mp3".into(), "C:/Music/b.mp3".into()],
            Some("C:/Music/a.mp3"),
        )
        .unwrap();
        assert_eq!(titles(&bench), ["Charlie", "Bravo", "Alpha"]);
    }

    #[test]
    fn the_stars_a_track_has_take_its_rating_off() {
        let bench = library();
        rate(&bench.state, &bench.library, "C:/Music/a.mp3", 4).unwrap();
        let rating = |bench: &Bench| {
            let mut library = lock(&bench.state.library);
            library.browser.set_window(0, 10);
            library.window().rows[0].rating
        };
        assert_eq!(rating(&bench), 4);
        rate(&bench.state, &bench.library, "C:/Music/a.mp3", 4).unwrap();
        assert_eq!(rating(&bench), 0);
    }

    #[test]
    fn only_the_tracks_from_elsewhere_leave_the_library_and_their_list_gives_way_once_empty() {
        let bench = library();
        open(&bench.state, &bench.library, Source::Elsewhere).unwrap();
        assert_eq!(titles(&bench), ["Delta"]);
        let paths = vec!["C:/Music/a.mp3".into(), "D:/Downloads/d.mp3".into()];
        remove_from_library(&bench.state, &bench.library, Some(paths)).unwrap();
        assert!(bench.library.get(Path::new("C:/Music/a.mp3")).unwrap().is_some(), "in a music folder: kept");
        assert!(bench.library.get(Path::new("D:/Downloads/d.mp3")).unwrap().is_none());
        assert_eq!(*lock(&bench.state.library).browser.source(), Source::Folder("C:/Music".into()));
    }

    #[test]
    fn the_rows_say_which_decks_hold_them_and_how_they_go_with_the_deck_compared_with() {
        let bench = library();
        let mut library = lock(&bench.state.library);
        library.around.decks_in_use = 2;
        library.around.decks[1] = Some(PathBuf::from("C:/Music/b.mp3"));
        library.around.playing[1] = true;
        library.around.previewed[0] = Some("C:/Music/c.mp3".into());
        library.browser.set_window(0, 10);
        let rows = library.window().rows;
        assert_eq!(rows[1].decks.iter().map(|d| (d.deck, d.playing)).collect::<Vec<_>>(), [(1, true)]);
        assert!(rows[2].previewing && !rows[0].previewing);
    }

    #[test]
    fn the_rows_are_sent_again_only_when_what_they_show_of_the_decks_changes() {
        let bench = library();
        let mut library = lock(&bench.state.library);
        library.browser.set_window(0, 10);
        let mut around = Around { decks_in_use: 2, ..Around::default() };
        assert!(!library.look_around(&around), "nothing they show");
        around.decks[0] = Some(PathBuf::from("C:/Music/a.mp3"));
        assert!(library.look_around(&around), "a deck holds one");
        around.playing[1] = true;
        assert!(!library.look_around(&around), "a deck holding none plays");
        around.compare.bpm = Some(128.0);
        assert!(!library.look_around(&around), "the rows have no tempo");
        around.previewed[0] = Some("C:/Music/b.mp3".into());
        assert!(library.look_around(&around));
    }

    #[test]
    fn the_separations_of_the_rows_are_looked_for_with_the_library_let_go_once() {
        let bench = library();
        lock(&bench.state.library).browser.set_window(0, 10);
        assert_eq!(lock(&bench.state.library).unknown().len(), 3);
        assert_eq!(shown(&bench.state, Library::window).rows.len(), 3);
        assert!(lock(&bench.state.library).unknown().is_empty(), "known from then on");
    }

    #[test]
    fn a_separation_told_while_the_rows_are_looked_for_is_kept() {
        let bench = library();
        lock(&bench.state.library).browser.set_window(0, 10);
        let unknown = lock(&bench.state.library).unknown();
        bench.state.separated("C:/Music/b.mp3", SeparationDto::Full);
        look_for_separations(&bench.state, unknown);
        let window = lock(&bench.state.library).window();
        let stems = |path: &str| window.rows.iter().find(|r| r.path == path).map(|r| r.stems);
        assert_eq!(stems("C:/Music/b.mp3"), Some(RowStemsDto::Separated { separation: SeparationDto::Full }));
        assert_eq!(stems("C:/Music/a.mp3"), Some(RowStemsDto::Separated { separation: SeparationDto::None }));
    }

    #[test]
    fn the_rows_follow_the_deck_s_tempo_as_far_as_their_pitch_shows_it() {
        let state = AppState::new(prisme_session::JsonFile::in_memory(Default::default()));
        let compared = |bpm| {
            let mut status = prisme_engine::EngineStatus::default();
            status.decks[0].bpm = Some(bpm);
            let view = crate::actions::view_with(&state, status, None, false, &Default::default(), 0.0);
            let mut around = Around::default();
            around.read(&view, 0);
            around.compare.bpm
        };
        assert_eq!(compared(124.04), compared(124.01), "a fader moving by a hair");
        assert_eq!(compared(124.2), Some(124.2));
    }

    #[test]
    fn the_rows_say_how_far_their_track_is_separated_and_whether_the_models_would_add_to_it() {
        let bench = library();
        let mut library = lock(&bench.state.library);
        library.browser.set_window(0, 10);
        let queued = HashSet::from([PathBuf::from("C:/Music/b.mp3")]);
        let mut around = Around {
            stems: RowsStems {
                current: Some((PathBuf::from("C:/Music/a.mp3"), 40)),
                queued: Queued(Arc::new(queued)),
                reach: SeparationDto::Full,
            },
            ..Around::default()
        };
        assert!(library.look_around(&around));
        let stems =
            |library: &mut Library| library.window().rows.into_iter().map(|r| r.stems).collect::<Vec<_>>();
        assert_eq!(
            stems(&mut library),
            [
                RowStemsDto::Separating { fraction: 0.4 },
                RowStemsDto::Queued,
                RowStemsDto::Addable { separation: SeparationDto::None }
            ]
        );
        around.stems = RowsStems::default();
        assert!(library.look_around(&around), "no model: nothing to add");
        assert!(stems(&mut library)
            .iter()
            .all(|s| *s == RowStemsDto::Separated { separation: SeparationDto::None }));
    }
}
