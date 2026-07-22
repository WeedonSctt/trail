//! UI rendering: draws the three-panel layout (nav, preview, status bar).
//!
//! Corresponds to the architecture doc's UI thread rendering responsibility.
//! `render()` is the single entry point called once per tick when `state.dirty`
//! is set.

mod nav_panel;
mod preview_panel;
mod status_bar;
mod theme;

use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use std::io::{self, Stdout};

/// Draws all three panels into the terminal frame.
///
/// The layout splits the screen into a top region (nav + preview side by side)
/// and a bottom status bar. The top region is split 40/60 between the
/// navigation panel and the preview panel.
pub fn render(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    terminal.draw(|frame| {
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // main area (nav + preview)
                Constraint::Length(1), // status bar
            ])
            .split(frame.area());

        let inner = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // navigation panel
                Constraint::Percentage(60), // preview panel
            ])
            .split(outer[0]);

        nav_panel::draw(frame, inner[0]);
        preview_panel::draw(frame, inner[1]);
        status_bar::draw(frame, outer[1]);
    })?;
    Ok(())
}
