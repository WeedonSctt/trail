//! Status bar rendering.
//!
//! Pure reflection of current state — no logic beyond formatting. Displays
//! `cwd`, mode label, active filter string, git branch, entry count,
//! Command Mode input buffer, notices, and delete confirmation.
//!
//! The bar is one row, and three different things want it: the path and
//! counters, a message, and the command line. They get it in that order of
//! precedence — the command line takes the whole row (it is what the user is
//! looking at), a message takes everything but the path (it is transient), and
//! otherwise the row shows path, filter and counters.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::actions::fs_ops::DeleteMode;
use crate::app::mode::Mode;
use crate::app::state::{AppState, NoticeLevel};
use crate::ui::theme;

/// Draws the status bar into `area`.
///
/// Layout adapts to what has to be said:
///
/// - **Command Mode**: the whole row is the command line, scrolled to keep the
///   insertion point visible, with the terminal cursor placed on it.
/// - **Notice or pending delete**: mode badge + path, then the message across
///   everything the path does not use, elided with `…` rather than cut.
/// - **Otherwise**: mode badge + path | filter query | entry count and branch.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let styles = theme::resolve(&state.config.theme);

    // Command Mode owns the row: a command is often longer than a third of the
    // screen, and the one thing the user needs to see is where their next
    // keystroke will land.
    if let Mode::Command { buffer, cursor, .. } = &state.mode {
        draw_command_line(frame, area, buffer, *cursor, styles.command);
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // left: mode + cwd
            Constraint::Percentage(30), // center: filter query
            Constraint::Percentage(20), // right: count / git branch
        ])
        .split(area);

    // ── Left section: mode badge + cwd ────────────────────────────────────────
    let mode_label = state.mode.label();
    let mode_style = match &state.mode {
        Mode::Navigation => Style::default()
            .fg(Color::Black)
            .bg(theme::parse_color(&state.config.theme.git_clean))
            .add_modifier(Modifier::BOLD),
        Mode::Search { .. } => Style::default()
            .fg(Color::Black)
            .bg(theme::parse_color(&state.config.theme.search))
            .add_modifier(Modifier::BOLD),
        Mode::Command { .. } => Style::default()
            .fg(Color::Black)
            .bg(theme::parse_color(&state.config.theme.command))
            .add_modifier(Modifier::BOLD),
    };

    let cwd_str = &state.status.cwd_display;

    let mut left_spans = vec![
        Span::styled(format!(" {mode_label} "), mode_style),
        Span::raw(" "),
    ];
    // Which tab is focused, but only once there is more than one: with a single
    // tab the answer is never in doubt and the path wants the room. Without
    // this, `Tab` and `Shift-Tab` changed the focused tab with nothing on screen
    // to say so, which reads as a key that does nothing.
    if !state.tab_manager.is_single() {
        left_spans.push(Span::styled(
            format!(
                "[{}/{}] ",
                state.tab_manager.active + 1,
                state.tab_manager.len()
            ),
            styles.command,
        ));
    }
    left_spans.push(Span::styled(cwd_str.as_str(), styles.normal));

    frame.render_widget(Paragraph::new(Line::from(left_spans)), sections[0]);

    // ── A message takes the rest of the row ───────────────────────────────────
    //
    // Errors name a path and a reason, and the delete prompt spells out both
    // keys that answer it; neither fits in 30% of a terminal. Giving a message
    // the counters' room as well costs nothing — it is gone on the next
    // keystroke — and what still does not fit is elided visibly instead of
    // being cut off mid-word.
    if let Some((text, style)) = message(state, &styles) {
        let rest = Rect {
            width: sections[1].width + sections[2].width,
            ..sections[1]
        };
        let elided = elide(&text, usize::from(rest.width));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(elided, style))),
            rest,
        );
        return;
    }

    // ── Center section: the filter query ──────────────────────────────────────
    if let Mode::Search { query, .. } = &state.mode {
        let center = Paragraph::new(Line::from(Span::styled(format!("/{query}"), styles.search)));
        frame.render_widget(center, sections[1]);
    }

    // ── Right section: entry count + git branch ───────────────────────────────
    let right_text = if let Some(ref git) = state.git {
        let dirty_marker = if git.is_dirty { " *" } else { "" };
        format!(
            "  {} {}{}  {} items ",
            "\u{e0a0}", git.branch, dirty_marker, state.status.entry_count
        )
    } else {
        format!("{} items ", state.status.entry_count)
    };
    let right = Paragraph::new(Line::from(Span::styled(right_text, styles.status)));
    frame.render_widget(right, sections[2]);
}

/// The message to show, with the style to show it in, or `None` when there is
/// nothing to say.
///
/// Priority: a pending delete is a question and outranks a notice, which is only
/// ever a report of something already done.
fn message(state: &AppState, styles: &theme::ThemeStyles) -> Option<(String, Style)> {
    if state.pending_delete {
        let name = state
            .selected_entry()
            .map(|e| e.file_name.as_str())
            .unwrap_or("selected entry");
        // The prompt names the outcome, not just the act: one of these can be
        // undone from the desktop's recycle bin and the other cannot, and `dd` on
        // a directory takes everything under it.
        let verb = match DeleteMode::parse(&state.config.general.delete_mode) {
            Some(DeleteMode::Permanent) => "Delete",
            _ => "Recycle",
        };
        return Some((
            format!(" {verb} '{name}'? [y/Enter=yes, n/Esc=cancel] "),
            Style::default()
                .fg(Color::Black)
                .bg(theme::parse_color(&state.config.theme.error))
                .add_modifier(Modifier::BOLD),
        ));
    }

    let notice = state.notice.as_ref()?;
    Some(match notice.level {
        // "Error:" belongs only on something that actually failed. A successful
        // bookmark used to arrive here through the error field and be announced
        // as one.
        NoticeLevel::Error => (format!(" Error: {} ", notice.text), styles.error),
        NoticeLevel::Info => (format!(" {} ", notice.text), styles.git_clean),
    })
}

/// Draws the command line across `area` and puts the terminal cursor on it.
///
/// `cursor` is a byte offset into `buffer`, as Command Mode tracks it. The line
/// scrolls horizontally so the insertion point is always on screen: a `:mv` with
/// a long destination runs past the width of any terminal, and before this the
/// tail of it was simply invisible.
///
/// Calling [`Frame::set_cursor_position`] is also what makes the cursor *appear*
/// — ratatui hides it on any frame that does not ask for it, which is why it is
/// absent in every other mode.
///
/// Columns are counted in characters rather than display width: a command line
/// is paths and flags, and the cost of being wrong about a double-width
/// character is a cursor one column out, not a corrupted line.
fn draw_command_line(frame: &mut Frame, area: Rect, buffer: &str, cursor: usize, style: Style) {
    // The buffer holds the text after the sentinel, except for `!`, which stays
    // in it — so the prompt is the `:` that was typed and is not stored.
    let prompt = if buffer.starts_with('!') { "" } else { ":" };
    let cursor_col = prompt.chars().count() + buffer[..cursor].chars().count();

    // Scroll only once the cursor would leave the row, and keep the last column
    // free so the cursor itself has somewhere to sit at the end of the line.
    let width = usize::from(area.width);
    let offset = cursor_col.saturating_sub(width.saturating_sub(1));

    let full: String = format!("{prompt}{buffer}");
    let visible: String = full.chars().skip(offset).take(width).collect();

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(visible, style))),
        area,
    );

    // `offset` is chosen so this cannot exceed the row.
    let column = area.x.saturating_add((cursor_col - offset) as u16);
    frame.set_cursor_position((column, area.y));
}

/// Fits `text` into `width` columns, marking anything dropped with `…`.
///
/// A silent cut is the failure this replaces: an error naming a path and a
/// reason looked, at 30% of a terminal, like an error naming half a path.
fn elide(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elide_leaves_text_that_fits_alone() {
        assert_eq!(elide("12 items", 20), "12 items");
        assert_eq!(elide("12 items", 8), "12 items");
    }

    #[test]
    fn elide_marks_what_it_drops() {
        assert_eq!(elide("destination already exists", 10), "destinati…");
        assert_eq!(elide("abc", 1), "…");
    }

    #[test]
    fn elide_of_zero_width_is_empty() {
        assert_eq!(elide("anything", 0), "");
    }
}
