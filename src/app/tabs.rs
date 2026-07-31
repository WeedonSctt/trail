//! Multi-tab support: `TabState` and tab management.
//!
//! Each tab holds an independent `{cwd, entries, selected, history}`.
//! `TabState` is embedded in `AppState.tabs`; Phase 8 enables creation,
//! switching, and closing of additional tabs alongside the initial one.

use std::path::PathBuf;

use crate::app::history::NavigationHistory;
use crate::app::state::Entry;

/// The complete navigation state for a single tab.
///
/// Every tab holds its own independent directory listing, selection index,
/// and history stack so switching between tabs restores each tab's exact
/// previous state.
#[derive(Debug)]
pub struct TabState {
    /// The directory this tab is currently displaying.
    pub cwd: PathBuf,
    /// The sorted directory entries for `cwd`.
    pub entries: Vec<Entry>,
    /// Index into `entries` of the currently highlighted item.
    pub selected: usize,
    /// Back/forward navigation history for this tab.
    pub history: NavigationHistory,
}

impl TabState {
    /// Creates a `TabState` for `cwd` with an empty entry list.
    ///
    /// The caller is responsible for populating `entries` by calling
    /// `AppState::load_dir` or equivalent after construction.
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            entries: Vec::new(),
            selected: 0,
            history: NavigationHistory::new(),
        }
    }
}

/// Manages the collection of open tabs within `AppState`.
///
/// There is always at least one tab. `active` is the index of the
/// currently focused tab in `tabs`. Invariant: `active < tabs.len()`.
#[derive(Debug)]
pub struct TabManager {
    /// All open tabs; never empty.
    pub tabs: Vec<TabState>,
    /// Index of the active (focused) tab. Always `< tabs.len()`.
    pub active: usize,
}

impl TabManager {
    /// Creates a `TabManager` with a single initial tab rooted at `cwd`.
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            tabs: vec![TabState::new(cwd)],
            active: 0,
        }
    }

    /// Returns a shared reference to the active tab.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn active_tab(&self) -> &TabState {
        // SAFETY-invariant: `active` is always in bounds. Maintained by all
        // mutating methods below; use `&self.tabs[self.active]` rather than
        // `.get().unwrap()` to make the invariant explicit.
        &self.tabs[self.active]
    }

    /// Returns a mutable reference to the active tab.
    pub fn active_tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active]
    }

    /// Returns the number of open tabs.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Returns `true` if there are no tabs open.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Returns `true` when exactly one tab is open.
    ///
    /// Used by the UI to decide whether to render the tab bar.
    pub fn is_single(&self) -> bool {
        self.tabs.len() == 1
    }

    /// Opens a new tab rooted at `cwd` and switches focus to it.
    ///
    /// The new tab is inserted after the currently active tab.
    pub fn open_tab(&mut self, cwd: PathBuf) {
        let insert_at = self.active + 1;
        self.tabs.insert(insert_at, TabState::new(cwd));
        self.active = insert_at;
    }

    /// Closes the active tab.
    ///
    /// If this would leave zero tabs, the close is refused (returns `false`).
    /// On success the active index is adjusted so it remains in bounds:
    /// - If there are tabs to the right, the next one becomes active.
    /// - Otherwise the previous tab becomes active.
    ///
    /// Returns `true` if the tab was closed, `false` if it was the last one.
    pub fn close_active_tab(&mut self) -> bool {
        if self.tabs.len() == 1 {
            return false;
        }
        self.tabs.remove(self.active);
        // Clamp to the new last index.
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        true
    }

    /// Switches focus to the tab at `index`.
    ///
    /// Returns `true` if the switch happened (index was valid and different
    /// from the current active tab), `false` otherwise.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    /// Switches focus to the next tab, wrapping around from the last to the first.
    ///
    /// Returns `true` when there is more than one tab (i.e. a switch happened).
    pub fn switch_next(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        self.active = (self.active + 1) % self.tabs.len();
        true
    }

    /// Switches focus to the previous tab, wrapping around from the first to
    /// the last.
    ///
    /// Returns `true` when there is more than one tab.
    pub fn switch_prev(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        self.active = if self.active == 0 {
            self.tabs.len() - 1
        } else {
            self.active - 1
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn new_tab_manager_has_one_tab() {
        let mgr = TabManager::new(p("/start"));
        assert_eq!(mgr.len(), 1);
        assert_eq!(mgr.active, 0);
        assert_eq!(mgr.active_tab().cwd, p("/start"));
    }

    #[test]
    fn open_tab_inserts_after_active() {
        let mut mgr = TabManager::new(p("/a"));
        mgr.open_tab(p("/b"));
        assert_eq!(mgr.len(), 2);
        assert_eq!(mgr.active, 1);
        assert_eq!(mgr.active_tab().cwd, p("/b"));
    }

    #[test]
    fn close_tab_not_allowed_when_last() {
        let mut mgr = TabManager::new(p("/a"));
        let closed = mgr.close_active_tab();
        assert!(!closed);
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn close_active_tab_switches_to_remaining() {
        let mut mgr = TabManager::new(p("/a"));
        mgr.open_tab(p("/b"));
        // Active is now 1 (/b). Close it; active should become 0 (/a).
        let closed = mgr.close_active_tab();
        assert!(closed);
        assert_eq!(mgr.len(), 1);
        assert_eq!(mgr.active, 0);
        assert_eq!(mgr.active_tab().cwd, p("/a"));
    }

    #[test]
    fn switch_next_wraps() {
        let mut mgr = TabManager::new(p("/a"));
        mgr.open_tab(p("/b"));
        let _ = mgr.switch_to(0);
        mgr.active = 0;
        assert!(mgr.switch_next());
        assert_eq!(mgr.active, 1);
        assert!(mgr.switch_next()); // wraps back to 0
        assert_eq!(mgr.active, 0);
    }

    #[test]
    fn switch_prev_wraps() {
        let mut mgr = TabManager::new(p("/a"));
        mgr.open_tab(p("/b"));
        mgr.active = 0;
        assert!(mgr.switch_prev()); // wraps to last (1)
        assert_eq!(mgr.active, 1);
    }

    #[test]
    fn switch_next_single_tab_returns_false() {
        let mut mgr = TabManager::new(p("/a"));
        assert!(!mgr.switch_next());
    }

    #[test]
    fn switch_to_out_of_bounds_returns_false() {
        let mut mgr = TabManager::new(p("/a"));
        assert!(!mgr.switch_to(99));
    }

    #[test]
    fn switch_to_same_tab_returns_false() {
        let mut mgr = TabManager::new(p("/a"));
        mgr.open_tab(p("/b"));
        mgr.active = 1;
        assert!(!mgr.switch_to(1));
    }

    #[test]
    fn is_single_reflects_tab_count() {
        let mut mgr = TabManager::new(p("/a"));
        assert!(mgr.is_single());
        mgr.open_tab(p("/b"));
        assert!(!mgr.is_single());
    }
}
