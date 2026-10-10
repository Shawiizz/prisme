//! Persistent state stored as JSON files.
//!
//! Writes are atomic (temporary file, then rename), so a crash never leaves a
//! half-written file. A file that cannot be read is set aside and replaced by the
//! defaults rather than blocking the start.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// A value read from disk.
#[derive(Debug)]
pub struct Loaded<T> {
    pub value: T,
    /// Where an unreadable file was moved, if there was one.
    pub set_aside: Option<PathBuf>,
}

/// Reads `path`; a missing file gives the default value.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Loaded<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return Loaded { value: T::default(), set_aside: None },
    };
    match serde_json::from_str(&text) {
        Ok(value) => Loaded { value, set_aside: None },
        Err(_) => {
            let backup = path.with_extension("unreadable.json");
            let set_aside = std::fs::rename(path, &backup).ok().map(|()| backup);
            Loaded { value: T::default(), set_aside }
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

/// A value kept in memory and saved to its file after every change.
pub struct JsonFile<T> {
    path: Option<PathBuf>,
    value: Mutex<T>,
}

impl<T: Serialize + DeserializeOwned + Default + Clone> JsonFile<T> {
    pub fn open(path: PathBuf) -> (Self, Option<PathBuf>) {
        let loaded = load(&path);
        (Self { path: Some(path), value: Mutex::new(loaded.value) }, loaded.set_aside)
    }

    /// Kept in memory only (when the data folder is not available).
    pub fn in_memory(value: T) -> Self {
        Self { path: None, value: Mutex::new(value) }
    }

    pub fn get(&self) -> T {
        self.lock().clone()
    }

    /// Changes the value and saves it. The change is kept even if saving fails.
    pub fn update<R>(&self, change: impl FnOnce(&mut T) -> R) -> (R, std::io::Result<()>) {
        let mut value = self.lock();
        let result = change(&mut value);
        let saved = self.path.as_deref().map_or(Ok(()), |path| save(path, &*value));
        (result, saved)
    }

    fn lock(&self) -> MutexGuard<'_, T> {
        self.value.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

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
    fn fields_added_later_take_their_default() {
        let dir = temp_dir("upgrade");
        let path = dir.join("prefs.json");
        std::fs::write(&path, r#"{ "volume": 0.8 }"#).unwrap();
        assert_eq!(load::<Prefs>(&path).value, Prefs { volume: 0.8, folder: None });
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn updates_are_saved() {
        let dir = temp_dir("update");
        let path = dir.join("prefs.json");
        let (file, _) = JsonFile::<Prefs>::open(path.clone());
        let (_, saved) = file.update(|p| p.volume = 0.3);
        saved.unwrap();
        assert_eq!(load::<Prefs>(&path).value.volume, 0.3);
        std::fs::remove_dir_all(dir).ok();
    }
}
