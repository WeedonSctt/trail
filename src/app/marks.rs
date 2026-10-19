//! The mark set: entries picked with `Space` (or a visual-mode range) for a
//! later paste, move, copy or delete.
//!
//! Marks are **paths**, not indices — an index names whichever entry occupies a
//! row now, and the whole point of a mark is that it survives navigation, a
//! re-sort and a refresh. There is one set for the whole session, shared by
//! every tab, so entries can be marked in one tab and pasted from another. It
//! is not persisted: marks end when Trail does.
//!
//! Visual mode is another way of filling the set: its range is worked out here
//! from the anchor and the selection.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::app::mode::Mode;
use crate::app::state::AppState;

/// The marked entries, in path order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MarkSet {
    paths: BTreeSet<PathBuf>,
}

impl MarkSet {
    /// Marks `path` if it was not marked, unmarks it if it was. Returns whether
    /// it is marked afterwards.
    pub fn toggle(&mut self, path: &Path) -> bool {
        if self.paths.remove(path) {
            false
        } else {
            self.paths.insert(path.to_owned());
            true
        }
    }

    /// Marks every path in `paths`, leaving those already marked as they are.
    pub fn extend(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.paths.extend(paths);
    }

    /// Whether `path` is marked.
    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    /// How many entries are marked.
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// Whether nothing is marked.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Unmarks everything. Returns how many were marked.
    pub fn clear(&mut self) -> usize {
        let count = self.paths.len();
        self.paths.clear();
        count
    }

    /// Unmarks `path`.
    pub fn remove(&mut self, path: &Path) {
        self.paths.remove(path);
    }

    /// The marked paths, in path order.
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.paths.iter()
    }
}

/// The rows, first and last inclusive, that visual mode's range covers in the
/// list the user is looking at — or `None` outside visual mode.
///
/// Counted the way `AppState::selected` is, so the range follows the sort, the
/// hidden-file setting and any filter. An anchor that has left the listing (a
/// refresh removed it) collapses the range onto the selection.
pub fn visual_bounds(state: &AppState) -> Option<(usize, usize)> {
    let Mode::Visual { anchor } = &state.mode else {
        return None;
    };
    let anchor_row = state
        .filtered_entries()
        .position(|(_, e)| &e.path == anchor)
        .unwrap_or(state.selected);
    Some((
        anchor_row.min(state.selected),
        anchor_row.max(state.selected),
    ))
}

/// The paths visual mode's range covers, top to bottom; empty outside it.
pub fn visual_paths(state: &AppState) -> Vec<PathBuf> {
    match visual_bounds(state) {
        Some((first, last)) => state
            .filtered_entries()
            .skip(first)
            .take(last - first + 1)
            .map(|(_, e)| e.path.clone())
            .collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_twice_unmarks() {
        let mut marks = MarkSet::default();
        let path = Path::new("a.txt");
        assert!(marks.toggle(path));
        assert!(marks.contains(path));
        assert!(!marks.toggle(path));
        assert!(marks.is_empty());
    }
}
