//! Core application state: `AppState`, `Entry`, `EntryKind`.
//!
//! The central data model that every other module reads or mutates.
//! `state.dirty` is the invariant that drives the render loop: every
//! mutation sets it, every render clears it.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::app::history::NavigationHistory;
use crate::app::mode::Mode;
use crate::preview::provider::PreviewContent;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors that can arise when loading or mutating `AppState`.
#[derive(Debug, Error)]
pub enum StateError {
    /// A directory listing failed.
    #[error("failed to read directory {path}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

// ── Entry ─────────────────────────────────────────────────────────────────────

/// The filesystem kind of a directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// A directory (or a junction on Windows).
    Dir,
    /// A regular file.
    File,
    /// A symbolic link (the target kind is not resolved for display purposes).
    Symlink,
}

/// The git status of a single file within a repository.
///
/// Populated asynchronously by the git worker (Phase 4); `None` until then.
#[allow(dead_code)] // TODO(phase-4): Used by git worker
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitFileStatus {
    /// File is untracked.
    Untracked,
    /// File has been modified.
    Modified,
    /// File has been added to the index.
    Added,
    /// File has been deleted.
    Deleted,
    /// File has been renamed.
    Renamed,
    /// File is unmodified / clean.
    Clean,
}

/// A single entry shown in the navigation panel.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Full absolute path to this entry.
    pub path: PathBuf,
    /// The file-name component (no parent path), pre-extracted for display.
    pub file_name: String,
    /// Whether this entry is a directory, file, or symlink.
    pub kind: EntryKind,
    /// Whether the entry name starts with `.` (Unix hidden-file convention).
    pub is_hidden: bool,
    /// Filesystem metadata, if available.
    #[allow(dead_code)] // TODO(phase-5): Used by binary formatter
    pub metadata: Option<fs::Metadata>,
    /// Git status, populated asynchronously in Phase 4. `None` until then.
    #[allow(dead_code)] // TODO(phase-4): Populated by git worker
    pub git_status: Option<GitFileStatus>,
}

impl Entry {
    /// Constructs an `Entry` from a `DirEntry`.
    ///
    /// Returns `None` if the file name cannot be represented as UTF-8.
    fn from_dir_entry(de: &fs::DirEntry) -> Option<Self> {
        let path = de.path();
        let file_name = path.file_name()?.to_str()?.to_owned();
        let is_hidden = file_name.starts_with('.');
        let metadata = de.metadata().ok();

        let kind = if let Some(ref m) = metadata {
            if m.is_symlink() {
                EntryKind::Symlink
            } else if m.is_dir() {
                EntryKind::Dir
            } else {
                EntryKind::File
            }
        } else {
            // If we can't stat, treat as file.
            EntryKind::File
        };

        Some(Entry {
            path,
            file_name,
            kind,
            is_hidden,
            metadata,
            git_status: None,
        })
    }
}

// ── Git directory state ───────────────────────────────────────────────────────

/// Git repository information for the current directory.
///
/// Populated asynchronously by the git worker (Phase 4). `AppState::git`
/// holds `None` until the worker reports back.
#[allow(dead_code)] // TODO(phase-4): Populated by git worker
#[derive(Debug, Clone)]
pub struct GitDirState {
    /// The active branch name, or `"HEAD"` if in detached-HEAD state.
    pub branch: String,
    /// Whether the working tree has any uncommitted changes.
    pub is_dirty: bool,
}

// ── Preview slot ──────────────────────────────────────────────────────────────

/// The preview pane's current content and the generation it belongs to.
///
/// `generation` is incremented on every selection change so that
/// late-arriving worker results for a since-abandoned selection can be
/// discarded in `workers::merge` (Phase 4). The field is defined here in
/// Phase 1 so the state shape is stable — the guard is exercised in Phase 4/5.
#[derive(Debug, Clone, Default)]
pub struct PreviewSlot {
    /// The path whose preview is currently being displayed (or loading).
    pub for_path: PathBuf,
    /// Monotonically increasing counter. Every selection change bumps this.
    pub generation: u64,
    /// The actual content to render, or a loading placeholder.
    pub content: PreviewContent,
}

// ── Filter state ──────────────────────────────────────────────────────────────

/// Active fuzzy-filter state while in Search Mode.
///
/// Defined here in Phase 1 so the state shape is complete; wired in Phase 2.
#[allow(dead_code)] // TODO(phase-2): Wired in Search Mode
#[derive(Debug, Clone, Default)]
pub struct FilterState {
    /// The current query string.
    pub query: String,
    /// Indices into `AppState::entries`, sorted by match score.
    pub matches: Vec<usize>,
}

// ── Status bar state ──────────────────────────────────────────────────────────

/// Derived values cached for the status bar so the render path is pure.
///
/// Updated whenever the underlying state changes rather than recomputed
/// on every frame.
#[derive(Debug, Clone, Default)]
pub struct StatusBarState {
    /// Human-readable string for the current working directory.
    pub cwd_display: String,
    /// Total number of entries (after hidden-file filtering).
    pub entry_count: usize,
}

// ── AppState ──────────────────────────────────────────────────────────────────

/// Central application state owned by the UI thread.
///
/// Every mutation must set `dirty = true` so the render loop knows to
/// redraw. Every render clears `dirty`.
#[derive(Debug)]
pub struct AppState {
    /// Current working directory being displayed.
    pub cwd: PathBuf,
    /// Directory-first sorted listing of the current directory.
    /// Hidden entries are included but may be filtered from display.
    pub entries: Vec<Entry>,
    /// Index into `entries` of the currently highlighted item.
    pub selected: usize,
    /// Current interaction mode.
    pub mode: Mode,
    /// Navigation history for `u`/`Ctrl-r` back/forward.
    pub history: NavigationHistory,
    /// Active fuzzy-filter while in Search Mode; `None` otherwise.
    pub filter: Option<FilterState>,
    /// Current preview pane content + generation counter.
    pub preview: PreviewSlot,
    /// Git repository state for the current directory; `None` until Phase 4.
    #[allow(dead_code)] // TODO(phase-4): Populated by git worker
    pub git: Option<GitDirState>,
    /// Cached status bar strings.
    pub status: StatusBarState,
    /// Whether hidden files are shown. Toggled by a keybinding (Phase 1).
    pub show_hidden: bool,
    /// Set `true` on any state mutation; cleared after each render.
    pub dirty: bool,
    /// The selection index to restore when re-entering a known directory.
    ///
    /// Key: canonical absolute path; Value: last selected index.
    pub selection_memory: std::collections::HashMap<PathBuf, usize>,
}

impl AppState {
    /// Creates a new `AppState` rooted at `start_path`.
    ///
    /// Loads the initial directory listing synchronously. Returns an error
    /// if `start_path` cannot be read.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::ReadDir`] if the initial directory listing fails.
    pub fn new(start_path: PathBuf) -> Result<Self, StateError> {
        let cwd = start_path.canonicalize().unwrap_or(start_path);

        let mut state = AppState {
            cwd: cwd.clone(),
            entries: Vec::new(),
            selected: 0,
            mode: Mode::default(),
            history: NavigationHistory::new(),
            filter: None,
            preview: PreviewSlot::default(),
            git: None,
            status: StatusBarState::default(),
            show_hidden: false,
            dirty: true,
            selection_memory: std::collections::HashMap::new(),
        };

        state.load_dir(&cwd)?;
        Ok(state)
    }

    /// Reads and sorts the directory at `path`, replacing `self.entries`.
    ///
    /// Sort order: directories first, then files/symlinks; within each group,
    /// alphabetical case-insensitive. Hidden entries are always included in
    /// `entries`; visibility is controlled by `show_hidden` at render time.
    ///
    /// Also updates `status` and sets `dirty`.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::ReadDir`] if the directory cannot be listed.
    pub fn load_dir(&mut self, path: &Path) -> Result<(), StateError> {
        let read = fs::read_dir(path).map_err(|e| StateError::ReadDir {
            path: path.to_owned(),
            source: e,
        })?;

        let mut entries: Vec<Entry> = read
            .filter_map(|res| res.ok())
            .filter_map(|de| Entry::from_dir_entry(&de))
            .collect();

        // Directory-first sort, then alphabetical case-insensitive within each
        // group. Decision log: tie-break is alphabetical, case-insensitive.
        entries.sort_by(|a, b| {
            let a_is_dir = a.kind == EntryKind::Dir;
            let b_is_dir = b.kind == EntryKind::Dir;
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()),
            }
        });

        self.entries = entries;

        // Restore the previously remembered selection, clamping to valid range.
        let remembered = self.selection_memory.get(path).copied().unwrap_or(0);
        self.selected = remembered.min(self.visible_count().saturating_sub(1));

        self.update_status();
        self.dirty = true;
        Ok(())
    }

    /// Returns the number of entries that are visible given `show_hidden`.
    pub fn visible_count(&self) -> usize {
        if self.show_hidden {
            self.entries.len()
        } else {
            self.entries.iter().filter(|e| !e.is_hidden).count()
        }
    }

    /// Returns a slice of entries visible given the current `show_hidden` flag.
    ///
    /// Returns an iterator rather than allocating a new `Vec`.
    pub fn visible_entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |e| self.show_hidden || !e.is_hidden)
    }

    /// Returns the currently selected `Entry`, or `None` if the list is empty.
    pub fn selected_entry(&self) -> Option<&Entry> {
        self.visible_entries().nth(self.selected)
    }

    /// Navigates into the directory at `path`, pushing the current `cwd` onto
    /// the history stack and loading the new listing.
    ///
    /// Saves the current selection in `selection_memory` before navigating.
    ///
    /// # Errors
    ///
    /// Propagates any `StateError` from `load_dir`.
    pub fn enter_dir(&mut self, path: PathBuf) -> Result<(), StateError> {
        // Remember where we were in the old directory.
        self.selection_memory
            .insert(self.cwd.clone(), self.selected);
        self.history.push(self.cwd.clone());
        self.cwd = path;
        self.load_dir(&self.cwd.clone())
    }

    /// Navigates to the parent directory, if one exists.
    ///
    /// Saves the current selection before navigating.
    ///
    /// # Errors
    ///
    /// Propagates any `StateError` from `load_dir`.
    pub fn go_parent(&mut self) -> Result<(), StateError> {
        if let Some(parent) = self.cwd.parent().map(|p| p.to_owned()) {
            self.selection_memory
                .insert(self.cwd.clone(), self.selected);
            self.history.push(self.cwd.clone());
            self.cwd = parent;
            self.load_dir(&self.cwd.clone())?;
        }
        Ok(())
    }

    /// Navigates backward in history (bound to `u`).
    ///
    /// # Errors
    ///
    /// Propagates any `StateError` from `load_dir`.
    pub fn history_back(&mut self) -> Result<(), StateError> {
        if let Some(prev) = self.history.back(self.cwd.clone()) {
            self.selection_memory
                .insert(self.cwd.clone(), self.selected);
            self.cwd = prev;
            self.load_dir(&self.cwd.clone())?;
        }
        Ok(())
    }

    /// Navigates forward in history (bound to `Ctrl-r`).
    ///
    /// # Errors
    ///
    /// Propagates any `StateError` from `load_dir`.
    pub fn history_forward(&mut self) -> Result<(), StateError> {
        if let Some(next) = self.history.forward(self.cwd.clone()) {
            self.selection_memory
                .insert(self.cwd.clone(), self.selected);
            self.cwd = next;
            self.load_dir(&self.cwd.clone())?;
        }
        Ok(())
    }

    /// Moves the selection down by one, clamping at the last visible entry.
    ///
    /// Sets `dirty = true` if the selection changed.
    pub fn move_down(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        let new = (self.selected + 1).min(count - 1);
        if new != self.selected {
            self.selected = new;
            self.dirty = true;
        }
    }

    /// Moves the selection up by one, clamping at zero.
    ///
    /// Sets `dirty = true` if the selection changed.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.dirty = true;
        }
    }

    /// Jumps the selection to the first visible entry.
    ///
    /// Sets `dirty = true` if the selection changed.
    pub fn jump_top(&mut self) {
        if self.selected != 0 {
            self.selected = 0;
            self.dirty = true;
        }
    }

    /// Jumps the selection to the last visible entry.
    ///
    /// Sets `dirty = true` if the selection changed.
    pub fn jump_bottom(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        let last = count - 1;
        if self.selected != last {
            self.selected = last;
            self.dirty = true;
        }
    }

    /// Toggles display of hidden files and reloads the directory listing.
    ///
    /// # Errors
    ///
    /// Propagates any `StateError` from `load_dir`.
    pub fn toggle_hidden(&mut self) -> Result<(), StateError> {
        self.show_hidden = !self.show_hidden;
        // Clamp selection to the new visible range.
        let count = self.visible_count();
        if count == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(count - 1);
        }
        self.dirty = true;
        Ok(())
    }

    /// Reloads the current directory listing in place (e.g. after an external
    /// change or a self-initiated filesystem mutation).
    ///
    /// # Errors
    ///
    /// Propagates any `StateError` from `load_dir`.
    pub fn refresh(&mut self) -> Result<(), StateError> {
        self.load_dir(&self.cwd.clone())
    }

    /// Updates the cached `StatusBarState` from current state.
    ///
    /// Called automatically by `load_dir`.
    fn update_status(&mut self) {
        self.status.cwd_display = self.cwd.display().to_string();
        self.status.entry_count = self.visible_count();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Creates a temp directory with a known set of files for testing.
    fn make_test_dir() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        fs::create_dir(p.join("alpha_dir")).unwrap();
        fs::create_dir(p.join("zeta_dir")).unwrap();
        fs::write(p.join("a_file.txt"), b"").unwrap();
        fs::write(p.join(".hidden_file"), b"").unwrap();
        dir
    }

    #[test]
    fn dir_first_sort() {
        let dir = make_test_dir();
        let state = AppState::new(dir.path().to_owned()).unwrap();

        // Directories should come first.
        let entries: Vec<_> = state.visible_entries().collect();
        assert_eq!(entries[0].kind, EntryKind::Dir);
        assert_eq!(entries[1].kind, EntryKind::Dir);
        // File comes after directories.
        assert_eq!(entries[2].kind, EntryKind::File);
    }

    #[test]
    fn dir_sort_is_alphabetical_case_insensitive() {
        let dir = make_test_dir();
        let state = AppState::new(dir.path().to_owned()).unwrap();
        let dirs: Vec<_> = state
            .visible_entries()
            .filter(|e| e.kind == EntryKind::Dir)
            .map(|e| e.file_name.as_str())
            .collect();
        assert_eq!(dirs, vec!["alpha_dir", "zeta_dir"]);
    }

    #[test]
    fn hidden_files_hidden_by_default() {
        let dir = make_test_dir();
        let state = AppState::new(dir.path().to_owned()).unwrap();
        assert!(
            state.visible_entries().all(|e| !e.is_hidden),
            "hidden files should not be visible by default"
        );
    }

    #[test]
    fn toggle_hidden_reveals_hidden_files() {
        let dir = make_test_dir();
        let mut state = AppState::new(dir.path().to_owned()).unwrap();
        state.toggle_hidden().unwrap();
        assert!(
            state.visible_entries().any(|e| e.is_hidden),
            "hidden files should be visible after toggle"
        );
    }

    #[test]
    fn move_down_clamps_at_bottom() {
        let dir = make_test_dir();
        let mut state = AppState::new(dir.path().to_owned()).unwrap();
        let count = state.visible_count();
        for _ in 0..count + 5 {
            state.move_down();
        }
        assert_eq!(state.selected, count - 1);
    }

    #[test]
    fn move_up_clamps_at_top() {
        let dir = make_test_dir();
        let mut state = AppState::new(dir.path().to_owned()).unwrap();
        state.move_up(); // Already at 0.
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn jump_top_bottom() {
        let dir = make_test_dir();
        let mut state = AppState::new(dir.path().to_owned()).unwrap();
        state.jump_bottom();
        let count = state.visible_count();
        assert_eq!(state.selected, count - 1);
        state.jump_top();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn dirty_set_on_mutation() {
        let dir = make_test_dir();
        let mut state = AppState::new(dir.path().to_owned()).unwrap();
        state.dirty = false; // simulate post-render clear
        state.move_down();
        assert!(state.dirty);
    }
}
