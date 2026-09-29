//! UI rendering: draws the three-panel layout (nav, preview, status bar).
//!
//! Corresponds to the architecture doc's UI thread rendering responsibility.
//! `render()` is the single entry point called once per tick when `state.dirty`
//! is set. The function is generic over `B: Backend` so that tests can pass a
//! `TestBackend` without a real terminal.

pub mod nav_panel;
mod preview_panel;
mod status_bar;
mod theme;

use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use std::io;

use crate::app::state::AppState;
use crate::preview::provider::PreviewContent;

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
    // An inline image is drawn by the *terminal*, not by ratatui: the protocol
    // sequence places a picture that the cell diff knows nothing about and
    // therefore cannot erase. Clearing when the previous frame drew one and this
    // one does not is what keeps a placement from sitting under the next
    // preview.
    //
    // This used to be an unconditional `terminal.clear()` on every frame — a
    // physical clear-screen and a full repaint per keystroke, which is what made
    // navigation flicker. The ghost content it was hiding came from preview text
    // being written to the terminal unsanitized, so a stray ESC in a "text" file
    // moved the cursor and painted outside the pane. That is fixed at the source
    // in `preview::provider::sanitize`, which leaves ratatui's own diff free to
    // do what it is good at.
    let drawing_image = matches!(state.preview.content, PreviewContent::Image(_));
    if state.preview.drew_image && !drawing_image {
        terminal.clear()?;
    }
    state.preview.drew_image = drawing_image;

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

        // The panel's scroll position is decided here rather than inside the
        // panel because it outlives the frame: the panel borrows `state` only
        // to read, and this is where the pane height is first known.
        let listed = state.filtered_count();
        let nav_offset = crate::app::scroll::NavScroll::update(
            &mut state.nav_scroll,
            &state.cwd,
            state.selected,
            listed,
            usize::from(inner[0].height.saturating_sub(2)),
            state.config.navigation.scroll_margin,
        );
        nav_panel::draw(frame, inner[0], state, nav_offset);
        status_bar::draw(frame, outer[1], state);
        // Drawn last: it borrows `state` mutably, so the read-only panels above
        // must have finished with it.
        preview_panel::draw(frame, inner[1], state);
    })?;
    Ok(())
}
