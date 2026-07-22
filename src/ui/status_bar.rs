//! Status bar rendering.
//!
//! Pure reflection of current state — no logic beyond formatting. Displays
//! `cwd`, mode label, active filter string, git branch, and entry count.
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
/// The bar is split into three sections:
/// - **Left**: mode badge + current working directory.
/// - **Center**: active filter string (Search Mode only; Phase 2 populates).
/// - **Right**: entry count. Phase 4 adds git branch here.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60), // left: mode + cwd
            Constraint::Percentage(20), // center: filter (Phase 2)
            Constraint::Percentage(20), // right: count / git branch
        ])
        .split(area);

    // ── Left section: mode badge + cwd ────────────────────────────────────
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

    // ── Center section: filter query (Phase 2 will populate) ─────────────
    let filter_text = match &state.mode {
        Mode::Search { query, .. } => format!("/{query}"),
        _ => String::new(),
    };
    let center = Paragraph::new(Span::styled(
        filter_text,
        Style::default().fg(Color::Yellow),
    ));
    frame.render_widget(center, sections[1]);

    // ── Right section: entry count; Phase 4 adds git branch ──────────────
    // TODO(phase-4): Append git branch from state.git when available.
    let count_text = format!("{} items ", state.status.entry_count);
    let right = Paragraph::new(Line::from(Span::styled(
        count_text,
        Style::default().fg(Color::DarkGray),
    )));
    // Align to the right by padding — ratatui Paragraph doesn't have right-align,
    // so we rely on the Constraint allocation.
    frame.render_widget(right, sections[2]);
}
