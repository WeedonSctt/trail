//! Navigation panel rendering.
//!
//! Renders the directory listing with the highlighted selection. When a fuzzy
//! filter is active (Search Mode), only the matching entries are shown, ordered
//! by descending match score. Otherwise the listing is in the active tab's sort
//! order (see [`crate::app::sort`]); hidden entries are dimmed when visible.
//! Git badges are rendered once the git worker populates `entry.git_status`.
//!
//! Each row can also carry a right-flushed details column — size, modification
//! time, or both — selected by `[navigation] entry_details`. The panel is 40%
//! of the screen, so the column is budgeted rather than assumed: see
//! `EntryDetails::fit`.

use std::path::Path;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::state::{AppState, Entry, EntryKind, GitFileStatus};
use crate::metafmt;
use crate::pathfmt;
use crate::preview::provider;
use crate::ui::theme;

/// The narrowest a name may be squeezed to before the details column gives up
/// its room.
///
/// Below this a listing stops being usable: the column would be showing sizes
/// for files you can no longer tell apart. The panel is 40% of the terminal, so
/// on an 80-column screen a row has 28 columns to divide, and the full
/// `size + modified` column wants 26 of them — which is exactly the case this
/// guards against.
const MIN_NAME_WIDTH: usize = 12;

/// Explanation attached to a rejected `[navigation] entry_details` value.
pub const ENTRY_DETAILS_REASON: &str = "must be none, size, modified or both";

/// What the listing shows beside each entry's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EntryDetails {
    /// Just the name. The default, and Trail's only listing before v1.8.0.
    #[default]
    None,
    /// The size in bytes, human-formatted.
    Size,
    /// The modification time.
    Modified,
    /// Size then modification time.
    Both,
}

impl EntryDetails {
    /// Parses the spelling used in config and `:set`, or `None` if it names no
    /// known value.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Self::None),
            "size" => Some(Self::Size),
            "modified" | "time" => Some(Self::Modified),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    /// The canonical spelling, as `[navigation] entry_details` takes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Size => "size",
            Self::Modified => "modified",
            Self::Both => "both",
        }
    }

    /// The next value in the cycle the `m` binding steps through.
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Size,
            Self::Size => Self::Modified,
            Self::Modified => Self::Both,
            Self::Both => Self::None,
        }
    }

    /// Decides how much of `row_width` this column may take, or `None` when it
    /// must stand down.
    ///
    /// The column is fixed-width — a right-flushed column whose width changed
    /// per row would not be a column — so the budget is computed once from the
    /// widest thing it can hold, and the name gets what is left. Two fallbacks
    /// keep a narrow panel usable rather than merely correct:
    ///
    /// 1. `Both` degrades to a short date (`2026-09-29`) before it degrades to
    ///    nothing, because losing the clock time costs less than losing the
    ///    size.
    /// 2. Any variant that still cannot leave [`MIN_NAME_WIDTH`] columns for
    ///    the name returns `None`, and the row is drawn as it was before this
    ///    feature existed.
    fn fit(self, row_width: usize) -> Option<DetailsLayout> {
        // One column of gap keeps the value off the end of the longest name.
        const GAP: usize = 1;

        let candidates: &[(usize, bool)] = match self {
            Self::None => return None,
            Self::Size => &[(SIZE_WIDTH, false)],
            Self::Modified => &[
                (metafmt::MODIFIED_WIDTH, false),
                (metafmt::MODIFIED_SHORT_WIDTH, true),
            ],
            Self::Both => &[
                (SIZE_WIDTH + 1 + metafmt::MODIFIED_WIDTH, false),
                (SIZE_WIDTH + 1 + metafmt::MODIFIED_SHORT_WIDTH, true),
            ],
        };

        for &(width, short_date) in candidates {
            let name_width = row_width.saturating_sub(width + GAP);
            if name_width >= MIN_NAME_WIDTH {
                return Some(DetailsLayout {
                    kind: self,
                    width,
                    name_width,
                    short_date,
                });
            }
        }
        None
    }
}

/// The width reserved for a formatted size.
///
/// `humansize` produces at most `999.99 XB` for any `u64`, which is nine
/// columns; reserving that keeps the column from shifting when a directory
/// happens to contain one very large file.
const SIZE_WIDTH: usize = 9;

/// A resolved details column: how wide it is, and how much is left for names.
#[derive(Debug, Clone, Copy)]
struct DetailsLayout {
    /// Which details are being shown.
    kind: EntryDetails,
    /// Columns reserved for the value itself.
    width: usize,
    /// Columns left for the name, git badge included.
    name_width: usize,
    /// Whether the time is drawn as a bare date because the full stamp did not
    /// fit.
    short_date: bool,
}

/// Draws the navigation panel into `area`.
///
/// When `state.filter` is `Some`, renders entries in match-score order from
/// `state.filtered_entries()`. Otherwise renders `state.visible_entries()` in
/// the active tab's sort order — see [`crate::app::sort`].
///
/// The current selection is highlighted. Directories are colored blue;
/// symlinks are colored cyan; hidden entries are dimmed.
///
/// When `[navigation] entry_details` asks for one, each row also carries a
/// right-flushed details column. Its width is resolved once per frame by
/// `EntryDetails::fit` and the name is truncated and padded to what is left,
/// so the values line up; a name long enough to reach the column takes its git
/// badge with it, which is the price of not reserving two columns on every row
/// for a badge most rows do not have.
///
/// Git badges are shown as a suffix on each entry line when `entry.git_status`
/// is populated by the git worker (Phase 4). Possible badges:
/// - `M` (yellow) — modified
/// - `A` (green) — added to the index
/// - `D` (red) — deleted
/// - `?` (DarkGray) — untracked
/// - `R` (cyan) — renamed
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let styles = theme::resolve(&state.config.theme);
    let title = format!(" {} ", panel_title(&state.cwd));

    // What a row actually has to work with: the pane, less the two border
    // columns, less the two the highlight symbol takes on every row. Getting
    // this wrong by two columns wraps every line in the list.
    let row_width = usize::from(area.width)
        .saturating_sub(2)
        .saturating_sub(HIGHLIGHT_SYMBOL.len());
    let details = state.config.navigation.details().fit(row_width);

    let items: Vec<ListItem> = state
        .filtered_entries()
        .map(|(_, entry)| {
            let base_style = match entry.kind {
                EntryKind::Dir => {
                    let s = styles.directory;
                    if entry.is_hidden {
                        s.patch(styles.hidden)
                    } else {
                        s
                    }
                }
                EntryKind::Symlink => {
                    let s = styles.symlink;
                    if entry.is_hidden {
                        s.patch(styles.hidden)
                    } else {
                        s
                    }
                }
                EntryKind::File => {
                    let s = styles.normal;
                    if entry.is_hidden {
                        s.patch(styles.hidden)
                    } else {
                        s
                    }
                }
            };

            // Add a trailing `/` to directories for quick visual identification.
            // The name is sanitized because a Unix file name may contain control
            // characters, which a terminal would act on rather than draw.
            let name = provider::sanitize(&entry.file_name);
            let label = if entry.kind == EntryKind::Dir {
                format!("{name}/")
            } else {
                name
            };

            // Build git badge span (empty when no status is known).
            let git_span = match &entry.git_status {
                Some(GitFileStatus::Modified) => Some(Span::styled(" M", styles.git_dirty)),
                Some(GitFileStatus::Added) => Some(Span::styled(" A", styles.git_clean)),
                Some(GitFileStatus::Deleted) => Some(Span::styled(
                    " D",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Some(GitFileStatus::Untracked) => {
                    Some(Span::styled(" ?", Style::default().fg(Color::DarkGray)))
                }
                Some(GitFileStatus::Renamed) => {
                    Some(Span::styled(" R", Style::default().fg(Color::Cyan)))
                }
                Some(GitFileStatus::Clean) | None => None,
            };

            let mut spans = vec![Span::styled(format!(" {label}"), base_style)];
            if let Some(badge) = git_span {
                spans.push(badge);
            }

            if let Some(layout) = details {
                // The name is truncated and padded to its budget so the value
                // lands in the same column on every row — a right-flushed
                // column that moves with the name length is not a column.
                let used = truncate_spans(&mut spans, layout.name_width);
                let pad = layout.name_width.saturating_sub(used) + 1;
                spans.push(Span::raw(" ".repeat(pad)));
                spans.push(Span::styled(
                    format!(
                        "{:>width$}",
                        details_text(entry, layout),
                        width = layout.width
                    ),
                    styles.status,
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(styles.border)
        .border_type(BorderType::Rounded);

    let list = List::new(items)
        .block(block)
        .highlight_style(styles.selection)
        .highlight_symbol(HIGHLIGHT_SYMBOL);

    // Drive the list widget's selection via ListState.
    let mut list_state = ListState::default();
    if state.filtered_count() > 0 {
        list_state.select(Some(state.selected));
    }

    frame.render_stateful_widget(list, area, &mut list_state);
}

/// What `List` draws in front of the selected row, and reserves in front of
/// every other one.
///
/// Named because the details column's budget depends on its width; a literal in
/// two places is a two-column layout bug waiting to happen.
const HIGHLIGHT_SYMBOL: &str = "> ";

/// The value the details column shows for `entry`, unpadded.
fn details_text(entry: &Entry, layout: DetailsLayout) -> String {
    let size = || {
        entry
            .metadata
            .as_ref()
            // A directory's byte length describes its directory record rather
            // than its contents, so the column says nothing instead of saying
            // something false. `sort_entries` declines to rank it for the same
            // reason.
            .filter(|_| entry.kind != EntryKind::Dir)
            .map(|m| metafmt::size(m.len()))
            .unwrap_or_else(|| metafmt::UNKNOWN.to_owned())
    };
    let time = || {
        let t = entry.metadata.as_ref().and_then(|m| m.modified().ok());
        if layout.short_date {
            metafmt::modified_short(t)
        } else {
            metafmt::modified(t)
        }
    };

    match layout.kind {
        EntryDetails::None => String::new(),
        EntryDetails::Size => size(),
        EntryDetails::Modified => time(),
        EntryDetails::Both => {
            let time_width = if layout.short_date {
                metafmt::MODIFIED_SHORT_WIDTH
            } else {
                metafmt::MODIFIED_WIDTH
            };
            let size_width = SIZE_WIDTH;
            format!("{:>size_width$} {:>time_width$}", size(), time())
        }
    }
}

/// Trims `spans` so they occupy at most `budget` columns, marking a trim with
/// `…`, and returns the width they occupy afterwards.
///
/// Columns are counted in `char`s rather than display width, the same trade-off
/// the command line makes in [`crate::ui::status_bar`]: `unicode-width` is not
/// a dependency, and the cost of being wrong is a column that looks ragged on a
/// row whose name contains double-width characters, not a corrupted listing.
fn truncate_spans(spans: &mut Vec<Span<'static>>, budget: usize) -> usize {
    let total: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    if total <= budget {
        return total;
    }

    let mut remaining = budget;
    let mut kept: Vec<Span<'static>> = Vec::with_capacity(spans.len());
    for span in spans.drain(..) {
        let len = span.content.chars().count();
        if len <= remaining {
            remaining -= len;
            kept.push(span);
            continue;
        }
        // This span is where the budget runs out. Keep what fits, less one
        // column for the ellipsis that says something was dropped.
        if remaining > 0 {
            let head: String = span
                .content
                .chars()
                .take(remaining.saturating_sub(1))
                .collect();
            kept.push(Span::styled(format!("{head}…"), span.style));
        }
        break;
    }

    *spans = kept;
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// The panel's border title: the directory's own name.
///
/// The status bar already carries the full path, and drawing it here as well
/// spent the title on a second copy of it — on a deep path, the one thing the
/// title could not then show was which directory you were actually in, because
/// the interesting end was the part that got cut.
///
/// A root has no name of its own, so it keeps its full spelling: `C:\` and `/`
/// are already as short as they get.
fn panel_title(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| pathfmt::display(cwd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_parse_round_trips_every_value() {
        for value in ["none", "size", "modified", "both"] {
            let parsed = EntryDetails::parse(value).expect("canonical spelling parses");
            assert_eq!(parsed.as_str(), value);
        }
        assert_eq!(EntryDetails::parse("off"), Some(EntryDetails::None));
        assert_eq!(EntryDetails::parse("bogus"), None);
    }

    #[test]
    fn details_cycle_returns_to_none() {
        let mut d = EntryDetails::None;
        for _ in 0..4 {
            d = d.next();
        }
        assert_eq!(d, EntryDetails::None, "`m` four times is a no-op");
    }

    #[test]
    fn none_never_takes_a_column() {
        assert!(EntryDetails::None.fit(200).is_none());
    }

    #[test]
    fn a_size_column_fits_an_eighty_column_terminal() {
        // 40% of 80 is 32; less the border and the highlight symbol, 28.
        let layout = EntryDetails::Size.fit(28).expect("size fits at 80 columns");
        assert_eq!(layout.width, SIZE_WIDTH);
        assert_eq!(layout.name_width, 28 - SIZE_WIDTH - 1);
        assert!(!layout.short_date);
    }

    #[test]
    fn both_stands_down_rather_than_crush_the_name() {
        // `both` wants 26 of the 28 columns an 80-column terminal offers, which
        // would leave two for the name. The name wins.
        assert!(EntryDetails::Both.fit(28).is_none());
    }

    #[test]
    fn both_drops_the_clock_before_it_drops_the_column() {
        // Wide enough for size + a bare date + a readable name, but not for the
        // full timestamp: the date is the part that distinguishes files.
        let width = SIZE_WIDTH + 1 + metafmt::MODIFIED_SHORT_WIDTH + 1 + MIN_NAME_WIDTH;
        let layout = EntryDetails::Both.fit(width).expect("the short form fits");
        assert!(layout.short_date);
        assert_eq!(layout.name_width, MIN_NAME_WIDTH);
    }

    #[test]
    fn a_name_budget_below_the_floor_gives_up_the_column() {
        let just_under = SIZE_WIDTH + 1 + (MIN_NAME_WIDTH - 1);
        assert!(EntryDetails::Size.fit(just_under).is_none());
        assert!(EntryDetails::Size.fit(just_under + 1).is_some());
    }

    #[test]
    fn truncation_marks_what_it_dropped_and_respects_the_budget() {
        let mut spans = vec![
            Span::raw(" a-very-long-file-name.txt".to_owned()),
            Span::raw(" M".to_owned()),
        ];
        let used = truncate_spans(&mut spans, 10);
        assert_eq!(used, 10, "the budget is filled exactly");
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.ends_with('…'), "a cut has to be visible: {text:?}");
    }

    #[test]
    fn truncation_leaves_something_that_already_fits_alone() {
        let mut spans = vec![Span::raw(" short.txt".to_owned())];
        let used = truncate_spans(&mut spans, 40);
        assert_eq!(used, 10);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), " short.txt");
    }

    #[test]
    fn title_is_the_directory_name() {
        // `/` is a separator on every platform Trail builds for, Windows
        // included, so this is the case that can be asserted everywhere.
        assert_eq!(panel_title(Path::new("/home/me/project")), "project");
    }

    /// A backslash is a separator only on Windows. Off it, a file may legitimately
    /// be *named* `C:\Users\me\project`, and the whole string is then its own
    /// name — so this case belongs to the platform whose paths they are.
    #[cfg(windows)]
    #[test]
    fn title_is_the_directory_name_for_a_windows_path() {
        assert_eq!(panel_title(Path::new(r"C:\Users\me\project")), "project");
    }

    #[test]
    fn title_of_a_root_is_the_root() {
        // No name of its own to fall back to, and nothing shorter to show.
        assert_eq!(panel_title(Path::new("/")), "/");
    }
}
