//! UI rendering: draws the three-panel layout (nav, preview, status bar), and
//! the terminal panel beneath the first two when it is open.
//!
//! Corresponds to the architecture doc's UI thread rendering responsibility.
//! `render()` is the single entry point called once per tick when `state.dirty`
//! is set. The function is generic over `B: Backend` so that tests can pass a
//! `TestBackend` without a real terminal.

pub mod nav_panel;
mod preview_panel;
mod status_bar;
mod tab_panel;
mod terminal_panel;
mod theme;

pub use tab_panel::MIN_ROWS as TAB_PANEL_MIN_ROWS;

/// The colour a `[theme]` value names, for text printed outside the TUI, such
/// as the exit summary's warnings. An unknown name reads as the terminal's
/// default, as it does on screen.
pub fn theme_colour(value: &str) -> ratatui::style::Color {
    theme::parse_color(value)
}

use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Terminal;
use std::io;

use crate::app::mode::Mode;
use crate::app::state::{AppState, PreviewSlot};
use crate::preview::provider::PreviewContent;

/// Where each panel goes this frame.
#[derive(Debug, Clone, Copy)]
struct Panes {
    nav: Rect,
    preview: Rect,
    /// `None` while the terminal panel is closed.
    terminal: Option<Rect>,
    /// `None` while the tab panel is closed, or there is no room to draw it.
    tabs: Option<Rect>,
    status: Rect,
    /// False while a maximized terminal panel, or a tab panel that has reached
    /// the top, leaves no room for the file list and the preview, which are then
    /// not drawn at all.
    files_shown: bool,
    /// Rows for the file list and preview before the tab panel takes any.
    file_area_rows: u16,
}

impl Panes {
    /// Lays the panels out in `area`.
    ///
    /// Separate from the draw because the preview's position has to be known
    /// before it: the screen can only be cleared between frames.
    fn new(area: Rect, state: &AppState) -> Panes {
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // main area (nav + preview)
                Constraint::Length(1), // status bar
            ])
            .split(area);

        let terminal_rows = state.terminal.is_visible().then(|| {
            crate::terminal::panel_rows(
                outer[0].height,
                state.config.terminal.height,
                state.terminal.is_maximized(),
            )
        });
        // The tab panel's rows come off the file area only. An open terminal
        // panel moves up and keeps its size: if it gave up rows too, every `t`
        // would resize its shells and reflow their output.
        let file_area_rows = outer[0]
            .height
            .saturating_sub(terminal_rows.unwrap_or_default());
        let tab_rows = matches!(state.mode, Mode::Tabs { .. })
            .then(|| tab_panel::rows(state.tab_manager.len(), file_area_rows))
            .flatten();

        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(terminal_rows.unwrap_or_default()),
                Constraint::Length(tab_rows.unwrap_or_default()),
            ])
            .split(outer[0]);
        let main = split[0];

        let inner = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // navigation panel
                Constraint::Percentage(60), // preview panel
            ])
            .split(main);

        Panes {
            nav: inner[0],
            preview: inner[1],
            terminal: terminal_rows.map(|_| split[1]),
            tabs: tab_rows.map(|_| split[2]),
            status: outer[1],
            files_shown: main.height > 0,
            file_area_rows,
        }
    }
}

/// Records where the current image is about to be drawn, returning whether the
/// screen must be cleared first because it moved or has only just arrived.
///
/// See [`crate::preview::graphics::ImagePreview::placed_at`] for why. Half-blocks
/// repaint every cell they use, so they never ask for a clear.
fn image_needs_clear(slot: &mut PreviewSlot, pane: Option<Rect>) -> bool {
    let PreviewContent::Image(image) = &mut slot.content else {
        return false;
    };
    let moved = pane.is_some() && image.placed_at != pane;
    image.placed_at = pane;
    moved && image.draws_over_cells
}

/// Draws all three panels into the terminal frame.
///
/// The layout splits the screen into a top region (nav + preview side by side)
/// and a bottom status bar. The top region is split 40/60 between the
/// navigation panel and the preview panel. An open terminal panel takes the
/// bottom `[terminal] height` percent of the top region, or all of it when
/// maximized — in which case the file list and preview are not drawn. An open tab
/// panel sits directly above the status bar, taking its rows from the file area.
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
    let size = terminal.size()?;
    let panes = Panes::new(Rect::new(0, 0, size.width, size.height), state);
    state.file_area_rows = panes.file_area_rows;
    // The window shrank, or a shell was maximized, with the panel open. Otherwise
    // the keys would go on driving a selector nobody can see.
    if matches!(state.mode, Mode::Tabs { .. }) && panes.tabs.is_none() {
        state.mode = Mode::Navigation;
    }
    let drawing_image =
        panes.files_shown && matches!(state.preview.content, PreviewContent::Image(_));
    let image_moved = image_needs_clear(
        &mut state.preview,
        panes.files_shown.then_some(panes.preview),
    );
    if image_moved || (state.preview.drew_image && !drawing_image) {
        terminal.clear()?;
    }
    state.preview.drew_image = drawing_image;

    terminal.draw(|frame| {
        status_bar::draw(frame, panes.status, state);
        if let Some(panel) = panes.terminal {
            terminal_panel::draw(frame, panel, state);
        }
        if let Some(panel) = panes.tabs {
            tab_panel::draw(frame, panel, state);
        }
        if !panes.files_shown {
            return;
        }

        // The panel's scroll position is decided here rather than inside the
        // panel because it outlives the frame: the panel borrows `state` only
        // to read, and this is where the pane height is first known.
        let listed = state.filtered_count();
        let nav_offset = crate::app::scroll::NavScroll::update(
            &mut state.nav_scroll,
            &state.cwd,
            state.selected,
            listed,
            usize::from(panes.nav.height.saturating_sub(2)),
            state.config.navigation.scroll_margin,
        );
        nav_panel::draw(frame, panes.nav, state, nav_offset);
        // Drawn last: it borrows `state` mutably, so the read-only panels above
        // must have finished with it.
        preview_panel::draw(frame, panes.preview, state);
    })?;
    Ok(())
}
