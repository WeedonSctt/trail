//! Navigation panel rendering.
//!
//! Renders the directory listing with the highlighted selection. When a fuzzy
//! filter is active (Search Mode), only the matching entries are shown, ordered
//! by descending match score. Directories are shown before files in Navigation
//! Mode; hidden entries are dimmed when visible. Git badges are added in Phase 4.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::state::{AppState, EntryKind};

/// Draws the navigation panel into `area`.
///
/// When `state.filter` is `Some`, renders entries in match-score order from
/// `state.filtered_entries()`. Otherwise renders `state.visible_entries()` in
/// the usual directory-first sorted order.
///
/// The current selection is highlighted. Directories are colored blue;
/// symlinks are colored cyan; hidden entries are dimmed.
///
/// Phase 4 will add git badges (modified indicator, branch icon) once the
/// git worker reports back.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let title = format!(" {} ", state.cwd.display());

    let items: Vec<ListItem> = state
        .filtered_entries()
        .map(|(_, entry)| {
            let style = match entry.kind {
                EntryKind::Dir => {
                    let base = Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD);
                    if entry.is_hidden {
                        base.add_modifier(Modifier::DIM)
                    } else {
                        base
                    }
                }
                EntryKind::Symlink => {
                    let base = Style::default().fg(Color::Cyan);
                    if entry.is_hidden {
                        base.add_modifier(Modifier::DIM)
                    } else {
                        base
                    }
                }
                EntryKind::File => {
                    let base = Style::default();
                    if entry.is_hidden {
                        base.add_modifier(Modifier::DIM)
                    } else {
                        base
                    }
                }
            };

            // Add a trailing `/` to directories for quick visual identification.
            let label = if entry.kind == EntryKind::Dir {
                format!("{}/", entry.file_name)
            } else {
                entry.file_name.clone()
            };

            ListItem::new(Line::from(Span::styled(format!(" {label}"), style)))
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
