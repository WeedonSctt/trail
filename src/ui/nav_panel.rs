//! Navigation panel rendering.
//!
//! Renders the directory listing with the highlighted selection. When a fuzzy
//! filter is active (Search Mode), only the matching entries are shown, ordered
//! by descending match score. Directories are shown before files in Navigation
//! Mode; hidden entries are dimmed when visible. Git badges are rendered in
//! Phase 4 once the git worker populates `entry.git_status`.

use std::path::Path;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::state::{AppState, EntryKind, GitFileStatus};
use crate::pathfmt;
use crate::ui::theme;

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
    let styles = theme::resolve(&state.config.theme);
    let title = format!(" {} ", panel_title(&state.cwd));

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
            let label = if entry.kind == EntryKind::Dir {
                format!("{}/", entry.file_name)
            } else {
                entry.file_name.clone()
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
        .highlight_symbol("> ");

    // Drive the list widget's selection via ListState.
    let mut list_state = ListState::default();
    if state.filtered_count() > 0 {
        list_state.select(Some(state.selected));
    }

    frame.render_stateful_widget(list, area, &mut list_state);
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
