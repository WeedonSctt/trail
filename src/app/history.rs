//! Navigation history: back/forward stack for directory traversal.
//!
//! Tracks visited directories so `u` / `Ctrl-r` can navigate history.
//! Each `push` saves the current directory onto the back stack and clears
//! the forward stack (standard browser-style history model).

use std::path::PathBuf;

/// Maximum number of entries kept in either direction of history.
const MAX_HISTORY: usize = 256;

/// Back/forward navigation history for directory traversal.
///
/// Invariant: `back` and `forward` together hold at most `MAX_HISTORY`
/// entries each. The current directory is *not* stored here — it lives in
/// `AppState::cwd`. `back[last]` is the directory to return to on `back()`.
#[derive(Debug, Clone, Default)]
pub struct NavigationHistory {
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
}

impl NavigationHistory {
    /// Creates an empty `NavigationHistory`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `current_cwd` in the back stack when navigating to a new
    /// directory. Clears the forward stack (new branch voids the future).
    ///
    /// Trims the back stack to `MAX_HISTORY` by dropping the oldest entry if
    /// it would exceed the limit.
    pub fn push(&mut self, current_cwd: PathBuf) {
        self.forward.clear();
        if self.back.len() == MAX_HISTORY {
            self.back.remove(0);
        }
        self.back.push(current_cwd);
    }

    /// Returns the previous directory (back one step), storing `current_cwd`
    /// in the forward stack so `forward()` can undo this.
    ///
    /// Returns `None` if the back stack is empty (already at the oldest point).
    pub fn back(&mut self, current_cwd: PathBuf) -> Option<PathBuf> {
        let prev = self.back.pop()?;
        if self.forward.len() == MAX_HISTORY {
            self.forward.remove(0);
        }
        self.forward.push(current_cwd);
        Some(prev)
    }

    /// Returns the next directory (forward one step after a `back()`),
    /// storing `current_cwd` on the back stack.
    ///
    /// Returns `None` if the forward stack is empty (no forward history).
    pub fn forward(&mut self, current_cwd: PathBuf) -> Option<PathBuf> {
        let next = self.forward.pop()?;
        if self.back.len() == MAX_HISTORY {
            self.back.remove(0);
        }
        self.back.push(current_cwd);
        Some(next)
    }

    /// Returns `true` if there is at least one entry to go back to.
    #[allow(dead_code)] // TODO(phase-something): Used for UI indicator
    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    /// Returns `true` if there is at least one entry to go forward to.
    #[allow(dead_code)] // TODO(phase-something): Used for UI indicator
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn push_and_back() {
        let mut h = NavigationHistory::new();
        h.push(p("/a"));
        let prev = h.back(p("/b"));
        assert_eq!(prev, Some(p("/a")));
        assert!(!h.can_go_back());
        assert!(h.can_go_forward());
    }

    #[test]
    fn back_then_forward() {
        let mut h = NavigationHistory::new();
        h.push(p("/a"));
        h.push(p("/b"));
        let prev = h.back(p("/c"));
        assert_eq!(prev, Some(p("/b")));
        let next = h.forward(p("/b"));
        assert_eq!(next, Some(p("/c")));
    }

    #[test]
    fn push_clears_forward() {
        let mut h = NavigationHistory::new();
        h.push(p("/a"));
        h.back(p("/b"));
        // Going back from /b gives /a; then push a new dir should clear forward.
        h.push(p("/c"));
        assert!(!h.can_go_forward());
    }

    #[test]
    fn back_on_empty_returns_none() {
        let mut h = NavigationHistory::new();
        assert_eq!(h.back(p("/x")), None);
    }

    #[test]
    fn forward_on_empty_returns_none() {
        let mut h = NavigationHistory::new();
        assert_eq!(h.forward(p("/x")), None);
    }
}
