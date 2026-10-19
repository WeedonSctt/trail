//! The mark set: entries picked with `Space` (or a visual-mode range) for a
//! later paste, move, copy or delete.
//!
//! Marks are **paths**, not indices — an index names whichever entry occupies a
//! row now, and the whole point of a mark is that it survives navigation, a
//! re-sort and a refresh. There is one set for the whole session, shared by
//! every tab, so entries can be marked in one tab and pasted from another. It
//! is not persisted: marks end when Trail does.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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
