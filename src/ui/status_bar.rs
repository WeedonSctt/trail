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
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD),
        Mode::Search { .. } => Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        Mode::Command { .. } => Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    };

    let cwd_str = &state.status.cwd_display;

    let left = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {mode_label} "), mode_style),
        Span::raw(" "),
        Span::styled(cwd_str.as_str(), Style::default().fg(Color::White)),
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
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )
    } else if let Some(ref err) = state.error_message {
        Span::styled(
            format!(" Error: {err} "),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        match &state.mode {
            Mode::Command { buffer, .. } => {
                // Display the command buffer with the leading sentinel.
                // When the buffer starts with '!' it is a shell command;
                // otherwise it is a ':'-prefixed verb command.
                let prompt = if buffer.starts_with('!') { "" } else { ":" };
                Span::styled(
                    format!("{prompt}{buffer}"),
                    Style::default().fg(Color::Cyan),
                )
            }
            Mode::Search { query, .. } => {
                Span::styled(format!("/{query}"), Style::default().fg(Color::Yellow))
            }
            Mode::Navigation => {
                if let Some(ref yank) = state.last_yank {
                    // Show what was yanked as brief feedback.
                    Span::styled(
                        format!(" yanked: {yank} "),
                        Style::default().fg(Color::Green),
                    )
                } else {
                    Span::raw("")
                }
            }
        }
    };

    let center = Paragraph::new(Line::from(vec![center_span]));
    frame.render_widget(center, sections[1]);

    // ── Right section: entry count; Phase 4 adds git branch ──────────────────
    // TODO(phase-4): Append git branch from state.git when available.
    let count_text = format!("{} items ", state.status.entry_count);
    let right = Paragraph::new(Line::from(Span::styled(
        count_text,
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(right, sections[2]);
}
