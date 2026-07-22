//! Navigation panel rendering.
//!
//! Renders the directory listing with the highlighted selection. Directories
//! are shown before files; hidden entries are dimmed when visible. Git badges
//! are added in Phase 4.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::state::{AppState, EntryKind};

/// Draws the navigation panel into `area`.
///
/// Renders `state.entries` (filtered by `show_hidden`) as a selectable list
/// with the current selection highlighted. Directories are colored blue;
/// symlinks are colored cyan; hidden entries are dimmed.
///
/// Phase 4 will add git badges (modified indicator, branch icon) once the
/// git worker reports back.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let title = format!(" {} ", state.cwd.display());

    let items: Vec<ListItem> = state
        .visible_entries()
        .map(|entry| {
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
    if state.visible_count() > 0 {
        list_state.select(Some(state.selected));
    }

    frame.render_stateful_widget(list, area, &mut list_state);
}
