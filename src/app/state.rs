//! Core application state: `AppState`, `Entry`, `EntryKind`.
//!
//! The central data model that every other module reads or mutates.
//! `state.dirty` is the invariant that drives the render loop: every
//! mutation sets it, every render clears it.

use std::fs;
use std::path::{Path, PathBuf};

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32Str};
use thiserror::Error;

use crate::app::history::NavigationHistory;
use crate::app::mode::Mode;
use crate::input::command_parser::{CommandHistory, TabState};
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
    /// A filesystem mutation (rename/move/delete/etc.) failed.
    #[error("filesystem error: {0}")]
    FsOp(#[from] crate::actions::fs_ops::FsError),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitFileStatus {
    /// File is untracked.
    #[allow(dead_code)]
    Untracked,
    /// File has been modified.
    #[allow(dead_code)]
    Modified,
    /// File has been added to the index.
    #[allow(dead_code)]
    Added,
    /// File has been deleted.
    #[allow(dead_code)]
    Deleted,
    /// File has been renamed.
    #[allow(dead_code)]
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
    /// Git status, populated asynchronously by the git worker (Phase 4).
    /// `None` before the worker reports back, or outside a git repo.
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
/// `matches` holds indices into `AppState::entries` ordered by descending
/// fuzzy-match score. `scores` is a parallel `Vec` keeping the score for
/// each corresponding match (used for sorting; not rendered directly).
#[derive(Debug, Clone, Default)]
pub struct FilterState {
    /// The current query string.
    pub query: String,
    /// Indices into `AppState::entries`, ordered by descending match score.
    pub matches: Vec<usize>,
    /// Match scores parallel to `matches`. `scores[i]` is the score for
    /// `matches[i]`.
    pub scores: Vec<u32>,
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
    /// Git repository state for the current directory.
    /// `None` before the git worker reports back or outside a git repo.
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

    // ── Phase 3 fields ────────────────────────────────────────────────────────
    /// `true` while awaiting the user's confirmation of a `dd` delete.
    ///
    /// The status bar renders a confirmation prompt while this is set.
    /// Confirmed via `Enter`; cancelled via `Esc`.
    pub pending_delete: bool,
    /// Last error message surfaced by a command or fs operation, displayed
    /// in the status bar. Cleared on the next successful action.
    pub error_message: Option<String>,
    /// The most-recently yanked path string (set by clipboard operations).
    /// Displayed briefly in the status bar and observable in tests.
    pub last_yank: Option<String>,
    /// Persisted command history for Command Mode.
    pub command_history: CommandHistory,
    /// Tab-completion cycling state for Command Mode. Stored here so it
    /// survives across individual key dispatches within the same mode session.
    pub tab_state: TabState,

    // ── Phase 3 multi-key sequence state ─────────────────────────────────────
    /// Tracks the first key of multi-key Navigation Mode sequences:
    /// `y` (for `ya`/`yr`/`yn`) and `d` (for `dd`).
    pub pending_nav_key: Option<char>,
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
            pending_delete: false,
            error_message: None,
            last_yank: None,
            command_history: CommandHistory::new(),
            tab_state: TabState::new(),
            pending_nav_key: None,
        };

        state.load_dir(&cwd)?;
        Ok(state)
    }

    /// Recomputes the fuzzy-filter against the current `entries` using
    /// `query`, storing results sorted by descending score in
    /// `self.filter`. Also auto-selects the top match (index 0).
    ///
    /// Call this every time the query changes in Search Mode. The filter
    /// runs synchronously on the UI thread — the architecture doc
    /// designates fuzzy filtering as "fast enough not to need offloading."
    ///
    /// An empty query matches all entries (no score order; original listing
    /// order is preserved).
    pub fn apply_filter(&mut self, query: String) {
        let mut filter = FilterState {
            query: query.clone(),
            matches: Vec::new(),
            scores: Vec::new(),
        };

        if query.is_empty() {
            // Empty query: show all visible entries in their original order.
            filter.matches = (0..self.entries.len())
                .filter(|&i| self.show_hidden || !self.entries[i].is_hidden)
                .collect();
            filter.scores = vec![0u32; filter.matches.len()];
        } else {
            let pattern = Pattern::parse(&query, CaseMatching::Ignore, Normalization::Smart);
            let mut matcher = Matcher::new(Config::DEFAULT);

            // Score each visible entry and collect those that match (score > 0).
            let mut scored: Vec<(usize, u32)> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| self.show_hidden || !e.is_hidden)
                .filter_map(|(idx, entry)| {
                    let mut buf = Vec::new();
                    let haystack = Utf32Str::new(&entry.file_name, &mut buf);
                    pattern.score(haystack, &mut matcher).map(|s| (idx, s))
                })
                .collect();

            // Sort by score descending so the best match is first.
            scored.sort_by_key(|b| std::cmp::Reverse(b.1));

            filter.matches = scored.iter().map(|(idx, _)| *idx).collect();
            filter.scores = scored.iter().map(|(_, score)| *score).collect();
        }

        // Auto-select the top match.
        self.selected = 0;
        self.filter = Some(filter);

        // Keep Mode::Search.query in sync with the FilterState.query.
        if let Mode::Search {
            query: q,
            matches: m,
        } = &mut self.mode
        {
            *q = query;
            *m = self
                .filter
                .as_ref()
                .map(|f| f.matches.clone())
                .unwrap_or_default();
        }

        self.dirty = true;
    }

    /// Returns the number of filtered-visible entries, or `visible_count()`
    /// when no filter is active.
    pub fn filtered_count(&self) -> usize {
        match &self.filter {
            Some(f) => f.matches.len(),
            None => self.visible_count(),
        }
    }

    /// Returns an iterator over the entries visible given the current filter
    /// and `show_hidden` flag.
    ///
    /// When a filter is active, entries are yielded in match-score order.
    /// When no filter is active, entries are yielded in listing order
    /// (excluding hidden entries unless `show_hidden` is `true`).
    pub fn filtered_entries(&self) -> impl Iterator<Item = (usize, &Entry)> {
        // Return a concrete `Vec` iterator so both branches have the same type.
        let pairs: Vec<(usize, &Entry)> = match &self.filter {
            Some(f) => f
                .matches
                .iter()
                .filter_map(|&idx| self.entries.get(idx).map(|e| (idx, e)))
                .collect(),
            None => self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| self.show_hidden || !e.is_hidden)
                .collect(),
        };
        pairs.into_iter()
    }

    /// Returns the currently selected `Entry` under the active filter,
    /// or `None` if the visible (filtered) list is empty.
    pub fn selected_filtered_entry(&self) -> Option<&Entry> {
        self.filtered_entries().nth(self.selected).map(|(_, e)| e)
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
    ///
    /// In Search Mode, delegates to `selected_filtered_entry` so the
    /// selection is resolved against the match list rather than the raw
    /// listing. Outside Search Mode the raw visible listing is used.
    pub fn selected_entry(&self) -> Option<&Entry> {
        if self.filter.is_some() {
            self.selected_filtered_entry()
        } else {
            self.visible_entries().nth(self.selected)
        }
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

    /// Moves the selection down by one within the visible (or filtered) list,
    /// clamping at the last entry.
    ///
    /// Sets `dirty = true` if the selection changed.
    pub fn move_down(&mut self) {
        let count = self.filtered_count();
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

    /// Jumps the selection to the last visible (or filtered) entry.
    ///
    /// Sets `dirty = true` if the selection changed.
    pub fn jump_bottom(&mut self) {
        let count = self.filtered_count();
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
    /// When a filter is active, re-applies it so hidden-file visibility is
    /// reflected correctly in the match list.
    ///
    /// # Errors
    ///
    /// Propagates any `StateError` from `load_dir`.
    pub fn toggle_hidden(&mut self) -> Result<(), StateError> {
        self.show_hidden = !self.show_hidden;
        // Re-apply the filter if one is active so the match set is correct.
        if let Some(f) = self.filter.take() {
            let q = f.query.clone();
            self.apply_filter(q);
        } else {
            // Clamp selection to the new visible range.
            let count = self.visible_count();
            if count == 0 {
                self.selected = 0;
            } else {
                self.selected = self.selected.min(count - 1);
            }
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
