//! UI rendering: draws the three-panel layout (nav, preview, status bar).
//!
//! Corresponds to the architecture doc's UI thread rendering responsibility.
//! `render()` is the single entry point called once per tick when `state.dirty`
//! is set. The function is generic over `B: Backend` so that tests can pass a
//! `TestBackend` without a real terminal.

mod nav_panel;
mod preview_panel;
mod status_bar;
mod theme;

use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use std::io;

use crate::app::state::AppState;

/// Draws all three panels into the terminal frame.
///
/// The layout splits the screen into a top region (nav + preview side by side)
/// and a bottom status bar. The top region is split 40/60 between the
/// navigation panel and the preview panel.
///
/// This function does **not** clear `state.dirty`; the caller (`main.rs`)
/// is responsible for setting it to `false` after a successful render so
/// that the invariant is visible at the event-loop level.
///
/// Generic over `B: Backend` so that integration tests can use
/// `ratatui::backend::TestBackend` without a real terminal.
///
/// Takes `state` mutably because an image preview owns encoder state that
/// `ratatui-image` re-encodes whenever the preview pane changes size.
pub fn render<B: Backend>(terminal: &mut Terminal<B>, state: &mut AppState) -> io::Result<()> {
    // Physically clear the screen and reset ratatui's previous buffer before
    // every draw. This prevents ghost content from lingering on the physical
    // terminal when content shrinks between frames (e.g. navigating from a
    // large file preview to a short binary metadata preview).
    //
    // The buffer-diff approach (frame.render_widget(Clear, …)) is insufficient
    // here: it fills ratatui's internal buffer with blanks, but the diff only
    // emits "blank" escape sequences for cells whose previous-buffer state was
    // non-blank. When the physical terminal drifts out of sync with ratatui's
    // buffer (rapid async transitions, Windows Terminal repaints, etc.), the
    // diff sees "blank → blank" and emits nothing — leaving ghost content.
    //
    // terminal.clear() performs a physical "clear screen" escape AND resets
    // ratatui's previous buffer to all-default cells, so the subsequent draw()
    // diffs against a blank baseline and writes every visible cell from scratch.
    terminal.clear()?;

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

        nav_panel::draw(frame, inner[0], state);
        status_bar::draw(frame, outer[1], state);
        // Drawn last: it borrows `state` mutably, so the read-only panels above
        // must have finished with it.
        preview_panel::draw(frame, inner[1], state);
    })?;
    Ok(())
}
