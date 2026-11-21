//! The rows in their order: by a column, the tracks without a value last, or by how well they
//! follow the deck they are compared with.

use super::{Browser, Scope};
use crate::catalogue::{Track, TrackId};
use crate::mix::{MixReference, Rank};
use std::cmp::Ordering;

/// What the rows are sorted by. `Order`: the list's own (the play order of a set); `Mix`: how
/// well the tracks follow the deck they are compared with (see [`Rank`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Column {
    Order,
    #[default]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub descending: bool,
}

impl Browser {
    /// Sorts by `column`, the other way if it already is; by `Mix`, against `mix`.
    pub fn sort_by(&mut self, column: Column, mix: Option<MixReference>) {
        let descending = self.sort.column == column && !self.sort.descending;
        self.sort = Sort { column, descending };
        if mix.is_some() {
            self.mix = mix;
        }
        self.resort();
        self.listed += 1;
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

    pub(super) fn rank(&self, mix: &MixReference, id: TrackId) -> Rank {
        Rank::of(self.catalogue.get(id), mix, self.tonight.contains(&id))
    }

    /// The rows sorted again, then searched.
    pub(super) fn resort(&mut self) {
        self.ranks = match (&self.mix, self.sort.column) {
            (Some(mix), Column::Mix) => self.rows().iter().map(|e| self.rank(mix, e.track)).collect(),
            _ => Vec::new(),
        };
        let sorted = match self.sort.column {
            column @ (Column::Title | Column::Artist | Column::Album | Column::Genre) => {
                self.sorted_by_text(column)
            }
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
    fn sorted_by_text(&self, column: Column) -> Vec<u32> {
        let rows = self.rows();
        let mut keyed: Vec<(Option<&str>, u32)> =
            (0..rows.len()).map(|r| (text(self.catalogue.get(rows[r].track), column), r as u32)).collect();
        let descending = self.sort.descending;
        keyed.sort_unstable_by(|a, b| {
            let by_text = last_missing(&a.0, &b.0);
            let by_text = if descending { by_text.reverse() } else { by_text };
            by_text.then(a.1.cmp(&b.1))
        });
        keyed.into_iter().map(|(_, r)| r).collect()
    }

    /// `row` put back in its place among the sorted rows and the rows shown: its track changed.
    pub(super) fn reposition(&mut self, row: u32) {
        if let (Some(mix), false) = (&self.mix, self.ranks.is_empty()) {
            self.ranks[row as usize] = self.rank(mix, self.rows()[row as usize].track);
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
            column @ (Column::Title | Column::Artist | Column::Album | Column::Genre) => {
                last_missing(&text(x, column), &text(y, column))
            }
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

/// The text of `track` a column of texts sorts by.
fn text(track: &Track, column: Column) -> Option<&str> {
    let texts = &track.sorted;
    match column {
        Column::Title => Some(&texts.title),
        Column::Artist => texts.artist.as_deref(),
        Column::Album => texts.album.as_deref(),
        Column::Genre => texts.genre.as_deref(),
        _ => None,
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
