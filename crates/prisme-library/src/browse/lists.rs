//! The lists of the side panel: the crates and their smart counts, the sets, the prepare list,
//! the music folders and the tracks under none of them; and a controller's knob over them.

use super::{Belongs, Browser, Entry, List};
use crate::catalogue::{folded, Track, TrackId};
use crate::db::{Crate, CrateId, HistorySession, TrackRecord};
use crate::duplicates::{duplicate_groups, song_key};
use crate::scan::{root_of, within};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where a controller's browse knob moves: the tracks, or the lists of the side panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Tracks,
    Lists,
}

/// A list of the side panel a controller goes through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Shelf {
    #[default]
    Prepare,
    Crate(CrateId),
}

/// The tracks under none of the music folders: by path, as their list shows them, and the same
/// by id, to tell one at once.
#[derive(Default)]
pub(super) struct Elsewhere {
    by_path: Vec<TrackId>,
    ids: HashSet<TrackId>,
}

impl Elsewhere {
    pub(super) fn contains(&self, id: TrackId) -> bool {
        self.ids.contains(&id)
    }
}

impl Browser {
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
        if let List::Crate(id) = self.source {
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
        if self.source == List::Prepare {
            self.rederive();
        }
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
        self.elsewhere.by_path.iter().map(|&id| self.catalogue.get(id))
    }

    /// Those of `paths` that are tracks from elsewhere.
    pub fn elsewhere_among(&self, paths: &[String]) -> Vec<String> {
        let outside =
            |path: &&String| self.catalogue.find(path).is_some_and(|id| self.elsewhere.contains(id));
        paths.iter().filter(outside).cloned().collect()
    }

    /// The music folders, the main one first.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// The main music folder, which stays (none before the folders are known).
    pub fn main_root(&self) -> Option<&PathBuf> {
        self.roots.first()
    }

    /// The music folder `path` is in, the deepest if they hold one another.
    pub fn root_of(&self, path: &Path) -> Option<&PathBuf> {
        root_of(path, &self.roots)
    }

    /// The folder above the folder shown, inside its music folder.
    pub fn folder_above(&self) -> Option<PathBuf> {
        let List::Folder(path) = &self.source else { return None };
        let root = self.root_of(path)?;
        (path != root).then(|| path.parent().map(Path::to_path_buf)).flatten()
    }

    /// The rules of the crate `id`, a smart one.
    pub fn crate_rules(&self, id: CrateId) -> Option<&crate::SmartRules> {
        self.crates.iter().find(|c| c.id == id)?.rules.as_ref()
    }

    /// The rows of `source` if they are known here, without the disk: the prepare list, the
    /// tracks from elsewhere, the duplicates, a smart crate.
    pub fn rows_of(&self, source: &List) -> Option<Vec<Entry>> {
        let entry = |track| Entry { track, played_at_ms: None };
        let rows = match source {
            List::Prepare => self.prepare.iter().copied().map(entry).collect(),
            List::Elsewhere => self.elsewhere.by_path.iter().copied().map(entry).collect(),
            List::Duplicates => {
                let records = self.catalogue.ids().filter_map(|id| self.catalogue.get(id).record.as_ref());
                let groups = duplicate_groups(records.filter(|r| r.error.is_none()));
                groups.iter().flatten().filter_map(|r| self.catalogue.find(&r.path)).map(entry).collect()
            }
            List::Crate(id) => {
                let rules = self.crate_rules(*id)?;
                let matching = self.catalogue.ids().filter(|&id| {
                    self.catalogue.get(id).record.as_ref().is_some_and(|record| rules.matches(record))
                });
                matching.map(entry).collect()
            }
            List::Folder(_) | List::History(_) => return None,
        };
        Some(rows)
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn pointed(&self) -> Shelf {
        self.pointed
    }

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
    pub fn open_pointed(&mut self) -> List {
        self.focus = Focus::Tracks;
        self.changed();
        match self.pointed {
            Shelf::Prepare => List::Prepare,
            Shelf::Crate(id) => List::Crate(id),
        }
    }

    /// The list `steps` after the one shown on the side panel (else after the one pointed at),
    /// to open.
    pub fn next_list(&mut self, steps: i64) -> List {
        let lists = self.lists();
        let at = lists.iter().rposition(|list| match (list, &self.source) {
            (List::Folder(folder), List::Folder(shown)) => within(shown, folder),
            (list, shown) => list == shown,
        });
        let next = lists[(at.unwrap_or(0) as i64 + steps).clamp(0, lists.len() as i64 - 1) as usize].clone();
        match next {
            List::Prepare => self.pointed = Shelf::Prepare,
            List::Crate(id) => self.pointed = Shelf::Crate(id),
            _ => {}
        }
        self.focus = Focus::Tracks;
        self.changed();
        next
    }

    /// The lists of the side panel in its order: the prepare list, the duplicates, each music
    /// folder and the folders right under it that hold tracks, the crates, the sets played.
    fn lists(&self) -> Vec<List> {
        let mut lists = vec![List::Prepare, List::Duplicates];
        for root in &self.roots {
            let depth = root.components().count();
            let mut under: Vec<PathBuf> = self
                .catalogue
                .ids()
                .map(|id| Path::new(&self.catalogue.get(id).path))
                .filter(|path| path.components().count() > depth + 1 && within(path, root))
                .filter_map(|path| path.components().nth(depth).map(|name| root.join(name)))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            under.sort_by_cached_key(|folder| folded(&folder.to_string_lossy()));
            lists.push(List::Folder(root.clone()));
            lists.extend(under.into_iter().map(List::Folder));
        }
        lists.extend(self.crates.iter().map(|c| List::Crate(c.id)));
        lists.extend(self.sessions.iter().map(|s| List::History(s.id)));
        lists
    }

    /// `id` among the tracks from elsewhere, or not, as its path says: whether that changed.
    pub(super) fn place(&mut self, id: TrackId) -> bool {
        let track = self.catalogue.get(id);
        let outside =
            track.record.is_some() && !self.roots.iter().any(|root| within(Path::new(&track.path), root));
        if outside == self.elsewhere.contains(id) {
            return false;
        }
        if outside {
            self.elsewhere.ids.insert(id);
            let at = self.elsewhere.by_path.partition_point(|&e| self.catalogue.get(e).path < track.path);
            self.elsewhere.by_path.insert(at, id);
        } else {
            self.elsewhere.ids.remove(&id);
            self.elsewhere.by_path.retain(|&e| e != id);
        }
        true
    }

    pub(super) fn find_elsewhere(&mut self) {
        let roots = &self.roots;
        let mut elsewhere: Vec<TrackId> = self
            .catalogue
            .ids()
            .filter(|&id| !roots.iter().any(|root| within(Path::new(&self.catalogue.get(id).path), root)))
            .collect();
        elsewhere.sort_by(|&a, &b| self.catalogue.get(a).path.cmp(&self.catalogue.get(b).path));
        self.elsewhere = Elsewhere { ids: elsewhere.iter().copied().collect(), by_path: elsewhere };
    }

    /// The lists the track `id` belongs to by what it is.
    pub(super) fn belongs(&self, id: TrackId) -> Belongs {
        let record = self.catalogue.get(id).record.as_ref();
        let rules = self.crates.iter().filter_map(|c| c.rules.as_ref());
        Belongs {
            smart: rules.map(|rules| record.is_some_and(|r| rules.matches(r))).collect(),
            song: record.filter(|r| r.error.is_none()).and_then(song_key),
        }
    }

    pub(super) fn count_smart_crates(&mut self) {
        let records: Vec<&TrackRecord> =
            self.catalogue.ids().filter_map(|id| self.catalogue.get(id).record.as_ref()).collect();
        for c in &mut self.crates {
            if let Some(rules) = &c.rules {
                c.track_count = records.iter().filter(|r| rules.matches(r)).count();
            }
        }
    }
}
