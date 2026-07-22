//! Preview panel rendering.
//!
//! Renders the preview of the currently selected entry. In Phase 0 this is an
//! empty bordered region; Phase 1 adds synchronous directory/text preview,
//! and Phase 5 adds syntax highlighting, binary metadata, and image preview.

use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

/// Draws the preview panel as a bordered region.
///
/// Phase 0: empty placeholder. Later phases dispatch by `Entry::kind` to
/// a `PreviewProvider` and render the content of `state.preview`.
pub fn draw(frame: &mut Frame, area: Rect) {
    let block = Block::default().title(" Preview ").borders(Borders::ALL);
    frame.render_widget(block, area);
}
