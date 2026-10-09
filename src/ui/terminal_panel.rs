//! Terminal panel rendering: the tab strip, and the active shell's screen.
//!
//! The screen is copied cell by cell out of the session's `vt100` grid into
//! ratatui's buffer — colours, bold, inverse and the rest — rather than through
//! a widget crate, so the panel needs no second copy of ratatui (see
//! `CLAUDE.md` §9). The session's lock is held only for that copy.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};
use ratatui::Frame;

use crate::app::state::AppState;
use crate::ui::theme;

/// Draws the panel into `area`, and resizes the shells to fit it.
///
/// Places the terminal cursor on the shell's cursor while the shell has the
/// keyboard, since that is where the next keystroke lands; ratatui hides the
/// cursor on any frame that does not ask for it.
pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let styles = theme::resolve(&state.config.theme);
    let panel = &mut state.terminal;
    let focused = panel.shell_focused();

    let mut title = vec![Span::styled(" ", styles.border)];
    for (index, session) in panel.sessions().iter().enumerate() {
        if index > 0 {
            title.push(Span::styled(" │ ", styles.border));
        }
        let label = format!("{}:{}", index + 1, session.label());
        let style = if index == panel.active() {
            styles
                .command
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            styles.normal
        };
        title.push(Span::styled(label, style));
    }
    title.push(Span::styled(" ", styles.border));

    // Which side has the keyboard is said by the status bar's `TERMINAL` badge.
    // A highlighted border for it is deferred (spec §11, L4).
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(styles.border)
        .title(Line::from(title));

    let inner = block.inner(area);
    panel.resize_all((inner.height, inner.width));

    let Some(session) = panel.sessions().get(panel.active()) else {
        frame.render_widget(block, area);
        return;
    };
    let parser = session.screen();
    let screen = parser.screen();

    let scrolled = screen.scrollback();
    if scrolled > 0 {
        block = block.title_bottom(Line::from(Span::styled(
            format!(" ↑ {scrolled} lines — type to return "),
            styles.command,
        )));
    }
    frame.render_widget(block, area);

    copy_screen(screen, inner, frame.buffer_mut());

    if focused && scrolled == 0 && !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        if row < inner.height && col < inner.width {
            frame.set_cursor_position((inner.x + col, inner.y + row));
        }
    }
}

/// Copies the visible part of `screen` into `area` of `buf`.
fn copy_screen(screen: &vt100::Screen, area: Rect, buf: &mut Buffer) {
    for row in 0..area.height {
        for col in 0..area.width {
            let Some(target) = buf.cell_mut((area.x + col, area.y + row)) else {
                continue;
            };
            let Some(cell) = screen.cell(row, col) else {
                target.reset();
                continue;
            };
            if cell.is_wide_continuation() {
                // The wide character to the left covers this column; ratatui
                // skips it when diffing, as long as it holds no text of its own.
                target.set_symbol(" ");
                continue;
            }
            let contents = cell.contents();
            target.set_symbol(if contents.is_empty() { " " } else { contents });
            target.set_style(style_of(cell));
        }
    }
}

fn style_of(cell: &vt100::Cell) -> Style {
    let mut modifiers = Modifier::empty();
    if cell.bold() {
        modifiers |= Modifier::BOLD;
    }
    if cell.dim() {
        modifiers |= Modifier::DIM;
    }
    if cell.italic() {
        modifiers |= Modifier::ITALIC;
    }
    if cell.underline() {
        modifiers |= Modifier::UNDERLINED;
    }
    if cell.inverse() {
        modifiers |= Modifier::REVERSED;
    }
    Style::default()
        .fg(color_of(cell.fgcolor()))
        .bg(color_of(cell.bgcolor()))
        .add_modifier(modifiers)
}

fn color_of(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_screen_is_copied_with_its_colours() {
        let mut parser = vt100::Parser::new(3, 10, 0);
        parser.process(b"hi \x1b[31;1mred\x1b[0m");
        let area = Rect::new(1, 1, 10, 3);
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 5));
        copy_screen(parser.screen(), area, &mut buf);

        let row: String = (1..11)
            .map(|x| buf.cell((x, 1)).map_or(" ", |c| c.symbol()).to_owned())
            .collect();
        assert_eq!(row, "hi red    ");
        let r = buf.cell((4, 1)).unwrap();
        assert_eq!(r.fg, Color::Indexed(1));
        assert!(r.modifier.contains(Modifier::BOLD));
        assert_eq!(buf.cell((1, 1)).unwrap().fg, Color::Reset);
    }
}
