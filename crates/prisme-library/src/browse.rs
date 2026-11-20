//! The library as the DJ browses it: the list shown (a folder, a crate, a set, the prepare
//! list…) or the whole library, searched, sorted and selected, held whole here while the
//! interface draws only the rows it shows ([`Browser::window`]). A search keeps the order
//! sorted, so a key typed only filters; a sort runs when the sort or the rows change. The
//! lists of the side panel are here too: the crates, the sets, the prepare list, the tracks
//! from elsewhere. Nothing is read from the disk here: what a list holds is given.

use crate::catalogue::{folded, Catalogue, Track, TrackId};
use crate::db::{Crate, CrateId, HistorySession, SessionId, TrackRecord};
use crate::duplicates::duplicate_groups;
use crate::mix::{MixReference, Rank};
use crate::scan::within;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Rows a page of the list moves by.
pub const PAGE: i64 = 12;

/// The list browsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Folder(PathBuf),
    Prepare,
    /// Tracks that look like the same song, grouped.
    Duplicates,
    /// The tracks of the library under none of the music folders.
    Elsewhere,
    Crate(CrateId),
    History(SessionId),
}

impl Source {
    /// Whether the list has an order of its own, shown in its rows.
    pub fn ordered(&self) -> bool {
        matches!(self, Source::History(_) | Source::Prepare | Source::Duplicates)
    }
}

/// Where a search looks: the list browsed, or every track of the library analysed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    List,
    Library,
}

/// What the rows are sorted by. `Order`: the list's own (the play order of a set); `Mix`: how
/// well the tracks follow the deck they are compared with (see [`Rank`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Order,
    Title,
    Artist,
    Album,
    Genre,
    Year,
    Bpm,
    Key,
    Rating,
    Plays,
    Mix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub descending: bool,
}

/// How a row is picked: `Only` alone, `Toggle` added to the selection or taken out of it,
/// `Range` with the rows from the last picked to it, `Context` alone unless it is in the
/// selection (a right click, a drag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Only,
    Toggle,
    Range,
    Context,
}

/// Where a controller's browse knob moves: the tracks, or the lists of the side panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tracks,
    Lists,
}

/// A list of the side panel a controller goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Prepare,
    Crate(CrateId),
}

/// A row of a list: its track, and when it was played in a set (one track played twice in a
/// set is two rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Entry {
    pub track: TrackId,
    pub played_at_ms: Option<i64>,
}

/// A row as it is shown.
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    pub entry: Entry,
    pub track: &'a Track,
    /// Its place in its list, from 1, in a list with an order of its own.
    pub position: Option<usize>,
    pub selected: bool,
    /// Played in the set going on.
    pub played: bool,
    /// Under none of the music folders.
    pub elsewhere: bool,
}

pub struct Browser {
    catalogue: Catalogue,
    roots: Vec<PathBuf>,
    crates: Vec<Crate>,
    sessions: Vec<HistorySession>,
    prepare: Vec<TrackId>,
    /// The tracks under none of the music folders, by path.
    elsewhere: Vec<TrackId>,
    outside: HashSet<TrackId>,
    tonight: HashSet<TrackId>,
    source: Source,
    /// The rows of the list browsed, in its own order.
    list: Vec<Entry>,
    scope: Scope,
    /// The rows of the whole library, while it is searched.
    library: Vec<Entry>,
    filter: String,
    words: Vec<String>,
    sort: Sort,
    mix: Option<MixReference>,
    /// Each row's rank, while sorted by `Mix`.
    ranks: Vec<Rank>,
    /// The rows of the scope in their sorted order, by index.
    sorted: Vec<u32>,
    /// Those of them the search finds: the rows shown.
    shown: Vec<u32>,
    cursor: Option<Entry>,
    picked: HashSet<Entry>,
    anchor: Option<Entry>,
    focus: Focus,
    pointed: Shelf,
    reference: usize,
    window: (usize, usize),
    revision: u64,
    lists_revision: u64,
    seq: u64,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            catalogue: Catalogue::default(),
            roots: Vec::new(),
            crates: Vec::new(),
            sessions: Vec::new(),
            prepare: Vec::new(),
            elsewhere: Vec::new(),
            outside: HashSet::new(),
            tonight: HashSet::new(),
            source: Source::Prepare,
            list: Vec::new(),
            scope: Scope::List,
            library: Vec::new(),
            filter: String::new(),
            words: Vec::new(),
            sort: Sort { column: Column::Title, descending: false },
            mix: None,
            ranks: Vec::new(),
            sorted: Vec::new(),
            shown: Vec::new(),
            cursor: None,
            picked: HashSet::new(),
            anchor: None,
            focus: Focus::Tracks,
            pointed: Shelf::Prepare,
            reference: 0,
            window: (0, 0),
            revision: 0,
            lists_revision: 0,
            seq: 0,
        }
    }
}

impl Browser {
    // --- the library and its lists ---

    /// The library as the database holds it all.
    pub fn load(&mut self, records: Vec<TrackRecord>) {
        self.catalogue.load(records);
        self.find_elsewhere();
        self.count_smart_crates();
        self.lists_changed();
        if self.scope == Scope::Library {
            self.library = self.analysed();
        }
        self.remake();
    }

    /// What the library knows of one track, analysed or changed.
    pub fn put(&mut self, record: TrackRecord) {
        let id = self.catalogue.put(record);
        if self.place(id) {
            self.lists_changed();
            if self.source == Source::Elsewhere {
                self.rederive();
            }
        }
        if self.scope == Scope::Library
            && self.catalogue.get(id).analysed()
            && !self.library.iter().any(|e| e.track == id)
        {
            self.library.push(Entry { track: id, played_at_ms: None });
            if self.mix.is_some() && self.sort.column == Column::Mix {
                let rank = self.rank(id);
                self.ranks.push(rank);
            }
            self.sorted.push(self.library.len() as u32 - 1);
            self.reposition(self.library.len() as u32 - 1);
            return self.changed();
        }
        let rows: Vec<u32> =
            self.rows().iter().enumerate().filter(|(_, e)| e.track == id).map(|(at, _)| at as u32).collect();
        for &row in &rows {
            self.reposition(row);
        }
        if !rows.is_empty() {
            self.changed();
        }
    }

    /// The tracks at `paths` out of the library.
    pub fn forget(&mut self, paths: &[String]) {
        for path in paths {
            if let Some(id) = self.catalogue.forget(path) {
                self.place(id);
            }
        }
        self.lists_changed();
        if self.scope == Scope::Library {
            self.library = self.analysed();
        }
        self.remake();
    }

    /// The music folders: a track under none of them is from elsewhere.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>) {
        if self.roots != roots {
            self.roots = roots;
            self.find_elsewhere();
            self.lists_changed();
            if !self.rederive() {
                self.changed();
            }
        }
    }

    /// The crates, by name, those filled by hand with their tracks counted.
    pub fn set_crates(&mut self, crates: Vec<Crate>) {
        self.crates = crates;
        self.count_smart_crates();
        self.lists_changed();
        if let Source::Crate(id) = self.source {
            if self.crate_rules(id).is_some() {
                self.rederive();
            }
        }
    }

    pub fn set_sessions(&mut self, sessions: Vec<HistorySession>) {
        self.sessions = sessions;
        self.lists_changed();
    }

    /// The tracks of the prepare list, in its order.
    pub fn set_prepare(&mut self, paths: &[String]) {
        self.prepare = paths.iter().map(|path| self.catalogue.id(path)).collect();
        self.lists_changed();
        if self.source == Source::Prepare {
            self.rederive();
        }
    }

    /// The tracks played in the set going on.
    pub fn set_tonight<'p>(&mut self, paths: impl IntoIterator<Item = &'p str>) {
        let tonight: HashSet<TrackId> =
            paths.into_iter().filter_map(|path| self.catalogue.find(path)).collect();
        if tonight != self.tonight {
            self.tonight = tonight;
            if self.sort.column == Column::Mix {
                self.resort();
            }
            self.changed();
        }
    }

    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }

    /// The track at `path`, listed without a record if it is not in the library.
    pub fn track_id(&mut self, path: &str) -> TrackId {
        self.catalogue.id(path)
    }

    pub fn crates(&self) -> &[Crate] {
        &self.crates
    }

    pub fn sessions(&self) -> &[HistorySession] {
        &self.sessions
    }

    pub fn prepare(&self) -> impl Iterator<Item = &Track> {
        self.prepare.iter().map(|&id| self.catalogue.get(id))
    }

    pub fn elsewhere(&self) -> impl Iterator<Item = &Track> {
        self.elsewhere.iter().map(|&id| self.catalogue.get(id))
    }

    pub fn is_elsewhere(&self, id: TrackId) -> bool {
        self.outside.contains(&id)
    }

    /// Those of `paths` that are tracks from elsewhere.
    pub fn elsewhere_among(&self, paths: &[String]) -> Vec<String> {
        let outside = |path: &&String| self.catalogue.find(path).is_some_and(|id| self.outside.contains(&id));
        paths.iter().filter(outside).cloned().collect()
    }

    /// The music folders, the main one first.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// The music folder `path` is in, the deepest if they hold one another.
    pub fn root_of(&self, path: &Path) -> Option<&PathBuf> {
        let holding = self.roots.iter().filter(|root| within(path, root));
        holding.max_by_key(|root| root.as_os_str().len())
    }

    /// The folder above the folder shown, inside its music folder.
    pub fn folder_above(&self) -> Option<PathBuf> {
        let Source::Folder(path) = &self.source else { return None };
        let root = self.root_of(path)?;
        (path != root).then(|| path.parent().map(Path::to_path_buf)).flatten()
    }

    /// The rules of the crate `id`, a smart one.
    pub fn crate_rules(&self, id: CrateId) -> Option<&crate::SmartRules> {
        self.crates.iter().find(|c| c.id == id)?.rules.as_ref()
    }

    /// The first `limit` tracks of the library a search for `text` finds, by the rule of the
    /// lists' searches.
    pub fn search(&self, text: &str, limit: usize) -> impl Iterator<Item = &Track> {
        let words: Vec<String> = folded(text).split_whitespace().map(String::from).collect();
        let tracks = self.catalogue.ids().map(|id| self.catalogue.get(id));
        tracks.filter(move |track| track.analysed() && track.matches(&words)).take(limit)
    }

    /// The rows of `source` if they are known here, without the disk: the prepare list, the
    /// tracks from elsewhere, the duplicates, a smart crate.
    pub fn rows_of(&self, source: &Source) -> Option<Vec<Entry>> {
        let entry = |track| Entry { track, played_at_ms: None };
        let rows = match source {
            Source::Prepare => self.prepare.iter().copied().map(entry).collect(),
            Source::Elsewhere => self.elsewhere.iter().copied().map(entry).collect(),
            Source::Duplicates => {
                let records = self.catalogue.ids().filter_map(|id| self.catalogue.get(id).record.as_ref());
                let groups = duplicate_groups(records.filter(|r| r.error.is_none()));
                groups.iter().flatten().filter_map(|r| self.catalogue.find(&r.path)).map(entry).collect()
            }
            Source::Crate(id) => {
                let rules = self.crate_rules(*id)?;
                let matching = self.catalogue.ids().filter(|&id| {
                    self.catalogue.get(id).record.as_ref().is_some_and(|record| rules.matches(record))
                });
                matching.map(entry).collect()
            }
            Source::Folder(_) | Source::History(_) => return None,
        };
        Some(rows)
    }

    /// Rows of the tracks at `paths`, each played when it says (a set's).
    pub fn entries(&mut self, paths: impl IntoIterator<Item = (String, Option<i64>)>) -> Vec<Entry> {
        let entry = |(path, played_at_ms): (String, Option<i64>)| Entry {
            track: self.catalogue.id(&path),
            played_at_ms,
        };
        paths.into_iter().map(entry).collect()
    }

    // --- the list shown ---

    /// Shows `source`, whose rows are `rows`: the whole list, nothing searched or picked, in
    /// the list's own order if it has one.
    pub fn show(&mut self, source: Source, rows: Vec<Entry>) {
        self.source = source;
        self.list = rows;
        self.picked.clear();
        self.anchor = None;
        self.scope = Scope::List;
        self.filter.clear();
        self.words.clear();
        if self.source.ordered() {
            self.sort = Sort { column: Column::Order, descending: false };
        } else if self.sort.column == Column::Order {
            self.sort = Sort { column: Column::Title, descending: false };
        }
        self.resort();
        self.changed();
    }

    /// The list shown read again, `rows`: searched, sorted and picked as it was.
    pub fn refresh(&mut self, rows: Vec<Entry>) {
        self.list = rows;
        if self.scope == Scope::List {
            self.resort();
        }
        self.changed();
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn scope(&self) -> Scope {
        self.scope
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn sort(&self) -> Sort {
        self.sort
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn pointed(&self) -> Shelf {
        self.pointed
    }

    /// The deck the tracks are compared with.
    pub fn reference(&self) -> usize {
        self.reference
    }

    /// How many rows are shown.
    pub fn count(&self) -> usize {
        self.shown.len()
    }

    /// Whether the selection can be taken out of the list shown: the prepare list's, a crate's
    /// filled by hand.
    pub fn removable(&self) -> bool {
        self.scope == Scope::List
            && match self.source {
                Source::Prepare => true,
                Source::Crate(id) => self.crate_rules(id).is_none(),
                _ => false,
            }
    }

    /// Whether the rows may be dragged to another place: the prepare list's, in its order.
    pub fn reorderable(&self) -> bool {
        self.source == Source::Prepare
            && self.scope == Scope::List
            && self.sort == Sort { column: Column::Order, descending: false }
    }

    /// Searches the rows for every word of `text`. A search narrowing the one before looks
    /// only into the rows it found.
    pub fn set_filter(&mut self, text: &str) {
        let before = folded(self.filter.trim());
        let now = folded(text.trim());
        self.filter = text.to_owned();
        self.words = now.split_whitespace().map(String::from).collect();
        let shown = if !before.is_empty() && now.starts_with(&before) {
            std::mem::take(&mut self.shown)
        } else {
            self.sorted.clone()
        };
        self.shown = shown.into_iter().filter(|&row| self.visible(row)).collect();
        self.changed();
    }

    /// Searches the list browsed, or the whole library.
    pub fn set_scope(&mut self, scope: Scope) {
        if self.scope == scope {
            return;
        }
        self.scope = scope;
        self.library = match scope {
            Scope::Library => self.analysed(),
            Scope::List => Vec::new(),
        };
        self.resort();
        self.changed();
    }

    /// Sorts by `column`, the other way if it already is; by `Mix`, against `mix`.
    pub fn sort_by(&mut self, column: Column, mix: Option<MixReference>) {
        let descending = self.sort.column == column && !self.sort.descending;
        self.sort = Sort { column, descending };
        if mix.is_some() {
            self.mix = mix;
        }
        self.resort();
        self.changed();
    }

    /// The deck the rows are ranked against as it sounds now, while they are sorted by `Mix`
    /// (another track came on it, or its analysis did): ranked again.
    pub fn set_mix(&mut self, mix: MixReference) {
        if self.mix.as_ref() != Some(&mix) {
            self.mix = Some(mix);
            if self.sort.column == Column::Mix {
                self.resort();
                self.changed();
            }
        }
    }

    pub fn compare_with(&mut self, deck: usize) {
        if self.reference != deck {
            self.reference = deck;
            self.changed();
        }
    }

    // --- the selection ---

    /// The row of the track at `path` (played at `played_at_ms`, in a set) in the rows browsed.
    pub fn find(&self, path: &str, played_at_ms: Option<i64>) -> Option<Entry> {
        let track = self.catalogue.find(path)?;
        let entry = Entry { track, played_at_ms };
        self.rows().contains(&entry).then_some(entry)
    }

    /// `entry` picked as `pick` says; it is the cursor's from then on.
    pub fn select(&mut self, entry: Entry, pick: Pick) {
        match pick {
            Pick::Only => self.picked.clear(),
            Pick::Toggle => {
                let mut picked = self.selection();
                if !picked.remove(&entry) {
                    picked.insert(entry);
                }
                self.picked = picked;
            }
            Pick::Range => {
                let Some(to) = self.index_of(entry) else { return };
                let from = self.anchor.or(self.cursor).and_then(|from| self.index_of(from)).unwrap_or(to);
                let rows = self.rows();
                self.picked =
                    self.shown[from.min(to)..=from.max(to)].iter().map(|&r| rows[r as usize]).collect();
            }
            Pick::Context if self.is_selected(entry) => return,
            Pick::Context => self.picked.clear(),
        }
        if pick != Pick::Range {
            self.anchor = Some(entry);
        }
        self.cursor = Some(entry);
        self.changed();
    }

    /// Every row shown picked.
    pub fn select_all(&mut self) {
        let rows = self.rows();
        self.picked = self.shown.iter().map(|&r| rows[r as usize]).collect();
        self.changed();
    }

    /// Nothing picked: the cursor's row alone is selected.
    pub fn clear_selection(&mut self) {
        self.picked.clear();
        self.anchor = None;
        self.changed();
    }

    /// The cursor moved by `steps` rows shown, held at the ends, to the first if it is on none;
    /// the rows picked let go.
    pub fn move_cursor(&mut self, steps: i64) {
        self.clear_selection();
        let Some(last) = self.shown.len().checked_sub(1) else { return };
        let at = match self.cursor.and_then(|cursor| self.index_of(cursor)) {
            Some(at) => (at as i64 + steps).clamp(0, last as i64) as usize,
            None => 0,
        };
        self.cursor = Some(self.rows()[self.shown[at] as usize]);
    }

    /// The cursor put on `entry`, nothing picked (where it goes after the rows it was on left).
    pub fn put_cursor(&mut self, entry: Option<Entry>) {
        self.picked.clear();
        self.anchor = None;
        self.cursor = entry;
        self.changed();
    }

    /// The row the cursor is on.
    pub fn cursor_entry(&self) -> Option<Entry> {
        self.cursor
    }

    /// The track the cursor is on, which a load takes.
    pub fn cursor(&self) -> Option<&Track> {
        self.cursor.map(|entry| self.catalogue.get(entry.track))
    }

    /// Where the cursor is among the rows shown.
    pub fn cursor_index(&self) -> Option<usize> {
        self.cursor.and_then(|cursor| self.index_of(cursor))
    }

    /// The rows the actions on the selection take, as shown: those picked, else the cursor's.
    pub fn selected(&self) -> Vec<Entry> {
        let rows = self.rows();
        self.shown.iter().map(|&r| rows[r as usize]).filter(|&e| self.is_selected(e)).collect()
    }

    /// The tracks of [`Browser::selected`], each once.
    pub fn selected_paths(&self) -> Vec<String> {
        self.paths_of(&self.selected())
    }

    /// The tracks of `entries`, each once.
    pub fn paths_of(&self, entries: &[Entry]) -> Vec<String> {
        let mut seen = HashSet::new();
        let tracks = entries.iter().filter(|e| seen.insert(e.track));
        tracks.map(|e| self.catalogue.get(e.track).path.clone()).collect()
    }

    /// The rows shown holding one of `paths`, as shown.
    pub fn rows_with(&self, paths: &[String]) -> Vec<Entry> {
        let tracks: HashSet<TrackId> = paths.iter().filter_map(|path| self.catalogue.find(path)).collect();
        let rows = self.rows();
        self.shown.iter().map(|&r| rows[r as usize]).filter(|e| tracks.contains(&e.track)).collect()
    }

    /// The row shown after the last of `entries`, else before it: where the cursor goes when
    /// they leave.
    pub fn neighbour(&self, entries: &[Entry]) -> Option<Entry> {
        let at = self.index_of(*entries.last()?)?;
        let rows = self.rows();
        let gone = |r: &&u32| !entries.contains(&rows[**r as usize]);
        let after = self.shown[at + 1..].iter().find(gone);
        after.or_else(|| self.shown[..at].iter().rev().find(gone)).map(|&r| rows[r as usize])
    }

    // --- a controller's knob over the lists ---

    pub fn set_focus(&mut self, focus: Focus) {
        if self.focus != focus {
            self.focus = focus;
            self.changed();
        }
    }

    /// The pointer on the lists of the side panel moved by `steps`, held at the ends; the knob
    /// on them.
    pub fn point(&mut self, steps: i64) {
        let shelves: Vec<Shelf> =
            std::iter::once(Shelf::Prepare).chain(self.crates.iter().map(|c| Shelf::Crate(c.id))).collect();
        let at = shelves.iter().position(|&s| s == self.pointed).unwrap_or(0) as i64;
        self.pointed = shelves[(at + steps).clamp(0, shelves.len() as i64 - 1) as usize];
        self.focus = Focus::Lists;
        self.changed();
    }

    /// The list pointed at, to open; the knob back on the tracks.
    pub fn open_pointed(&mut self) -> Source {
        self.focus = Focus::Tracks;
        self.changed();
        match self.pointed {
            Shelf::Prepare => Source::Prepare,
            Shelf::Crate(id) => Source::Crate(id),
        }
    }

    /// The list `steps` after the one shown on the side panel (else after the one pointed at),
    /// to open.
    pub fn next_list(&mut self, steps: i64) -> Source {
        match self.source {
            Source::Prepare => self.pointed = Shelf::Prepare,
            Source::Crate(id) => self.pointed = Shelf::Crate(id),
            _ => {}
        }
        self.point(steps);
        self.open_pointed()
    }

    // --- what the interface draws ---

    /// The rows the interface shows: `count` from the `first`.
    pub fn set_window(&mut self, first: usize, count: usize) {
        if self.window != (first, count) {
            self.window = (first, count);
            self.seq += 1;
        }
    }

    /// The first row of the window, within the rows shown.
    pub fn window_first(&self) -> usize {
        self.window.0.min(self.shown.len())
    }

    /// The rows of the window.
    pub fn window(&self) -> impl Iterator<Item = Row<'_>> {
        let first = self.window_first();
        let last = (first + self.window.1).min(self.shown.len());
        let rows = self.rows();
        let ordered = self.scope == Scope::List && self.source.ordered();
        self.shown[first..last].iter().map(move |&r| {
            let entry = rows[r as usize];
            Row {
                entry,
                track: self.catalogue.get(entry.track),
                position: ordered.then_some(r as usize + 1),
                selected: self.is_selected(entry),
                played: self.tonight.contains(&entry.track),
                elsewhere: self.outside.contains(&entry.track),
            }
        })
    }

    /// Moved by every change of what is shown.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Moved by every change of the lists of the side panel.
    pub fn lists_revision(&self) -> u64 {
        self.lists_revision
    }

    /// Moved by every change of what is shown and of the window: what the interface was sent
    /// last is the one with the highest.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    // --- inside ---

    fn rows(&self) -> &[Entry] {
        match self.scope {
            Scope::List => &self.list,
            Scope::Library => &self.library,
        }
    }

    fn changed(&mut self) {
        self.revision += 1;
        self.seq += 1;
    }

    fn lists_changed(&mut self) {
        self.lists_revision += 1;
    }

    fn analysed(&self) -> Vec<Entry> {
        let analysed = self.catalogue.ids().filter(|&id| self.catalogue.get(id).analysed());
        analysed.map(|track| Entry { track, played_at_ms: None }).collect()
    }

    /// The list shown made again, if it is made here: what it holds changed. Whether it is.
    fn rederive(&mut self) -> bool {
        let Some(rows) = self.rows_of(&self.source) else { return false };
        self.refresh(rows);
        true
    }

    /// The rows made again and sorted, their tracks changed.
    fn remake(&mut self) {
        if !self.rederive() {
            self.resort();
            self.changed();
        }
    }

    /// `id` among the tracks from elsewhere, or not, as its path says: whether that changed.
    fn place(&mut self, id: TrackId) -> bool {
        let track = self.catalogue.get(id);
        let outside =
            track.record.is_some() && !self.roots.iter().any(|root| within(Path::new(&track.path), root));
        if outside == self.outside.contains(&id) {
            return false;
        }
        if outside {
            self.outside.insert(id);
            let at = self.elsewhere.partition_point(|&e| self.catalogue.get(e).path < track.path);
            self.elsewhere.insert(at, id);
        } else {
            self.outside.remove(&id);
            self.elsewhere.retain(|&e| e != id);
        }
        true
    }

    fn find_elsewhere(&mut self) {
        let roots = &self.roots;
        let mut elsewhere: Vec<TrackId> = self
            .catalogue
            .ids()
            .filter(|&id| !roots.iter().any(|root| within(Path::new(&self.catalogue.get(id).path), root)))
            .collect();
        elsewhere.sort_by(|&a, &b| self.catalogue.get(a).path.cmp(&self.catalogue.get(b).path));
        self.outside = elsewhere.iter().copied().collect();
        self.elsewhere = elsewhere;
    }

    fn count_smart_crates(&mut self) {
        let records: Vec<&TrackRecord> =
            self.catalogue.ids().filter_map(|id| self.catalogue.get(id).record.as_ref()).collect();
        for c in &mut self.crates {
            if let Some(rules) = &c.rules {
                c.track_count = records.iter().filter(|r| rules.matches(r)).count();
            }
        }
    }

    fn rank(&self, id: TrackId) -> Rank {
        let mix = self.mix.as_ref().expect("ranked against a reference");
        Rank::of(self.catalogue.get(id), mix, self.tonight.contains(&id))
    }

    /// The rows sorted again, then searched.
    fn resort(&mut self) {
        self.ranks = match (&self.mix, self.sort.column) {
            (Some(_), Column::Mix) => self.rows().iter().map(|e| self.rank(e.track)).collect(),
            _ => Vec::new(),
        };
        let sorted = match self.sort.column {
            Column::Title => self.sorted_by_text(0),
            Column::Artist => self.sorted_by_text(1),
            Column::Album => self.sorted_by_text(2),
            Column::Genre => self.sorted_by_text(3),
            _ => {
                let mut sorted: Vec<u32> = (0..self.rows().len() as u32).collect();
                sorted.sort_unstable_by(|&a, &b| self.compare(a, b));
                sorted
            }
        };
        self.shown = sorted.iter().copied().filter(|&row| self.visible(row)).collect();
        self.sorted = sorted;
    }

    /// The rows sorted by the text of their tracks' `field` (see `Track::sorted`), as
    /// [`Browser::compare`] does, its texts read once.
    fn sorted_by_text(&self, field: usize) -> Vec<u32> {
        let rows = self.rows();
        let mut keyed: Vec<(Option<&str>, u32)> = (0..rows.len())
            .map(|r| (self.catalogue.get(rows[r].track).sorted[field].as_deref(), r as u32))
            .collect();
        let descending = self.sort.descending;
        keyed.sort_unstable_by(|a, b| {
            let by_text = last_missing(&a.0, &b.0);
            let by_text = if descending { by_text.reverse() } else { by_text };
            by_text.then(a.1.cmp(&b.1))
        });
        keyed.into_iter().map(|(_, r)| r).collect()
    }

    /// `row` put back in its place among the sorted rows and the rows shown: its track changed.
    fn reposition(&mut self, row: u32) {
        if !self.ranks.is_empty() {
            self.ranks[row as usize] = self.rank(self.rows()[row as usize].track);
        }
        if let Some(at) = self.sorted.iter().position(|&r| r == row) {
            self.sorted.remove(at);
        }
        let at = self.sorted.partition_point(|&r| self.compare(r, row) == Ordering::Less);
        self.sorted.insert(at, row);
        if let Some(at) = self.shown.iter().position(|&r| r == row) {
            self.shown.remove(at);
        }
        if self.visible(row) {
            let at = self.shown.partition_point(|&r| self.compare(r, row) == Ordering::Less);
            self.shown.insert(at, row);
        }
    }

    fn visible(&self, row: u32) -> bool {
        let track = self.catalogue.get(self.rows()[row as usize].track);
        track.matches(&self.words) && (self.scope == Scope::List || track.analysed())
    }

    fn is_selected(&self, entry: Entry) -> bool {
        if self.picked.is_empty() {
            self.cursor == Some(entry)
        } else {
            self.picked.contains(&entry)
        }
    }

    fn selection(&self) -> HashSet<Entry> {
        match (self.picked.is_empty(), self.cursor) {
            (false, _) => self.picked.clone(),
            (true, Some(cursor)) => HashSet::from([cursor]),
            (true, None) => HashSet::new(),
        }
    }

    fn index_of(&self, entry: Entry) -> Option<usize> {
        let rows = self.rows();
        self.shown.iter().position(|&r| rows[r as usize] == entry)
    }

    /// The sorted order of two rows: by the sort's column (the tracks without a value last,
    /// whichever way), then by their place in the list.
    fn compare(&self, a: u32, b: u32) -> Ordering {
        let by_column = self.by_column(a, b);
        let by_column = if self.sort.descending { by_column.reverse() } else { by_column };
        by_column.then(a.cmp(&b))
    }

    fn by_column(&self, a: u32, b: u32) -> Ordering {
        let rows = self.rows();
        let (x, y) = (self.catalogue.get(rows[a as usize].track), self.catalogue.get(rows[b as usize].track));
        let marks = |t: &Track| t.record.as_ref().map(|r| (r.marks.rating, r.play_count)).unwrap_or_default();
        match self.sort.column {
            Column::Order => match self.scope {
                Scope::List => a.cmp(&b),
                Scope::Library => Ordering::Equal,
            },
            Column::Title => x.sorted[0].cmp(&y.sorted[0]),
            Column::Artist => last_missing(&x.sorted[1], &y.sorted[1]),
            Column::Album => last_missing(&x.sorted[2], &y.sorted[2]),
            Column::Genre => last_missing(&x.sorted[3], &y.sorted[3]),
            Column::Year => last_missing(&year(x), &year(y)),
            Column::Bpm => match (x.bpm, y.bpm) {
                (Some(a), Some(b)) => a.total_cmp(&b),
                (a, b) => b.is_some().cmp(&a.is_some()),
            },
            Column::Key => key_rank(x).cmp(&key_rank(y)),
            // the best rated and most played first
            Column::Rating => marks(y).0.cmp(&marks(x).0),
            Column::Plays => marks(y).1.cmp(&marks(x).1),
            Column::Mix => match self.ranks.is_empty() {
                true => Ordering::Equal,
                false => self.ranks[a as usize].cmp(&self.ranks[b as usize]),
            },
        }
    }
}

fn last_missing<T: Ord>(a: &Option<T>, b: &Option<T>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(b),
        (a, b) => b.is_some().cmp(&a.is_some()),
    }
}

fn year(track: &Track) -> Option<u16> {
    track.record.as_ref()?.tags.year
}

/// 1A, 1B, 2A … 12B, then the tracks without a key.
fn key_rank(track: &Track) -> u8 {
    track.key.map_or(u8::MAX, |key| {
        key.camelot_number() * 2 + u8::from(key.mode() == prisme_analysis::Mode::Major)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::tests::{record, tagged};
    use crate::smart::{Rule, SmartRules, TextField};
    use prisme_analysis::Key;
    use prisme_media::TrackTags;

    fn tags(title: &str, artist: Option<&str>, genre: Option<&str>) -> TrackTags {
        TrackTags {
            title: Some(title.into()),
            artist: artist.map(Into::into),
            genre: genre.map(Into::into),
            ..TrackTags::default()
        }
    }

    /// A folder of four tracks, three analysed, shown; a fifth track from elsewhere.
    fn folder() -> Browser {
        let mut browser = Browser::default();
        browser.set_roots(vec![PathBuf::from("C:/Music")]);
        browser.load(vec![
            record(
                "C:/Music/a.mp3",
                tags("Strobe", Some("deadmau5"), Some("Progressive House")),
                Some(128.0),
                Some("8A"),
            ),
            record("C:/Music/b.mp3", tags("Alone", Some("Claptone"), None), Some(122.0), Some("3B")),
            record("C:/Music/c.mp3", tags("Café Noir", None, Some("House")), None, None),
            record("D:/Downloads/d.mp3", tags("Dropped", Some("Someone"), None), Some(174.0), Some("1A")),
        ]);
        let rows = browser
            .entries(["a.mp3", "b.mp3", "c.mp3", "e.mp3"].map(|name| (format!("C:/Music/{name}"), None)));
        browser.show(Source::Folder("C:/Music".into()), rows);
        browser
    }

    fn titles(browser: &mut Browser) -> Vec<String> {
        browser.set_window(0, 100);
        browser.window().map(|row| row.track.title.clone()).collect()
    }

    fn entry(browser: &Browser, name: &str) -> Entry {
        browser.find(&format!("C:/Music/{name}"), None).unwrap()
    }

    #[test]
    fn the_library_is_searched_by_the_lists_rule() {
        let browser = folder();
        let found = |text: &str| browser.search(text, 10).map(|t| t.title.clone()).collect::<Vec<_>>();
        assert_eq!(found("ouse"), ["Strobe", "Café Noir"]);
        assert_eq!(found("e.mp3"), Vec::<String>::new(), "not analysed");
        assert_eq!(found("o").len(), 4);
        assert_eq!(browser.search("o", 2).count(), 2);
    }

    #[test]
    fn a_list_is_shown_by_title_and_searched_by_every_word_in_one_way() {
        let mut browser = folder();
        assert_eq!(titles(&mut browser), ["Alone", "Café Noir", "e", "Strobe"]);
        browser.set_filter("ouse");
        assert_eq!(titles(&mut browser), ["Café Noir", "Strobe"], "a word inside another");
        browser.set_filter("ouse  cafe");
        assert_eq!(titles(&mut browser), ["Café Noir"], "accents aside");
        browser.set_filter("e.mp3");
        assert_eq!(titles(&mut browser), ["e"], "the file name");
        browser.set_filter("");
        assert_eq!(browser.count(), 4);
        browser.set_scope(Scope::Library);
        browser.set_filter("o");
        assert_eq!(
            titles(&mut browser),
            ["Alone", "Café Noir", "Dropped", "Strobe"],
            "the library, by the same rule"
        );
    }

    #[test]
    fn each_column_sorts_the_tracks_without_a_value_last_and_again_the_other_way() {
        let mut browser = folder();
        browser.sort_by(Column::Bpm, None);
        assert_eq!(titles(&mut browser), ["Alone", "Strobe", "Café Noir", "e"]);
        browser.sort_by(Column::Bpm, None);
        assert_eq!(
            titles(&mut browser),
            ["Café Noir", "e", "Strobe", "Alone"],
            "reversed, ties in their order"
        );
        browser.sort_by(Column::Artist, None);
        assert_eq!(titles(&mut browser), ["Alone", "Strobe", "Café Noir", "e"]);
        browser.sort_by(Column::Key, None);
        assert_eq!(titles(&mut browser), ["Alone", "Strobe", "Café Noir", "e"], "3B before 8A");
        browser.sort_by(Column::Genre, None);
        assert_eq!(titles(&mut browser), ["Café Noir", "Strobe", "Alone", "e"]);
    }

    #[test]
    fn a_list_with_an_order_of_its_own_opens_in_it_and_numbers_its_rows() {
        let mut browser = folder();
        let rows = browser.entries(["b", "a", "b"].map(|name| (format!("C:/Music/{name}.mp3"), None)));
        browser.show(Source::Prepare, rows);
        assert_eq!(browser.sort(), Sort { column: Column::Order, descending: false });
        browser.set_window(0, 10);
        let rows: Vec<_> = browser.window().map(|r| (r.track.title.clone(), r.position)).collect();
        assert_eq!(rows, [("Alone".into(), Some(1)), ("Strobe".into(), Some(2)), ("Alone".into(), Some(3))]);
        assert!(browser.reorderable() && browser.removable());
        let folder = browser.entries([("C:/Music/a.mp3".to_string(), None)]);
        browser.show(Source::Folder("C:/Music".into()), folder);
        assert_eq!(browser.sort().column, Column::Title, "a folder has no order of its own");
        assert!(!browser.removable());
    }

    #[test]
    fn a_track_played_twice_in_a_set_is_two_rows_picked_apart() {
        let mut browser = folder();
        let rows = browser.entries([1, 2].map(|at| ("C:/Music/a.mp3".to_string(), Some(at))));
        browser.show(Source::History(SessionId(1)), rows);
        let second = browser.find("C:/Music/a.mp3", Some(2)).unwrap();
        browser.select(second, Pick::Only);
        assert_eq!(browser.selected(), [second]);
        browser.select_all();
        assert_eq!(browser.selected_paths(), ["C:/Music/a.mp3"], "one track");
    }

    #[test]
    fn rows_are_picked_alone_added_or_by_range_and_the_cursor_moves_through_those_shown() {
        let mut browser = folder();
        let [alone, cafe, e, strobe] = ["b.mp3", "c.mp3", "e.mp3", "a.mp3"].map(|n| entry(&browser, n));
        browser.select(cafe, Pick::Only);
        browser.select(strobe, Pick::Range);
        assert_eq!(browser.selected(), [cafe, e, strobe]);
        browser.select(e, Pick::Toggle);
        assert_eq!(browser.selected(), [cafe, strobe]);
        browser.select(cafe, Pick::Context);
        assert_eq!(browser.selected(), [cafe, strobe], "a right click in the selection keeps it");
        browser.select(alone, Pick::Context);
        assert_eq!(browser.selected(), [alone]);
        browser.move_cursor(2);
        assert_eq!(browser.selected(), [e]);
        browser.move_cursor(-9);
        assert_eq!(browser.cursor_index(), Some(0), "held at the ends");
        browser.select_all();
        assert_eq!(browser.selected_paths().len(), 4);
        browser.set_filter("house");
        assert_eq!(browser.selected(), [cafe, strobe], "the picks hidden by the search left out");
        browser.clear_selection();
        assert_eq!(browser.selected(), [], "the cursor's row is hidden");
    }

    #[test]
    fn the_cursor_goes_to_the_row_after_those_that_leave_else_before() {
        let browser = folder();
        let [alone, cafe, e, strobe] = ["b.mp3", "c.mp3", "e.mp3", "a.mp3"].map(|n| entry(&browser, n));
        assert_eq!(browser.neighbour(&[cafe, e]), Some(strobe));
        assert_eq!(browser.neighbour(&[e, strobe]), Some(cafe));
        assert_eq!(browser.neighbour(&[alone, cafe, e, strobe]), None);
    }

    #[test]
    fn a_track_analysed_takes_its_place_in_the_list_and_joins_the_library_searched() {
        let mut browser = folder();
        browser.put(record("C:/Music/e.mp3", tags("Aardvark", None, None), Some(100.0), None));
        assert_eq!(titles(&mut browser), ["Aardvark", "Alone", "Café Noir", "Strobe"]);
        browser.set_scope(Scope::Library);
        browser.set_filter("a");
        let before = browser.count();
        browser.put(tagged("C:/Music/new.mp3", "Zebra", "a"));
        assert_eq!(browser.count(), before + 1);
        assert_eq!(titles(&mut browser).last().map(String::as_str), Some("Zebra"));
        browser.forget(&["C:/Music/new.mp3".into()]);
        assert_eq!(browser.count(), before);
    }

    #[test]
    fn the_tracks_under_none_of_the_music_folders_are_from_elsewhere() {
        let mut browser = folder();
        let names = |b: &Browser| b.elsewhere().map(|t| t.title.clone()).collect::<Vec<_>>();
        assert_eq!(names(&browser), ["Dropped"]);
        browser.put(tagged("E:/Sets/x.mp3", "X", "a"));
        assert_eq!(names(&browser), ["Dropped", "X"]);
        browser.set_roots(vec!["C:/Music".into(), "E:/Sets".into()]);
        assert_eq!(names(&browser), ["Dropped"]);
        let rows = browser.rows_of(&Source::Elsewhere).unwrap();
        browser.show(Source::Elsewhere, rows);
        browser.forget(&["D:/Downloads/d.mp3".into()]);
        assert_eq!(browser.count(), 0, "the list follows");
    }

    #[test]
    fn a_smart_crate_picks_its_tracks_from_the_library_and_counts_them() {
        let mut browser = folder();
        let rules = SmartRules {
            match_all: true,
            rules: vec![Rule::Text { field: TextField::Genre, contains: "house".into() }],
        };
        let smart = Crate { id: CrateId(2), name: "House".into(), rules: Some(rules), track_count: 0 };
        let hand = Crate { id: CrateId(1), name: "Hand".into(), rules: None, track_count: 7 };
        browser.set_crates(vec![hand, smart]);
        assert_eq!(browser.crates().iter().map(|c| c.track_count).collect::<Vec<_>>(), [7, 2]);
        let rows = browser.rows_of(&Source::Crate(CrateId(2))).unwrap();
        browser.show(Source::Crate(CrateId(2)), rows);
        assert_eq!(titles(&mut browser), ["Café Noir", "Strobe"]);
        assert!(!browser.removable(), "it picks its own");
        assert!(browser.rows_of(&Source::Crate(CrateId(1))).is_none(), "the database's");
    }

    #[test]
    fn a_controller_points_at_the_lists_held_at_the_ends_and_opens_the_next() {
        let mut browser = folder();
        let named =
            |id, name: &str| Crate { id: CrateId(id), name: name.into(), rules: None, track_count: 0 };
        browser.set_crates(vec![named(3, "A"), named(7, "B")]);
        browser.point(1);
        assert_eq!((browser.pointed(), browser.focus()), (Shelf::Crate(CrateId(3)), Focus::Lists));
        browser.point(5);
        assert_eq!(browser.pointed(), Shelf::Crate(CrateId(7)));
        assert_eq!(browser.open_pointed(), Source::Crate(CrateId(7)));
        assert_eq!(browser.focus(), Focus::Tracks);
        browser.show(Source::Prepare, Vec::new());
        assert_eq!(browser.next_list(1), Source::Crate(CrateId(3)), "after the list shown");
        browser.point(-4);
        assert_eq!(browser.pointed(), Shelf::Prepare);
    }

    #[test]
    fn the_rows_follow_the_deck_they_are_ranked_against() {
        let mut browser = folder();
        let mix = |key: &str| MixReference {
            bpm: Some(128.0),
            key: Key::from_camelot(key),
            path: None,
            range: 0.08,
            key_lock: true,
        };
        browser.sort_by(Column::Mix, Some(mix("8A")));
        assert_eq!(titles(&mut browser)[..2], ["Strobe", "Alone"]);
        browser.set_mix(mix("3B"));
        assert_eq!(titles(&mut browser)[..2], ["Alone", "Strobe"]);
        browser.set_tonight(["C:/Music/b.mp3"]);
        assert_eq!(titles(&mut browser)[..2], ["Strobe", "Café Noir"], "played tonight: last");
    }

    #[test]
    fn the_window_holds_the_rows_asked_for_and_says_what_each_row_is() {
        let mut browser = folder();
        browser.set_tonight(["C:/Music/a.mp3"]);
        let strobe = entry(&browser, "a.mp3");
        browser.select(strobe, Pick::Only);
        browser.set_window(3, 10);
        let rows: Vec<_> = browser.window().map(|r| (r.track.title.clone(), r.selected, r.played)).collect();
        assert_eq!(rows, [("Strobe".into(), true, true)]);
        assert_eq!(browser.window_first(), 3);
        let seq = browser.seq();
        browser.set_window(3, 10);
        assert_eq!(browser.seq(), seq, "the same window");
        browser.set_filter("zzz");
        assert_eq!((browser.window_first(), browser.window().count()), (0, 0));
        assert!(browser.seq() > seq);
    }

    #[test]
    fn duplicates_and_the_prepare_list_are_made_from_the_library() {
        let mut browser = folder();
        browser.put(record("C:/Music/copy.mp3", tags("Strobe!", Some("Deadmau5"), None), None, None));
        assert_eq!(browser.rows_of(&Source::Duplicates).unwrap().len(), 2);
        browser.set_prepare(&["C:/Music/c.mp3".into(), "C:/Music/a.mp3".into()]);
        let rows = browser.rows_of(&Source::Prepare).unwrap();
        browser.show(Source::Prepare, rows);
        assert_eq!(titles(&mut browser), ["Café Noir", "Strobe"]);
        browser.set_prepare(&["C:/Music/a.mp3".into()]);
        assert_eq!(titles(&mut browser), ["Strobe"], "the list shown follows");
    }
}
