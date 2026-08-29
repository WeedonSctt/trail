//! Status bar rendering.
//!
//! Pure reflection of current state — no logic beyond formatting. Displays
//! `cwd`, mode label, active filter string, git branch, entry count,
//! Command Mode input buffer, validation errors, and delete confirmation.
//! Phase 4 adds the git branch indicator.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::mode::Mode;
use crate::app::state::AppState;
use crate::ui::theme;

/// Draws the status bar into `area`.
///
/// Layout adapts to the current mode:
///
/// - **Navigation**: mode badge + cwd | (empty) | entry count.
/// - **Search**: mode badge + cwd | `/query` | entry count.
/// - **Command**: mode badge + cwd | command buffer | entry count.
/// - **Error**: replaces the center section with the error message in red.
/// - **Pending delete**: center section shows a delete confirmation prompt.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let styles = theme::resolve(&state.config.theme);
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // left: mode + cwd / command line
            Constraint::Percentage(30), // center: filter / error / command buffer
            Constraint::Percentage(20), // right: count / git branch (Phase 4)
        ])
        .split(area);

    // ── Left section: mode badge + cwd ────────────────────────────────────────
    let mode_label = state.mode.label();
    let mode_style = match &state.mode {
        Mode::Navigation => Style::default()
            .fg(Color::Black)
            .bg(theme::parse_color(&state.config.theme.git_clean))
            .add_modifier(Modifier::BOLD),
        Mode::Search { .. } => Style::default()
            .fg(Color::Black)
            .bg(theme::parse_color(&state.config.theme.search))
            .add_modifier(Modifier::BOLD),
        Mode::Command { .. } => Style::default()
            .fg(Color::Black)
            .bg(theme::parse_color(&state.config.theme.command))
            .add_modifier(Modifier::BOLD),
    };

    let cwd_str = &state.status.cwd_display;

    let left = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {mode_label} "), mode_style),
        Span::raw(" "),
        Span::styled(cwd_str.as_str(), styles.normal),
    ]));
    frame.render_widget(left, sections[0]);

    // ── Center section ─────────────────────────────────────────────────────────
    //
    // Priority (highest first):
    //   1. Pending-delete confirmation prompt.
    //   2. Error message (red).
    //   3. Command Mode input buffer.
    //   4. Search Mode filter query.
    //   5. Last yank notification (brief feedback).
    //   6. Empty.
    let center_span: Span = if state.pending_delete {
        let name = state
            .selected_entry()
            .map(|e| e.file_name.as_str())
            .unwrap_or("selected entry");
        Span::styled(
            format!(" Delete '{name}'? [y/Enter=yes, n/Esc=cancel] "),
            Style::default()
                .fg(Color::Black)
                .bg(theme::parse_color(&state.config.theme.error))
                .add_modifier(Modifier::BOLD),
        )
    } else if let Some(ref err) = state.error_message {
        Span::styled(format!(" Error: {err} "), styles.error)
    } else {
        match &state.mode {
            Mode::Command { buffer, .. } => {
                // Display the command buffer with the leading sentinel.
                // When the buffer starts with '!' it is a shell command;
                // otherwise it is a ':'-prefixed verb command.
                let prompt = if buffer.starts_with('!') { "" } else { ":" };
                Span::styled(format!("{prompt}{buffer}"), styles.command)
            }
            Mode::Search { query, .. } => Span::styled(format!("/{query}"), styles.search),
            Mode::Navigation => {
                if let Some(ref yank) = state.last_yank {
                    // Show what was yanked as brief feedback.
                    Span::styled(
                        format!(" yanked: {} ", yank_preview(yank)),
                        styles.git_clean,
                    )
                } else {
                    Span::raw("")
                }
            }
        }
    };

    let center = Paragraph::new(Line::from(vec![center_span]));
    frame.render_widget(center, sections[1]);

    // ── Right section: entry count + git branch ───────────────────────────────
    let right_text = if let Some(ref git) = state.git {
        let dirty_marker = if git.is_dirty { " *" } else { "" };
        format!(
            "  {} {}{}  {} items ",
            "\u{e0a0}", git.branch, dirty_marker, state.status.entry_count
        )
    } else {
        format!("{} items ", state.status.entry_count)
    };
    let right = Paragraph::new(Line::from(Span::styled(right_text, styles.status)));
    frame.render_widget(right, sections[2]);
}

/// Characters of a yank shown in the status bar before it is elided.
///
/// Named constant per coding-standard §10: no magic numbers.
const YANK_PREVIEW_MAX_CHARS: usize = 40;

/// Condenses a yanked string into one short line of status-bar feedback.
///
/// `yc` puts whole files on the clipboard, so `last_yank` is no longer
/// guaranteed to be a short single-line path: the raw string would spill
/// newlines and control characters into a one-line widget. Only the first line
/// is shown, capped at [`YANK_PREVIEW_MAX_CHARS`] and marked with `…` when
/// anything was left out.
///
/// Scans at most one line and `YANK_PREVIEW_MAX_CHARS + 1` characters of it, so
/// the cost does not grow with the size of the yank — this runs every frame.
fn yank_preview(yank: &str) -> String {
    let first_line = yank.split('\n').next().unwrap_or("");

    let mut visible = first_line.chars().filter(|c| !c.is_control());
    let mut preview: String = visible.by_ref().take(YANK_PREVIEW_MAX_CHARS).collect();

    // Elided if the line ran past the cap, or if there were further lines.
    if visible.next().is_some() || first_line.len() < yank.len() {
        preview.push('…');
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yank_preview_passes_short_paths_through() {
        assert_eq!(yank_preview("src/main.rs"), "src/main.rs");
    }

    #[test]
    fn yank_preview_keeps_only_the_first_line() {
        assert_eq!(
            yank_preview("fn main() {\n    todo!()\n}\n"),
            "fn main() {…"
        );
    }

    #[test]
    fn yank_preview_truncates_a_long_line() {
        let long = "a".repeat(YANK_PREVIEW_MAX_CHARS * 2);
        let preview = yank_preview(&long);
        assert_eq!(preview.chars().count(), YANK_PREVIEW_MAX_CHARS + 1);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn yank_preview_drops_control_characters() {
        // A lone CR would otherwise reset the cursor mid-status-bar.
        assert_eq!(yank_preview("a\tb\rc"), "abc");
    }
}
