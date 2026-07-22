//! Preview panel rendering.
//!
//! Renders the preview of the currently selected entry. Phase 1 adds
//! synchronous directory/text preview. Phase 5 adds syntax highlighting,
//! binary metadata, and image preview.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::state::AppState;
use crate::preview::provider::PreviewContent;

/// Draws the preview panel into `area`.
///
/// Renders the content from `state.preview.content`:
/// - `Empty` → a plain bordered panel with no text.
/// - `Loading` → a "Loading…" placeholder (Phase 4/5 async path).
/// - `Text(lines)` → numbered lines of plain text.
/// - `Directory { .. }` → summary header and child entry names.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let title = if let Some(entry) = state.selected_entry() {
        format!(" {} ", entry.file_name)
    } else {
        " Preview ".to_owned()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    match &state.preview.content {
        PreviewContent::Empty => {
            frame.render_widget(block, area);
        }

        PreviewContent::Loading => {
            let p = Paragraph::new("Loading…")
                .style(
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )
                .block(block);
            frame.render_widget(p, area);
        }

        PreviewContent::Text(lines) => {
            let text: Vec<Line> = lines
                .iter()
                .map(|l| {
                    // Split into line-number prefix and content for styling.
                    if let Some((num_part, rest)) = l.split_once("  ") {
                        Line::from(vec![
                            Span::styled(
                                format!("{num_part}  "),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::raw(rest.to_owned()),
                        ])
                    } else {
                        Line::from(Span::raw(l.as_str()))
                    }
                })
                .collect();

            let p = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
            frame.render_widget(p, area);
        }

        PreviewContent::Directory {
            file_count,
            dir_count,
            hidden_count,
            entries,
        } => {
            let mut lines: Vec<Line> = Vec::with_capacity(entries.len() + 4);

            // Summary header.
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{dir_count}"),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" dirs, "),
                Span::styled(format!("{file_count}"), Style::default().fg(Color::Green)),
                Span::raw(" files"),
                if *hidden_count > 0 {
                    Span::styled(
                        format!(", {hidden_count} hidden"),
                        Style::default().fg(Color::DarkGray),
                    )
                } else {
                    Span::raw("")
                },
            ]));
            lines.push(Line::from(""));

            for name in entries {
                let style = if name.ends_with('/') {
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(Line::from(Span::styled(format!("  {name}"), style)));
            }

            let p = Paragraph::new(lines).block(block);
            frame.render_widget(p, area);
        }
    }
}
