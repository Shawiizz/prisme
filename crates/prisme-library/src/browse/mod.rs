//! The library as the DJ browses it: the list shown (a folder, a crate, a set, the prepare
//! list…) or the whole library, searched, sorted and selected, held whole here while the
//! interface draws only the rows it shows ([`Browser::window`]). A search keeps the order
//! sorted, so a key typed only filters; a sort runs when the sort or the rows change. The
//! lists of the side panel are here too: the crates, the sets, the prepare list, the tracks
//! from elsewhere. Nothing is read from the disk here: what a list holds is given.

mod lists;
mod selection;
mod sort;
#[cfg(test)]
mod tests;

pub use lists::{Focus, Shelf};
pub use selection::Pick;
pub use sort::{Column, Sort};

use crate::catalogue::{folded, search_words, Catalogue, Track, TrackId, Tracks};
use crate::db::{Crate, CrateId, HistorySession, SessionId, TrackRecord};
use crate::mix::{MixReference, Rank};
use lists::Elsewhere;
use std::collections::HashSet;
use std::path::PathBuf;

/// Rows a page of the list moves by.
pub const PAGE: i64 = 12;

/// The list browsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Source {
    Folder(PathBuf),
    #[default]
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    List,
    Library,
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

#[derive(Default)]
pub struct Browser {
    catalogue: Catalogue,
    roots: Vec<PathBuf>,
    crates: Vec<Crate>,
    sessions: Vec<HistorySession>,
    prepare: Vec<TrackId>,
    elsewhere: Elsewhere,
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
    /// Where the cursor is among the rows shown, found again at each change.
    cursor_at: Option<usize>,
    /// Moved each time the cursor goes to another row.
    cursor_moves: u64,
    /// Moved each time another list is shown, or the list is searched, sorted or scoped.
    listed: u64,
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

/// The lists a track belongs to by what it is, not by where its file is: the smart crates it
/// matches, in their order, and its song, which its duplicates share.
#[derive(PartialEq)]
struct Belongs {
    smart: Vec<bool>,
    song: Option<String>,
}

impl Belongs {
    /// A track the library does not know, `smart` crates kept.
    fn nowhere(smart: usize) -> Self {
        Self { smart: vec![false; smart], song: None }
    }
}

impl Browser {
    /// The library as the database holds it all.
    pub fn load(&mut self, tracks: Tracks) {
        self.catalogue.load(tracks);
        self.find_elsewhere();
        self.count_smart_crates();
        self.lists_changed();
        if self.scope == Scope::Library {
            self.library = self.analysed();
        }
        self.remake();
    }

    /// What the library knows of one track, analysed or changed: in the lists it now belongs
    /// to, those shown and the counts of the side panel, at once.
    pub fn put(&mut self, record: TrackRecord) {
        let before = match self.catalogue.find(&record.path) {
            Some(id) => self.belongs(id),
            None => Belongs::nowhere(self.crates.iter().filter(|c| c.rules.is_some()).count()),
        };
        let id = self.catalogue.put(record);
        let placed = self.place(id);
        let now = self.belongs(id);
        let smart = self.crates.iter_mut().filter(|c| c.rules.is_some());
        for (c, (was, is)) in smart.zip(before.smart.iter().zip(&now.smart)) {
            c.track_count = (c.track_count + usize::from(*is)).saturating_sub(usize::from(*was));
        }
        if placed || before != now {
            self.lists_changed();
        }
        let shown_moved = match self.source {
            Source::Elsewhere => placed,
            Source::Duplicates => before.song != now.song,
            Source::Crate(shown) => {
                let at = self.crates.iter().filter(|c| c.rules.is_some()).position(|c| c.id == shown);
                at.is_some_and(|at| before.smart[at] != now.smart[at])
            }
            _ => false,
        };
        if shown_moved {
            self.rederive();
        }
        if self.scope == Scope::Library
            && self.catalogue.get(id).analysed()
            && !self.library.iter().any(|e| e.track == id)
        {
            self.library.push(Entry { track: id, played_at_ms: None });
            if let (Some(mix), Column::Mix) = (&self.mix, self.sort.column) {
                let rank = self.rank(mix, id);
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
        self.count_smart_crates();
        self.lists_changed();
        if self.scope == Scope::Library {
            self.library = self.analysed();
        }
        self.remake();
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

    /// The first `limit` tracks of the library a search for `text` finds, by the rule of the
    /// lists' searches.
    pub fn search(&self, text: &str, limit: usize) -> impl Iterator<Item = &Track> {
        let words = search_words(text);
        let tracks = self.catalogue.ids().map(|id| self.catalogue.get(id));
        tracks.filter(move |track| track.analysed() && track.matches(&words)).take(limit)
    }

    /// Rows of the tracks at `paths`, each played when it says (a set's).
    pub fn entries(&mut self, paths: impl IntoIterator<Item = (String, Option<i64>)>) -> Vec<Entry> {
        let entry = |(path, played_at_ms): (String, Option<i64>)| Entry {
            track: self.catalogue.id(&path),
            played_at_ms,
        };
        paths.into_iter().map(entry).collect()
    }

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
        self.listed += 1;
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
        self.words = search_words(text);
        self.shown = if !before.is_empty() && now.starts_with(&before) {
            let shown = std::mem::take(&mut self.shown);
            shown.into_iter().filter(|&row| self.visible(row)).collect()
        } else {
            self.sorted.iter().copied().filter(|&row| self.visible(row)).collect()
        };
        self.listed += 1;
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
        self.listed += 1;
        self.changed();
    }

    pub fn compare_with(&mut self, deck: usize) {
        if self.reference != deck {
            self.reference = deck;
            self.changed();
        }
    }

    /// Moved each time another list is shown, or the list is searched, sorted or scoped: a
    /// cursor moved meanwhile was not moved within the list.
    pub fn listed(&self) -> u64 {
        self.listed
    }

    /// The rows the interface shows: `count` from the `first`.
    pub fn set_window(&mut self, first: usize, count: usize) {
        if self.window != (first, count) {
            self.window = (first, count);
            self.seq += 1;
        }
    }

    /// The first row of the window, within the rows shown: those at the end if the list got
    /// shorter than where the window was.
    pub fn window_first(&self) -> usize {
        self.window.0.min(self.shown.len().saturating_sub(self.window.1))
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
                elsewhere: self.elsewhere.contains(entry.track),
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

    fn rows(&self) -> &[Entry] {
        match self.scope {
            Scope::List => &self.list,
            Scope::Library => &self.library,
        }
    }

    fn changed(&mut self) {
        self.cursor_at = self.cursor.and_then(|cursor| self.index_of(cursor));
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

    fn visible(&self, row: u32) -> bool {
        let track = self.catalogue.get(self.rows()[row as usize].track);
        track.matches(&self.words) && (self.scope == Scope::List || track.analysed())
    }
}
