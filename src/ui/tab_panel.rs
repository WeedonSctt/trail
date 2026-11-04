//! Tab panel rendering: the open tabs, labelled with the directory each is in,
//! and the selector that picks one.
//!
//! Open only in `Mode::Tabs`. The panel takes rows from the file area rather
//! than drawing over it, because a real image preview is drawn by the terminal
//! and would sit on top of anything ratatui drew across it.

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, HighlightSpacing, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::mode::Mode;
use crate::app::scroll;
use crate::app::state::AppState;
use crate::pathfmt;
use crate::preview::provider::sanitize;
use crate::ui::nav_panel::HIGHLIGHT_SYMBOL;
use crate::ui::theme;

/// The two rows the border takes.
const BORDER_ROWS: u16 = 2;

/// The fewest rows the file area can have for the panel to open: one tab and its
/// border. Below this `t` says there is no room.
pub const MIN_ROWS: u16 = BORDER_ROWS + 1;

/// How many of `room` rows the panel takes for `tabs` open tabs, or `None` when it
/// does not fit.
///
/// One row per tab plus the border, so its top grows upward with the tabs. Past
/// that it takes everything and scrolls. So does a panel that would leave too few
/// rows to draw the listing in, which would be nothing but its border.
pub(super) fn rows(tabs: usize, room: u16) -> Option<u16> {
    if room < MIN_ROWS {
        return None;
    }
    let wanted = u16::try_from(tabs)
        .unwrap_or(u16::MAX)
        .saturating_add(BORDER_ROWS);
    Some(if room.saturating_sub(wanted) < MIN_ROWS {
        room
    } else {
        wanted
    })
}

/// Draws the panel into `area`, and records its scroll position in the mode.
///
/// Does nothing outside `Mode::Tabs`.
pub(super) fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let Mode::Tabs { selected, offset } = state.mode else {
        return;
    };
    let styles = theme::resolve(&state.config.theme);

    let tabs = state.tab_manager.len();
    let selected = selected.min(tabs.saturating_sub(1));
    let offset = scroll::clamp(
        offset,
        selected,
        tabs,
        usize::from(area.height.saturating_sub(BORDER_ROWS)),
        state.config.navigation.scroll_margin,
    );
    state.mode = Mode::Tabs { selected, offset };

    let label_width =
        usize::from(area.width.saturating_sub(BORDER_ROWS)).saturating_sub(HIGHLIGHT_SYMBOL.len());
    let active = state.tab_manager.active;
    let items: Vec<ListItem> = state
        .tab_dirs()
        .enumerate()
        .map(|(index, dir)| {
            let label = pathfmt::elide_front(&sanitize(&pathfmt::display(dir)), label_width);
            // Italic, so the tab you are in can still be found once the selector
            // has moved off it.
            let style = if index == active {
                styles.normal.add_modifier(Modifier::ITALIC)
            } else {
                styles.normal
            };
            ListItem::new(Line::from(Span::styled(label, style)))
        })
        .collect();

    let block = Block::default()
        .title(" Tabs ")
        .borders(Borders::ALL)
        .border_style(styles.border)
        .border_type(BorderType::Rounded);
    let list = List::new(items)
        .block(block)
        .highlight_style(styles.selection)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .highlight_spacing(HighlightSpacing::Always);
    let mut list_state = ListState::default()
        .with_offset(offset)
        .with_selected(Some(selected));

    frame.render_stateful_widget(list, area, &mut list_state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_is_one_row_per_tab_and_the_border() {
        assert_eq!(rows(1, 20), Some(3));
        assert_eq!(rows(3, 20), Some(5));
    }

    #[test]
    fn past_the_top_it_takes_everything_there_is() {
        assert_eq!(rows(50, 20), Some(20));
        assert_eq!(rows(usize::MAX, 20), Some(20));
    }

    /// Fewer than three rows would hold nothing but part of the listing's border,
    /// so they go to the panel instead.
    #[test]
    fn it_does_not_leave_a_sliver_of_listing() {
        assert_eq!(rows(16, 20), Some(20), "two rows left over");
        assert_eq!(rows(15, 20), Some(17), "three rows left over");
    }

    #[test]
    fn it_does_not_open_without_room_for_one_tab() {
        assert_eq!(rows(1, 2), None);
        assert_eq!(rows(1, 0), None);
        assert_eq!(rows(1, MIN_ROWS), Some(MIN_ROWS));
    }
}
