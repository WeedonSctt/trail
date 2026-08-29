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
use crate::app::tabs::TabManager;
use crate::config::TrailConfig;
use crate::input::command_parser::{CommandHistory, TabState};
use crate::plugin::bookmarks::{BookmarkError, BookmarkStore};
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
    /// Built-in configuration failed to load.
    #[error("config error: {0}")]
    Config(#[from] crate::config::ConfigError),
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
    /// Whether this entry is a text file, as determined by a content-inspection
    /// probe at listing time. `None` for non-regular-file entries (directories,
    /// symlinks). Populated once in `from_dir_entry` so that
    /// `TextProvider::can_handle` can return without performing any I/O.
    pub is_text: Option<bool>,
}

impl Entry {
    /// Constructs an `Entry` from a `DirEntry`.
    ///
    /// Returns `None` if the file name cannot be represented as UTF-8.
    ///
    /// For regular files the first 8 KB are read once here (via
    /// `content_inspector`) to populate `is_text`. This is the only place
    /// that probe runs; `TextProvider::can_handle` reads `is_text` directly
    /// without any further I/O.
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

        // Probe text vs. binary once at listing time; only meaningful for
        // regular files. Directories and symlinks leave is_text as None.
        let is_text = if kind == EntryKind::File {
            Some(crate::preview::text::is_text_file(&path))
        } else {
            None
        };

        Some(Entry {
            path,
            file_name,
            kind,
            is_hidden,
            metadata,
            git_status: None,
            is_text,
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
    /// Runtime configuration loaded from defaults plus any user TOML file.
    pub config: TrailConfig,
    /// Current working directory being displayed.
    pub cwd: PathBuf,
    /// The directory Trail was launched from — the shell's working directory
    /// at startup, which is unaffected by navigation.
    ///
    /// This is the base for `yr` (copy relative path): the point of a relative
    /// path is that it can be pasted back into the shell that started Trail,
    /// and that shell has not moved. Relative to `cwd` instead, every yank
    /// would be the selection's own file name, which is what `yn` yanks.
    ///
    /// Canonicalized to match the form of `cwd` and of entry paths, so the two
    /// can be related component-by-component (on Windows both therefore carry
    /// the same `\\?\` verbatim prefix).
    pub launch_dir: PathBuf,
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

    // ── Phase 3 multi-key sequence state ───────────────────────────────────────
    /// Tracks the first key of multi-key Navigation Mode sequences:
    /// `y` (for `ya`/`yr`/`yn`/`yc`) and `d` (for `dd`).
    pub pending_nav_key: Option<char>,

    // ── Phase 6 shell integration ──────────────────────────────────────────────
    /// A pending `RunExternal` action to be executed by the event loop.
    ///
    /// Set by `apply()` when an editor-open or `!<shell command>` action is
    /// requested. The event loop in `main.rs` drains this field after each
    /// key dispatch, calling `shell_exec::run_external` and forcing a full
    /// redraw. Using a state field rather than a channel keeps `apply()`
    /// synchronous and testable without a running terminal.
    pub pending_external: Option<crate::actions::Action>,

    // ── Phase 8 tab management ─────────────────────────────────────────────────
    /// Multi-tab state manager.
    ///
    /// The active tab's `cwd`/`entries`/`selected`/`history` are kept in sync
    /// with the flat `AppState` fields so all existing code continues to work
    /// unchanged. On a tab switch the active state is written to the old tab
    /// slot and read from the new one.
    pub tab_manager: TabManager,

    // -- Phase 8 bookmark store --------------------------------------------------
    /// Persisted named-path bookmark registry.
    ///
    /// None before startup initialization. Bookmark operations silently
    /// degrade when None (e.g. data directory unavailable).
    pub bookmark_store: Option<BookmarkStore>,
    /// Phase 8 plugin engine for firing hooks on navigation.
    pub plugin_engine: Option<crate::plugin::PluginEngine>,
    /// Recent directories tracker.
    pub recent_dirs: crate::session::RecentDirs,
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
    // clippy: dead_code — used heavily by integration tests in `tests/`
    #[allow(dead_code)]
    pub fn new(start_path: PathBuf) -> Result<Self, StateError> {
        let config = crate::config::load(None)?;
        Self::with_config(start_path, config)
    }

    /// Creates a new `AppState` rooted at `start_path` using `config`.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::ReadDir`] if the initial directory listing fails.
    pub fn with_config(start_path: PathBuf, config: TrailConfig) -> Result<Self, StateError> {
        let cwd = start_path.canonicalize().unwrap_or(start_path);

        // The process working directory at startup is the shell's, since
        // nothing has navigated yet — Trail changes `cwd` here, never the
        // process's own directory. If it cannot be read (the shell was started
        // in a directory that has since been deleted), fall back to the start
        // path, which at worst makes `yr` behave as it did before.
        let launch_dir = std::env::current_dir()
            .map(|dir| dir.canonicalize().unwrap_or(dir))
            .unwrap_or_else(|e| {
                tracing::debug!("could not read the launch directory: {e}");
                cwd.clone()
            });

        let mut state = AppState {
            config,
            cwd: cwd.clone(),
            launch_dir,
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
            pending_external: None,
            tab_manager: TabManager::new(cwd.clone()),
            bookmark_store: None,
            plugin_engine: None,
            recent_dirs: crate::session::RecentDirs::default(),
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
        self.cwd = path.clone();

        let res = self.load_dir(&self.cwd.clone());

        self.recent_dirs.visit(path.clone());
        if let Some(engine) = &self.plugin_engine {
            engine.fire_on_enter_dir(&path);
        }

        res
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
    /// Unlike `enter_dir`, this preserves the current `selected` index (clamped
    /// to the new listing length) so that `R` does not jump the cursor to the
    /// top of the list.
    ///
    /// # Errors
    ///
    /// Propagates any `StateError` from `load_dir`.
    pub fn refresh(&mut self) -> Result<(), StateError> {
        let saved = self.selected;
        self.load_dir(&self.cwd.clone())?;
        // Restore the selection the user had, clamped to the new count.
        let count = self.visible_count();
        self.selected = if count == 0 { 0 } else { saved.min(count - 1) };
        self.dirty = true;
        Ok(())
    }

    /// Updates the cached `StatusBarState` from current state.
    ///
    /// Called automatically by `load_dir`.
    fn update_status(&mut self) {
        self.status.cwd_display = self.cwd.display().to_string();
        self.status.entry_count = self.visible_count();
    }

    // -- Tab management -------------------------------------------------------

    /// Saves the current flat state into the active tab slot before switching.
    fn save_to_active_tab(&mut self) {
        let tab = self.tab_manager.active_tab_mut();
        tab.cwd = self.cwd.clone();
        tab.entries = self.entries.clone();
        tab.selected = self.selected;
        std::mem::swap(&mut tab.history, &mut self.history);
    }

    /// Restores the flat state from the active tab slot after switching.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::ReadDir`] if the restored `cwd` can no longer be listed.
    pub fn restore_from_active_tab(&mut self) -> Result<(), StateError> {
        let tab = self.tab_manager.active_tab_mut();
        let new_cwd = tab.cwd.clone();
        let new_selected = tab.selected;
        std::mem::swap(&mut tab.history, &mut self.history);
        self.cwd = new_cwd.clone();
        self.selected = new_selected;
        self.git = None;
        self.filter = None;
        self.preview = PreviewSlot::default();
        self.mode = Mode::Navigation;
        self.load_dir(&new_cwd)?;
        Ok(())
    }

    /// Opens a new tab rooted at `cwd` (or the current directory when `None`).
    ///
    /// Saves the current tab's state before switching.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::ReadDir`] if the new tab's directory cannot be listed.
    pub fn open_tab(&mut self, cwd: Option<PathBuf>) -> Result<(), StateError> {
        let new_cwd = cwd.unwrap_or_else(|| self.cwd.clone());
        self.save_to_active_tab();
        self.tab_manager.open_tab(new_cwd.clone());
        self.cwd = new_cwd.clone();
        self.selected = 0;
        self.git = None;
        self.filter = None;
        self.preview = PreviewSlot::default();
        self.mode = Mode::Navigation;
        self.history = NavigationHistory::new();
        self.load_dir(&new_cwd)?;
        Ok(())
    }

    /// Closes the active tab and restores the adjacent tab's state.
    ///
    /// Returns `false` when only one tab is open (close refused).
    ///
    /// # Errors
    ///
    /// Returns [`StateError::ReadDir`] if the revealed tab's directory cannot be listed.
    pub fn close_tab(&mut self) -> Result<bool, StateError> {
        if !self.tab_manager.close_active_tab() {
            return Ok(false);
        }
        let tab = self.tab_manager.active_tab_mut();
        let new_cwd = tab.cwd.clone();
        let new_selected = tab.selected;
        std::mem::swap(&mut tab.history, &mut self.history);
        self.cwd = new_cwd.clone();
        self.selected = new_selected;
        self.git = None;
        self.filter = None;
        self.preview = PreviewSlot::default();
        self.mode = Mode::Navigation;
        self.load_dir(&new_cwd)?;
        Ok(true)
    }

    /// Switches to the next tab (wraps), restoring its state.
    ///
    /// No-op when only one tab is open.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::ReadDir`] if the target tab's directory cannot be listed.
    pub fn switch_tab_next(&mut self) -> Result<(), StateError> {
        if self.tab_manager.is_single() {
            return Ok(());
        }
        self.save_to_active_tab();
        self.tab_manager.switch_next();
        self.restore_from_active_tab()
    }

    /// Switches to the previous tab (wraps), restoring its state.
    ///
    /// No-op when only one tab is open.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::ReadDir`] if the target tab's directory cannot be listed.
    pub fn switch_tab_prev(&mut self) -> Result<(), StateError> {
        if self.tab_manager.is_single() {
            return Ok(());
        }
        self.save_to_active_tab();
        self.tab_manager.switch_prev();
        self.restore_from_active_tab()
    }

    // -- Bookmark management --------------------------------------------------

    /// Adds a bookmark named `name` pointing to the current directory.
    ///
    /// No-op (logs at debug) when the bookmark store is not initialized.
    ///
    /// # Errors
    ///
    /// Returns [`BookmarkError`] if the store cannot be persisted.
    pub fn bookmark_add(&mut self, name: String) -> Result<(), BookmarkError> {
        let cwd = self.cwd.clone();
        if let Some(store) = &mut self.bookmark_store {
            store.add(name, cwd)?;
        } else {
            tracing::debug!("bookmark_add: store not initialized");
        }
        Ok(())
    }

    /// Navigates to the bookmark named `name`.
    ///
    /// Returns `Ok(true)` if the bookmark existed and navigation succeeded,
    /// `Ok(false)` if no bookmark with that name exists.
    ///
    /// # Errors
    ///
    /// Returns [`StateError`] if the bookmarked directory cannot be listed.
    pub fn bookmark_jump(&mut self, name: &str) -> Result<bool, StateError> {
        let target = match &self.bookmark_store {
            Some(store) => store.get(name).map(|p| p.to_owned()),
            None => {
                tracing::debug!("bookmark_jump: store not initialized");
                return Ok(false);
            }
        };
        match target {
            Some(path) => {
                self.enter_dir(path)?;
                Ok(true)
            }
            None => Ok(false),
        }
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
