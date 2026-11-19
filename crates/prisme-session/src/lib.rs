//! Persistent state stored as JSON files.
//!
//! Writes are atomic (temporary file, then rename), so a crash never leaves a
//! half-written file. A file that cannot be parsed is set aside and replaced by the
//! defaults rather than blocking the start; one that another program keeps from being read
//! (an antivirus, a backup) is left alone, the session going on without saving over it.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::error;

/// Reads of a file another program holds: a scan by an antivirus takes a moment.
const READ_TRIES: usize = 5;
const READ_RETRY: Duration = Duration::from_millis(100);

/// A value read from disk.
#[derive(Debug)]
pub struct Loaded<T> {
    pub value: T,
    /// Where a file that could not be parsed was moved, if there was one.
    pub set_aside: Option<PathBuf>,
    /// The file is there but could not be read: it must not be saved over.
    pub held: bool,
}

/// Reads `path`; a missing file gives the default value, and so does one that cannot be
/// read (`held`).
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Loaded<T> {
    let defaults = |held| Loaded { value: T::default(), set_aside: None, held };
    let bytes = match read(path) {
        Ok(bytes) => bytes,
        Err(e) => return defaults(e.kind() != std::io::ErrorKind::NotFound),
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Loaded { value, set_aside: None, held: false },
        Err(_) => match set_aside(path) {
            Some(backup) => Loaded { set_aside: Some(backup), ..defaults(false) },
            // not saved over, as it could not be kept
            None => defaults(true),
        },
    }
}

/// Moves `path` out of the way, beside it, without replacing a file set aside before.
fn set_aside(path: &Path) -> Option<PathBuf> {
    let backup = (1..100)
        .map(|n| {
            path.with_extension(if n == 1 {
                "unreadable.json".into()
            } else {
                format!("unreadable {n}.json")
            })
        })
        .find(|backup| !backup.exists())?;
    std::fs::rename(path, &backup).ok().map(|()| backup)
}

/// The bytes of `path`, tried again a few times while another program holds it (content
/// that is not text is for the parser to refuse: the file is set aside, not held).
fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut tries = 1;
    loop {
        match std::fs::read(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound && tries < READ_TRIES => {
                tries += 1;
                std::thread::sleep(READ_RETRY);
            }
            read => return read,
        }
    }
}

/// Writes `value` to `path` atomically.
pub fn save<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(value)?;
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path)
}

/// A value kept in memory and saved to its file after every change, by a thread of its own:
/// a change never waits for the disk, nor does a read wait for a save, and the changes made
/// while a save runs go to the file together in the next.
pub struct JsonFile<T> {
    shared: Arc<Shared<T>>,
    writer: Option<JoinHandle<()>>,
}

struct Shared<T> {
    value: Mutex<T>,
    saves: Mutex<Saves>,
    changed: Condvar,
}

#[derive(Default)]
struct Saves {
    /// Changes made so far, and those in the file.
    made: u64,
    saved: u64,
    closing: bool,
}

impl<T: Serialize + DeserializeOwned + Default + Clone + Send + 'static> JsonFile<T> {
    /// The value of `path`, saved there after every change unless the file could not be read
    /// (`Loaded::held`); how it was read.
    pub fn open(path: PathBuf) -> (Self, Loaded<()>) {
        let loaded = load(&path);
        let mut file = Self::in_memory(loaded.value);
        if !loaded.held {
            let shared = file.shared.clone();
            file.writer = Some(
                std::thread::Builder::new()
                    .name("prisme-save".into())
                    .spawn(move || shared.write(&path))
                    .expect("cannot start a saving thread"),
            );
        }
        (file, Loaded { value: (), set_aside: loaded.set_aside, held: loaded.held })
    }

    /// Kept in memory only (when the data folder is not available).
    pub fn in_memory(value: T) -> Self {
        let shared = Shared { value: Mutex::new(value), saves: Mutex::default(), changed: Condvar::new() };
        Self { shared: Arc::new(shared), writer: None }
    }

    pub fn get(&self) -> T {
        lock(&self.shared.value).clone()
    }

    /// Reads the value without copying it.
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&lock(&self.shared.value))
    }

    /// Changes the value, saved soon after.
    pub fn update<R>(&self, change: impl FnOnce(&mut T) -> R) -> R {
        let result = change(&mut lock(&self.shared.value));
        lock(&self.shared.saves).made += 1;
        self.shared.changed.notify_all();
        result
    }

    /// Waits until every change made so far is in the file (or failed to be saved).
    pub fn flush(&self) {
        if self.writer.is_none() {
            return;
        }
        let mut saves = lock(&self.shared.saves);
        let made = saves.made;
        while saves.saved < made {
            saves = self.shared.changed.wait(saves).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl<T: Serialize + Clone> Shared<T> {
    /// Saves the value to `path` whenever it changed, until the file is closed.
    fn write(&self, path: &Path) {
        let mut saves = lock(&self.saves);
        loop {
            while saves.saved == saves.made && !saves.closing {
                saves = self.changed.wait(saves).unwrap_or_else(|e| e.into_inner());
            }
            if saves.saved == saves.made {
                return;
            }
            let made = saves.made;
            drop(saves);
            let value = lock(&self.value).clone();
            if let Err(e) = save(path, &value) {
                error!("{} not saved: {e}", path.display());
            }
            saves = lock(&self.saves);
            saves.saved = made;
            self.changed.notify_all();
        }
    }
}

impl<T> Drop for JsonFile<T> {
    /// Saves what is left to save.
    fn drop(&mut self) {
        lock(&self.shared.saves).closing = true;
        self.shared.changed.notify_all();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Prefs {
        volume: f32,
        folder: Option<String>,
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("prisme-session-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn saved_values_are_read_back() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("prefs.json");
        let prefs = Prefs { volume: 0.5, folder: Some("House".into()) };
        save(&path, &prefs).unwrap();
        assert_eq!(load::<Prefs>(&path).value, prefs);
        assert!(!path.with_extension("tmp").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let loaded = load::<Prefs>(Path::new("does/not/exist.json"));
        assert_eq!(loaded.value, Prefs::default());
        assert!(loaded.set_aside.is_none());
    }

    #[test]
    fn an_unreadable_file_is_set_aside() {
        let dir = temp_dir("corrupt");
        let path = dir.join("prefs.json");
        std::fs::write(&path, "{ not json").unwrap();
        let loaded = load::<Prefs>(&path);
        assert_eq!(loaded.value, Prefs::default());
        let backup = loaded.set_aside.expect("set aside");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "{ not json");
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_file_that_cannot_be_read_is_never_saved_over() {
        let dir = temp_dir("held");
        // a folder where the file should be: there, but not readable as a file
        let path = dir.join("prefs.json");
        std::fs::create_dir_all(&path).unwrap();
        let (file, opened) = JsonFile::<Prefs>::open(path.clone());
        assert!(opened.held && opened.set_aside.is_none());
        file.update(|p| p.volume = 0.3);
        file.flush();
        assert!(path.is_dir());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_file_set_aside_before_is_kept() {
        let dir = temp_dir("twice");
        let path = dir.join("prefs.json");
        for content in ["{ first", "{ second"] {
            std::fs::write(&path, content).unwrap();
            assert!(load::<Prefs>(&path).set_aside.is_some());
        }
        assert_eq!(std::fs::read_to_string(dir.join("prefs.unreadable.json")).unwrap(), "{ first");
        assert_eq!(std::fs::read_to_string(dir.join("prefs.unreadable 2.json")).unwrap(), "{ second");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_file_that_is_not_utf_8_is_set_aside() {
        let dir = temp_dir("ansi");
        let path = dir.join("prefs.json");
        // saved as Windows-1252 by an editor: "é" is the byte 0xE9
        std::fs::write(&path, b"{ \"folder\": \"Musique \xe9t\xe9\" }").unwrap();
        let loaded = load::<Prefs>(&path);
        assert!(!loaded.held && loaded.set_aside.is_some());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn fields_added_later_take_their_default() {
        let dir = temp_dir("upgrade");
        let path = dir.join("prefs.json");
        std::fs::write(&path, r#"{ "volume": 0.8 }"#).unwrap();
        assert_eq!(load::<Prefs>(&path).value, Prefs { volume: 0.8, folder: None });
        std::fs::remove_dir_all(dir).ok();
    }

    /// A value whose saving takes a while, as on a slow disk; how many times it was saved.
    #[derive(Clone, Default, Deserialize)]
    struct Slow(f32);

    const SLOW_SAVE: Duration = Duration::from_millis(300);
    static SLOW_SAVES: AtomicUsize = AtomicUsize::new(0);

    impl Serialize for Slow {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            SLOW_SAVES.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(SLOW_SAVE);
            self.0.serialize(serializer)
        }
    }

    #[test]
    fn changes_wait_for_no_save_and_are_saved_together() {
        let dir = temp_dir("slow");
        let path = dir.join("slow.json");
        let (file, _) = JsonFile::<Slow>::open(path.clone());
        let started = Instant::now();
        for step in 0..50 {
            file.update(|v| v.0 = step as f32);
            assert_eq!(file.read(|v| v.0), step as f32);
        }
        assert!(started.elapsed() < SLOW_SAVE, "{:?}", started.elapsed());
        file.flush();
        assert!(SLOW_SAVES.load(Ordering::SeqCst) <= 2);
        assert_eq!(load::<f32>(&path).value, 49.0);
        drop(file);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn what_is_left_to_save_is_saved_when_the_file_closes() {
        let dir = temp_dir("close");
        let path = dir.join("prefs.json");
        let (file, _) = JsonFile::<Prefs>::open(path.clone());
        file.update(|p| p.volume = 0.7);
        drop(file);
        assert_eq!(load::<Prefs>(&path).value.volume, 0.7);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn updates_are_saved() {
        let dir = temp_dir("update");
        let path = dir.join("prefs.json");
        let (file, _) = JsonFile::<Prefs>::open(path.clone());
        file.update(|p| p.volume = 0.3);
        file.flush();
        assert_eq!(load::<Prefs>(&path).value.volume, 0.3);
        std::fs::remove_dir_all(dir).ok();
    }
}
