//! Navigation panel rendering.
//!
//! Renders the directory listing with the highlighted selection. When a fuzzy
//! filter is active (Search Mode), only the matching entries are shown, ordered
//! by descending match score. Directories are shown before files in Navigation
//! Mode; hidden entries are dimmed when visible. Git badges are rendered in
//! Phase 4 once the git worker populates `entry.git_status`.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::state::{AppState, EntryKind, GitFileStatus};

/// Draws the navigation panel into `area`.
///
/// When `state.filter` is `Some`, renders entries in match-score order from
/// `state.filtered_entries()`. Otherwise renders `state.visible_entries()` in
/// the usual directory-first sorted order.
///
/// The current selection is highlighted. Directories are colored blue;
/// symlinks are colored cyan; hidden entries are dimmed.
///
/// Git badges are shown as a suffix on each entry line when `entry.git_status`
/// is populated by the git worker (Phase 4). Possible badges:
/// - `M` (yellow) — modified
/// - `A` (green) — added to the index
/// - `D` (red) — deleted
/// - `?` (DarkGray) — untracked
/// - `R` (cyan) — renamed
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let title = format!(" {} ", state.cwd.display());

    let items: Vec<ListItem> = state
        .filtered_entries()
        .map(|(_, entry)| {
            let base_style = match entry.kind {
                EntryKind::Dir => {
                    let s = Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD);
                    if entry.is_hidden {
                        s.add_modifier(Modifier::DIM)
                    } else {
                        s
                    }
                }
                EntryKind::Symlink => {
                    let s = Style::default().fg(Color::Cyan);
                    if entry.is_hidden {
                        s.add_modifier(Modifier::DIM)
                    } else {
                        s
                    }
                }
                EntryKind::File => {
                    let s = Style::default();
                    if entry.is_hidden {
                        s.add_modifier(Modifier::DIM)
                    } else {
                        s
                    }
                }
            };

            // Add a trailing `/` to directories for quick visual identification.
            let label = if entry.kind == EntryKind::Dir {
                format!("{}/", entry.file_name)
            } else {
                entry.file_name.clone()
            };

            // Build git badge span (empty when no status is known).
            let git_span = match &entry.git_status {
                Some(GitFileStatus::Modified) => Some(Span::styled(
                    " M",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Some(GitFileStatus::Added) => Some(Span::styled(
                    " A",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )),
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

            ListItem::new(Line::from(spans))
        })
        .collect();

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    // Drive the list widget's selection via ListState.
    let mut list_state = ListState::default();
    if state.filtered_count() > 0 {
        list_state.select(Some(state.selected));
    }

    frame.render_stateful_widget(list, area, &mut list_state);
}
