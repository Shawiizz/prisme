//! Knowing a file again where the library never saw it: renamed, moved, on a drive that
//! came back under another letter. What was known of it at its old path (analysis, cues,
//! corrections, marks, crates, history) follows it.

use crate::db::{path_key, DbError, FileStamp, LibraryDb};
use std::path::{Path, PathBuf};

/// How far [`adopt`] looks for the file a new one may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Search {
    /// By its name, size and time: a query, for a whole folder being listed.
    Stamp,
    /// And by its audio, read from the file: for a track being loaded.
    Audio,
}

/// Gives the file at `path`, which the library does not know, what it knew of the same file
/// at a path where it is no longer; returns that path. The same file is:
///
/// - one of the same size and time under the same path on another drive (a USB stick that
///   came back under another letter);
/// - one of the same name, size and time that is gone from a drive still there (moved);
/// - with [`Search::Audio`], one of the same audio gone from a drive still there (renamed
///   and tagged again).
///
/// A file on a drive that is away is otherwise left what it has: it may come back.
pub fn adopt(
    db: &LibraryDb,
    path: &Path,
    stamp: FileStamp,
    search: Search,
) -> Result<Option<PathBuf>, DbError> {
    adopt_unless(db, path, stamp, search, |_| false)
}

/// [`adopt`], leaving alone the paths that `may_be_back` (under a folder that could not be
/// listed).
pub fn adopt_unless(
    db: &LibraryDb,
    path: &Path,
    stamp: FileStamp,
    search: Search,
    may_be_back: impl Fn(&Path) -> bool,
) -> Result<Option<PathBuf>, DbError> {
    if db.has_track(path)? {
        return Ok(None);
    }
    let own = path_key(path);
    let elsewhere = |old: &String| {
        let file = Path::new(old);
        *old != own && !file.exists() && !may_be_back(file)
    };
    let stamped: Vec<String> = db.paths_stamped(stamp)?.into_iter().filter(elsewhere).collect();
    let moved = |old: &&String| {
        let old = Path::new(old.as_str());
        if drive_is_there(old) {
            old.file_name() == path.file_name()
        } else {
            same_but_for_the_drive(old, path)
        }
    };
    let mut from = single(stamped.iter().filter(moved).cloned());
    if from.is_none() && search == Search::Audio {
        if let Ok(id) = prisme_media::audio_id(path) {
            let sounding = db.paths_sounding_like(id)?;
            from =
                single(sounding.into_iter().filter(|old| elsewhere(old) && drive_is_there(Path::new(old))));
        }
    }
    let Some(from) = from.map(PathBuf::from) else { return Ok(None) };
    db.relocate(&from, path)?;
    Ok(Some(from))
}

/// The only item of `items`: two candidates cannot be told apart.
fn single<T>(mut items: impl Iterator<Item = T>) -> Option<T> {
    items.next().filter(|_| items.next().is_none())
}

/// The folders removable drives and shares are mounted in, on Linux and macOS, with how
/// many folders down the drive itself is (`/media/<user>/<drive>`).
#[cfg(not(windows))]
const MOUNTS: [(&str, usize); 4] = [("/Volumes", 1), ("/media", 2), ("/run/media", 2), ("/mnt", 1)];

/// The drive `path` is on, and the path under it: a drive letter or a share on Windows, the
/// folder a drive is mounted in elsewhere (the root for the others).
fn drive_of(path: &Path) -> (PathBuf, &Path) {
    #[cfg(windows)]
    let drive = path.ancestors().last().unwrap_or(path).to_path_buf();
    #[cfg(not(windows))]
    let drive = MOUNTS
        .iter()
        .find_map(|(mounts, depth)| {
            let under = path.strip_prefix(mounts).ok()?;
            (under.components().count() > *depth).then(|| {
                under.components().take(*depth).fold(PathBuf::from(mounts), |drive, c| drive.join(c))
            })
        })
        .unwrap_or_else(|| PathBuf::from("/"));
    let under = path.strip_prefix(&drive).unwrap_or(path);
    (drive, under)
}

/// Whether the drive (or the share) of `path` is there.
pub(crate) fn drive_is_there(path: &Path) -> bool {
    let drive = drive_of(path).0;
    drive.has_root() && drive.exists()
}

/// Whether two paths are the same under their drives (whatever the case, where the system
/// does not tell it).
fn same_but_for_the_drive(a: &Path, b: &Path) -> bool {
    let under = |path: &Path| {
        let under = drive_of(path).1.to_string_lossy().into_owned();
        if cfg!(windows) {
            under.to_lowercase()
        } else {
            under
        }
    };
    under(a) == under(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TrackRecord;
    use crate::{Cue, CueSlot};
    use prisme_analysis::TrackAnalysis;
    use prisme_media::TrackTags;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("prisme-identity-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A track stored at `path` with a cue, its file gone since.
    fn known(db: &LibraryDb, path: &Path, stamp: FileStamp) {
        db.save(&TrackRecord::analyzed(path, stamp, 1.0, TrackAnalysis::default(), TrackTags::default()))
            .unwrap();
        db.save_cue(path, &Cue::point(CueSlot::Hot(0), 12.0)).unwrap();
    }

    fn file(path: &Path) -> FileStamp {
        std::fs::write(path, b"not read").unwrap();
        FileStamp::of(path).unwrap()
    }

    #[test]
    fn a_file_moved_is_known_by_its_name_size_and_time() {
        let dir = folder("moved");
        let db = LibraryDb::open_in_memory().unwrap();
        let (old, new) = (dir.join("Old/track.mp3"), dir.join("New/track.mp3"));
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        let stamp = file(&new);
        known(&db, &old, stamp);
        assert_eq!(adopt(&db, &new, stamp, Search::Stamp).unwrap(), Some(old.clone()));
        assert_eq!(db.cues(&new).unwrap().len(), 1);
        assert!(db.get(&old).unwrap().is_none());
        // known now: nothing more to take
        assert_eq!(adopt(&db, &new, stamp, Search::Stamp).unwrap(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_copy_leaves_the_file_still_there_what_it_has() {
        let dir = folder("copy");
        let db = LibraryDb::open_in_memory().unwrap();
        let (original, copy) = (dir.join("track.mp3"), dir.join("copy of track.mp3"));
        let stamp = file(&original);
        file(&copy);
        known(&db, &original, stamp);
        assert_eq!(adopt(&db, &copy, stamp, Search::Audio).unwrap(), None);
        assert_eq!(db.cues(&original).unwrap().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn another_name_with_the_same_size_and_time_is_another_file() {
        let dir = folder("renamed");
        let db = LibraryDb::open_in_memory().unwrap();
        let new = dir.join("other.mp3");
        let stamp = file(&new);
        known(&db, &dir.join("track.mp3"), stamp);
        assert_eq!(adopt(&db, &new, stamp, Search::Stamp).unwrap(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn two_files_it_could_be_are_left_alone() {
        let dir = folder("two");
        let db = LibraryDb::open_in_memory().unwrap();
        let new = dir.join("track.mp3");
        let stamp = file(&new);
        known(&db, &dir.join("A/track.mp3"), stamp);
        known(&db, &dir.join("B/track.mp3"), stamp);
        assert_eq!(adopt(&db, &new, stamp, Search::Stamp).unwrap(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    /// The root of a drive that is not there.
    #[cfg(windows)]
    fn drive_away() -> PathBuf {
        ('D'..='Z')
            .map(|letter| PathBuf::from(format!(r"{letter}:\")))
            .find(|root| !root.exists())
            .expect("a drive letter not in use")
    }

    #[cfg(not(windows))]
    fn drive_away() -> PathBuf {
        PathBuf::from(format!("/mnt/prisme-no-such-drive-{}", std::process::id()))
    }

    #[test]
    #[cfg(windows)]
    fn the_same_path_on_another_drive_is_the_same_file() {
        assert!(same_but_for_the_drive(Path::new(r"E:\Sets\Track.mp3"), Path::new(r"F:\sets\track.mp3")));
        assert!(!same_but_for_the_drive(Path::new(r"E:\Sets\track.mp3"), Path::new(r"F:\Other\track.mp3")));
    }

    #[test]
    #[cfg(not(windows))]
    fn the_same_path_on_another_drive_is_the_same_file() {
        let same = |a: &str, b: &str| same_but_for_the_drive(Path::new(a), Path::new(b));
        assert!(same("/Volumes/STICK/Sets/track.mp3", "/Volumes/STICK 1/Sets/track.mp3"));
        assert!(same("/media/me/STICK/Sets/track.mp3", "/run/media/me/STICK1/Sets/track.mp3"));
        assert!(!same("/Volumes/STICK/Sets/track.mp3", "/Volumes/STICK/Other/track.mp3"));
        assert!(!same("/Volumes/STICK/Sets/track.mp3", "/home/me/Sets/Track.mp3"));
        assert_eq!(drive_of(Path::new("/home/me/Music/track.mp3")).0, Path::new("/"));
        assert_eq!(drive_of(Path::new("/media/me/STICK/track.mp3")).0, Path::new("/media/me/STICK"));
        // the folder of the drives is no drive itself
        assert_eq!(drive_of(Path::new("/media/me")).0, Path::new("/"));
    }

    #[test]
    fn a_drive_is_there_or_away() {
        assert!(!drive_is_there(&drive_away().join("track.mp3")));
        assert!(drive_is_there(&std::env::temp_dir().join("gone.mp3")));
    }

    #[test]
    fn a_track_on_a_drive_that_is_away_follows_its_path_on_another_letter() {
        let dir = folder("letter");
        let db = LibraryDb::open_in_memory().unwrap();
        let new = dir.join("track.mp3");
        let stamp = file(&new);
        let away = drive_away().join(drive_of(&new).1);
        known(&db, &away, stamp);
        // elsewhere on the drive that is away: it may come back, and keeps what it has
        let other = drive_away().join("Elsewhere").join("track.mp3");
        known(&db, &other, FileStamp { size: stamp.size + 1, ..stamp });
        assert_eq!(adopt(&db, &new, stamp, Search::Stamp).unwrap(), Some(away));
        assert_eq!(db.cues(&new).unwrap().len(), 1);
        assert_eq!(db.cues(&other).unwrap().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_copy_of_a_track_on_a_drive_that_is_away_takes_nothing() {
        let dir = folder("away");
        let db = LibraryDb::open_in_memory().unwrap();
        let copy = dir.join("track.mp3");
        let stamp = file(&copy);
        let away = drive_away().join("Sets").join("track.mp3");
        known(&db, &away, stamp);
        assert_eq!(adopt(&db, &copy, stamp, Search::Audio).unwrap(), None);
        assert_eq!(db.cues(&away).unwrap().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }
}
