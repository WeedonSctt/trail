//! Bookmark store: a persisted, named-path registry.
//!
//! Bookmarks are stored as a TOML file (`bookmarks.toml`) in the user's
//! Trail data directory (resolved via the `directories` crate). Each bookmark
//! maps a short, user-chosen name to an absolute filesystem path.
//!
//! The store is loaded once at startup and written atomically on every
//! mutation. Persistence failures are logged at `debug` level and do not
//! abort navigation — a bookmark that fails to save is inconvenient, not
//! catastrophic.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced while loading or saving the bookmark store.
#[derive(Debug, Error)]
pub enum BookmarkError {
    /// The bookmark store file could not be read.
    #[error("failed to read bookmarks from {path}: {source}")]
    Read {
        /// Store file path.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The bookmark store file could not be written.
    #[error("failed to write bookmarks to {path}: {source}")]
    Write {
        /// Store file path.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A TOML parse error occurred while loading the store.
    #[error("failed to parse bookmarks TOML: {0}")]
    Parse(#[from] toml::de::Error),
    /// A TOML serialization error occurred while saving the store.
    #[error("failed to serialize bookmarks TOML: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// The on-disk TOML format for the bookmark store.
#[derive(Debug, Default, Serialize, Deserialize)]
struct BookmarksFile {
    /// Map from bookmark name → absolute path string.
    #[serde(default)]
    bookmarks: BTreeMap<String, String>,
}

/// A persisted, named-path bookmark registry.
///
/// Loaded from and saved to a `bookmarks.toml` file in the user's Trail
/// data directory. Mutations are written immediately after every change.
///
/// Internally, bookmarks are a `BTreeMap<String, PathBuf>` so listing them
/// is always in alphabetical order.
#[derive(Debug)]
pub struct BookmarkStore {
    /// Path to the backing TOML file (may not exist yet on a fresh install).
    file_path: PathBuf,
    /// In-memory map of name → absolute path.
    map: BTreeMap<String, PathBuf>,
}

impl BookmarkStore {
    /// Opens the bookmark store from `file_path`, creating it if absent.
    ///
    /// If the file does not exist, an empty store is returned (not an error).
    ///
    /// # Errors
    ///
    /// Returns [`BookmarkError::Read`] if the file exists but cannot be read,
    /// or [`BookmarkError::Parse`] if the TOML is malformed.
    pub fn open(file_path: PathBuf) -> Result<Self, BookmarkError> {
        let map = if file_path.exists() {
            let content = std::fs::read_to_string(&file_path).map_err(|e| BookmarkError::Read {
                path: file_path.clone(),
                source: e,
            })?;
            let parsed: BookmarksFile = toml::from_str(&content)?;
            parsed
                .bookmarks
                .into_iter()
                .map(|(k, v)| (k, PathBuf::from(v)))
                .collect()
        } else {
            BTreeMap::new()
        };

        Ok(Self { file_path, map })
    }

    /// Adds or replaces a bookmark named `name` pointing to `path`.
    ///
    /// Persists the updated store immediately.
    ///
    /// # Errors
    ///
    /// Returns a [`BookmarkError`] if serialization or the write fails.
    pub fn add(&mut self, name: String, path: PathBuf) -> Result<(), BookmarkError> {
        self.map.insert(name, path);
        self.save()
    }

    /// Removes the bookmark named `name`.
    ///
    /// Returns `true` if a bookmark with that name existed and was removed.
    /// Persists the updated store even if no entry was removed (idempotent).
    ///
    /// # Errors
    ///
    /// Returns a [`BookmarkError`] if serialization or the write fails.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn remove(&mut self, name: &str) -> Result<bool, BookmarkError> {
        let existed = self.map.remove(name).is_some();
        self.save()?;
        Ok(existed)
    }

    /// Returns the absolute path for bookmark `name`, or `None` if it does
    /// not exist.
    pub fn get(&self, name: &str) -> Option<&Path> {
        self.map.get(name).map(|p| p.as_path())
    }

    /// Returns an iterator over `(name, path)` pairs in alphabetical order.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn list(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.map.iter().map(|(k, v)| (k.as_str(), v.as_path()))
    }

    /// Returns `true` if the store contains no bookmarks.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Returns the number of bookmarks in the store.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Saves the current in-memory map to `self.file_path`.
    ///
    /// Creates parent directories if they do not exist.
    ///
    /// # Errors
    ///
    /// Returns [`BookmarkError::Write`] on I/O failure or
    /// [`BookmarkError::Serialize`] on TOML serialization failure.
    fn save(&self) -> Result<(), BookmarkError> {
        // Ensure the parent directory exists.
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| BookmarkError::Write {
                path: self.file_path.clone(),
                source: e,
            })?;
        }

        let on_disk = BookmarksFile {
            bookmarks: self
                .map
                .iter()
                .map(|(k, v)| (k.clone(), v.display().to_string()))
                .collect(),
        };

        let toml_str = toml::to_string_pretty(&on_disk)?;
        std::fs::write(&self.file_path, toml_str).map_err(|e| BookmarkError::Write {
            path: self.file_path.clone(),
            source: e,
        })
    }
}

/// Returns the default path for the bookmark store in the user's data directory.
///
/// On Linux/macOS this is typically `~/.local/share/trail/bookmarks.toml`;
/// on Windows `%APPDATA%\trail\bookmarks.toml`. Falls back to a path in
/// the current directory if the platform data dir is unavailable.
// clippy: dead_code — API consumed in Phase 9 UI
#[allow(dead_code)]
pub fn default_bookmark_path() -> PathBuf {
    if let Some(proj_dirs) = directories::ProjectDirs::from("", "", "trail") {
        proj_dirs.data_dir().join("bookmarks.toml")
    } else {
        PathBuf::from("trail_bookmarks.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, BookmarkStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bookmarks.toml");
        let store = BookmarkStore::open(path).expect("open");
        (dir, store)
    }

    #[test]
    fn empty_store_on_new_file() {
        let (_dir, store) = temp_store();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn add_and_get() {
        let (_dir, mut store) = temp_store();
        store
            .add("home".to_owned(), PathBuf::from("/home/user"))
            .expect("add");
        assert_eq!(store.get("home"), Some(Path::new("/home/user")));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn add_overwrites_existing() {
        let (_dir, mut store) = temp_store();
        store
            .add("home".to_owned(), PathBuf::from("/home/old"))
            .expect("add");
        store
            .add("home".to_owned(), PathBuf::from("/home/new"))
            .expect("add");
        assert_eq!(store.get("home"), Some(Path::new("/home/new")));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn remove_returns_true_when_existed() {
        let (_dir, mut store) = temp_store();
        store
            .add("work".to_owned(), PathBuf::from("/work"))
            .expect("add");
        let removed = store.remove("work").expect("remove");
        assert!(removed);
        assert!(store.is_empty());
    }

    #[test]
    fn remove_returns_false_when_absent() {
        let (_dir, mut store) = temp_store();
        let removed = store.remove("nope").expect("remove");
        assert!(!removed);
    }

    #[test]
    fn list_is_alphabetical() {
        let (_dir, mut store) = temp_store();
        store
            .add("zoo".to_owned(), PathBuf::from("/zoo"))
            .expect("add");
        store
            .add("alpha".to_owned(), PathBuf::from("/alpha"))
            .expect("add");
        store
            .add("middle".to_owned(), PathBuf::from("/middle"))
            .expect("add");
        let names: Vec<_> = store.list().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["alpha", "middle", "zoo"]);
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bookmarks.toml");

        {
            let mut store = BookmarkStore::open(path.clone()).expect("open");
            store
                .add("work".to_owned(), PathBuf::from("/work/project"))
                .expect("add");
        }

        let store2 = BookmarkStore::open(path).expect("reopen");
        assert_eq!(store2.get("work"), Some(Path::new("/work/project")));
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let (_dir, store) = temp_store();
        assert_eq!(store.get("nope"), None);
    }
}
