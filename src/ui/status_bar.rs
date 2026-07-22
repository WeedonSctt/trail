//! Status bar rendering.
//!
//! Pure reflection of current state — no logic beyond formatting. Displays
//! `cwd`, mode label, active filter string, git branch, and entry count.

use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

/// Draws the status bar as a bordered region.
///
/// Phase 0: empty placeholder. Phase 1 adds cwd, mode, and entry count.
/// Phase 4 adds the git branch.
pub fn draw(frame: &mut Frame, area: Rect) {
    let block = Block::default().title(" Status ").borders(Borders::ALL);
    frame.render_widget(block, area);
}
