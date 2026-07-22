//! Navigation panel rendering.
//!
//! Renders the directory listing with the highlighted selection. In Phase 0
//! this is an empty bordered region; Phase 1 populates it with `state.entries`.

use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

/// Draws the navigation panel as a bordered region.
///
/// Phase 0: empty placeholder. Phase 1 will render `state.entries` here,
/// with directory-first sort and hidden-file dimming.
pub fn draw(frame: &mut Frame, area: Rect) {
    let block = Block::default().title(" Navigation ").borders(Borders::ALL);
    frame.render_widget(block, area);
}
