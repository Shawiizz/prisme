//! The rows picked, and the cursor among the rows shown: what the actions on the selection
//! take.

use super::{Browser, Entry};
use crate::catalogue::{Track, TrackId};
use std::collections::HashSet;

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

impl Browser {
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
        self.set_cursor(Some(entry));
        self.changed();
    }

    /// The rows shown from `from` to `to` picked, as a frame drawn over them takes them: the
    /// cursor on `to`, a Shift click going on from `from`. Held to the rows there are.
    pub fn select_span(&mut self, from: usize, to: usize) {
        let Some(last) = self.shown.len().checked_sub(1) else { return };
        let (from, to) = (from.min(last), to.min(last));
        let rows = self.rows();
        let entry = |at: usize| rows[self.shown[at] as usize];
        let picked = (from.min(to)..=from.max(to)).map(entry).collect();
        let (anchor, cursor) = (entry(from), entry(to));
        self.picked = picked;
        self.anchor = Some(anchor);
        self.set_cursor(Some(cursor));
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
        self.picked.clear();
        self.anchor = None;
        if let Some(last) = self.shown.len().checked_sub(1) {
            let at = match self.cursor_at {
                Some(at) => (at as i64 + steps).clamp(0, last as i64) as usize,
                None => 0,
            };
            self.set_cursor(Some(self.rows()[self.shown[at] as usize]));
        }
        self.changed();
    }

    /// The cursor put on `entry`, nothing picked (where it goes after the rows it was on left).
    pub fn put_cursor(&mut self, entry: Option<Entry>) {
        self.picked.clear();
        self.anchor = None;
        self.set_cursor(entry);
        self.changed();
    }

    /// The row the cursor is on.
    pub fn cursor_entry(&self) -> Option<Entry> {
        self.cursor
    }

    /// The track the cursor is on while its row is shown, which a load takes.
    pub fn cursor(&self) -> Option<&Track> {
        self.cursor_at.and(self.cursor).map(|entry| self.catalogue.get(entry.track))
    }

    /// Where the cursor is among the rows shown.
    pub fn cursor_index(&self) -> Option<usize> {
        self.cursor_at
    }

    /// Moved each time the cursor goes to another row, by a gesture or because the rows it was
    /// on left; not when rows before it come, go or move.
    pub fn cursor_moves(&self) -> u64 {
        self.cursor_moves
    }

    /// How many tracks [`Browser::selected`] holds.
    pub fn selected_count(&self) -> usize {
        if self.picked.is_empty() {
            return usize::from(self.cursor_at.is_some());
        }
        let rows = self.rows();
        let picked = self.shown.iter().map(|&r| rows[r as usize]).filter(|e| self.picked.contains(e));
        picked.map(|e| e.track).collect::<HashSet<_>>().len()
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

    /// The cursor on `entry`, whatever is shown.
    fn set_cursor(&mut self, entry: Option<Entry>) {
        if self.cursor != entry {
            self.cursor = entry;
            self.cursor_moves += 1;
        }
    }

    pub(super) fn is_selected(&self, entry: Entry) -> bool {
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

    pub(super) fn index_of(&self, entry: Entry) -> Option<usize> {
        let rows = self.rows();
        self.shown.iter().position(|&r| rows[r as usize] == entry)
    }
}
