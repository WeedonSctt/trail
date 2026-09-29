//! Navigation-panel scrolling: which row of the listing is at the top of the
//! pane, and the scroll margin that keeps the selection off the edges.
//!
//! The offset has to outlive a frame. ratatui's `List` computes its window from
//! the offset it is handed, and a `ListState` built fresh per frame always
//! hands it `0` — from which the cheapest window containing selection *N* ends
//! at *N*, so every selection past the first screenful was drawn on the bottom
//! row and the list scrolled under a selection that never moved. Only the
//! renderer knows the pane height, so the renderer calls [`NavScroll::update`]
//! once per frame and the panel draws with what it returns.

use std::path::{Path, PathBuf};

/// The scroll position of the navigation panel, for the directory it was
/// computed in.
///
/// Keyed by directory rather than reset by every code path that changes one:
/// a position carried from one listing into another means nothing, and
/// comparing the directory at render time catches navigation, tab switches and
/// tab closes without any of them having to remember to clear it. A refresh of
/// the same directory keeps it, which is what stops a filesystem-watch reload
/// from jumping the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavScroll {
    /// The directory `offset` belongs to.
    dir: PathBuf,
    /// Index of the entry drawn on the pane's first row.
    offset: usize,
}

impl NavScroll {
    /// Returns the offset to draw with this frame, and records it in `slot`.
    ///
    /// `selected` indexes the list being drawn, `len` is its length and
    /// `height` the number of rows the pane shows. `margin` is the number of
    /// rows kept between the selection and either edge — vim's `scrolloff` —
    /// and is capped at what the pane can honour, so an oversized margin keeps
    /// the selection centred rather than making the list jitter.
    ///
    /// The margin collapses at the true ends of the list: the first entry is
    /// drawn on the first row and the last entry on the last row. Within the
    /// margin the previous offset is kept, so the selection floats rather than
    /// being pinned to either edge.
    ///
    /// When `slot` holds no offset for `dir` — a directory just entered, or a
    /// tab just switched to — the selection is centred, which puts a
    /// remembered selection in context instead of on an edge.
    pub fn update(
        slot: &mut Option<NavScroll>,
        dir: &Path,
        selected: usize,
        len: usize,
        height: usize,
        margin: usize,
    ) -> usize {
        let previous = slot
            .as_ref()
            .filter(|s| s.dir == dir)
            .map(|s| s.offset)
            .unwrap_or_else(|| selected.saturating_sub(height / 2));
        let offset = clamp(previous, selected, len, height, margin);
        match slot {
            Some(s) if s.dir == dir => s.offset = offset,
            _ => {
                *slot = Some(NavScroll {
                    dir: dir.to_owned(),
                    offset,
                })
            }
        }
        offset
    }
}

/// Moves `offset` the least distance that keeps `selected` at least `margin`
/// rows inside a `height`-row window over a `len`-entry list.
fn clamp(offset: usize, selected: usize, len: usize, height: usize, margin: usize) -> usize {
    if height == 0 || len <= height {
        return 0;
    }
    // A margin of half the pane or more cannot be kept on both sides at once;
    // capping it here is what makes the lower bound below never exceed the
    // upper one.
    let margin = margin.min(height.saturating_sub(1) / 2);
    let selected = selected.min(len - 1);
    let lowest = (selected + margin + 1).saturating_sub(height);
    let highest = selected.saturating_sub(margin);
    // The last clamp is the collapse at the bottom: the window never scrolls
    // past the final entry, so near the end the selection walks down into the
    // margin instead.
    offset.clamp(lowest, highest).min(len - height)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Row within the pane at which `selected` is drawn.
    fn row(offset: usize, selected: usize) -> usize {
        selected - offset
    }

    #[test]
    fn a_list_that_fits_never_scrolls() {
        assert_eq!(clamp(5, 9, 10, 10, 3), 0);
        assert_eq!(clamp(0, 0, 0, 10, 3), 0);
    }

    #[test]
    fn moving_down_stops_margin_rows_short_of_the_bottom() {
        // 100 entries, 20-row pane, margin 3: walking down from the top, the
        // selection stops at row 16 and the list scrolls under it.
        let mut offset = 0;
        for selected in 0..60 {
            offset = clamp(offset, selected, 100, 20, 3);
            assert!(row(offset, selected) <= 16, "selection {selected} too low");
        }
        assert_eq!(row(offset, 59), 16);
    }

    #[test]
    fn moving_up_from_the_bottom_leaves_the_selection_where_it_is() {
        // The reported bug: at the bottom, moving up re-pinned the selection to
        // the last row. With a persisted offset the view holds still and the
        // selection climbs until it reaches the top margin.
        let mut offset = clamp(0, 59, 100, 20, 3);
        let start = row(offset, 59);
        for selected in (50..59).rev() {
            offset = clamp(offset, selected, 100, 20, 3);
            assert_eq!(
                offset,
                clamp(0, 59, 100, 20, 3),
                "view scrolled at {selected}"
            );
        }
        assert_eq!(row(offset, 50), start - 9);
    }

    #[test]
    fn moving_up_stops_margin_rows_short_of_the_top() {
        let mut offset = 80;
        for selected in (10..99).rev() {
            offset = clamp(offset, selected, 100, 20, 3);
            assert!(row(offset, selected) >= 3, "selection {selected} too high");
        }
    }

    #[test]
    fn the_margin_collapses_at_the_ends_of_the_list() {
        assert_eq!(row(clamp(40, 0, 100, 20, 3), 0), 0, "first entry on row 0");
        assert_eq!(
            row(clamp(0, 99, 100, 20, 3), 99),
            19,
            "last entry on the last row"
        );
    }

    #[test]
    fn a_zero_margin_lets_the_selection_reach_either_edge() {
        assert_eq!(row(clamp(0, 19, 100, 20, 0), 19), 19);
        assert_eq!(row(clamp(19, 19, 100, 20, 0), 19), 0);
    }

    #[test]
    fn an_oversized_margin_centres_instead_of_jittering() {
        // Margin 50 in a 21-row pane is capped at 10: the selection sits on the
        // middle row whichever way it moves.
        let mut offset = 0;
        for selected in 10..80 {
            offset = clamp(offset, selected, 100, 21, 50);
            assert_eq!(row(offset, selected), 10);
        }
    }

    #[test]
    fn a_new_directory_centres_its_selection() {
        let mut slot = None;
        let offset = NavScroll::update(&mut slot, Path::new("/a"), 50, 100, 20, 3);
        assert_eq!(row(offset, 50), 10);
    }

    #[test]
    fn the_same_directory_keeps_its_offset_and_another_does_not() {
        let mut slot = None;
        let first = NavScroll::update(&mut slot, Path::new("/a"), 50, 100, 20, 3);
        // One step down stays within the margin, so the view holds still.
        assert_eq!(
            NavScroll::update(&mut slot, Path::new("/a"), 51, 100, 20, 3),
            first
        );
        // The same index in a different directory is re-centred, not carried.
        let other = NavScroll::update(&mut slot, Path::new("/b"), 5, 100, 20, 3);
        assert_eq!(other, 0);
    }
}
