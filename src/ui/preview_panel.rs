//! Preview panel rendering.
//!
//! Renders the preview of the currently selected entry. Phase 1 added
//! synchronous directory/text preview. Phase 5 adds syntax highlighting
//! (`Highlighted`), binary metadata (`Binary`), and image metadata rendering.

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
/// - `Empty`       → a plain bordered panel with no text.
/// - `Loading`     → a "Loading…" italic placeholder.
/// - `Text(lines)` → numbered lines of plain text.
/// - `Highlighted` → syntax-highlighted lines from the worker.
/// - `Binary`      → metadata lines for binary or image files.
/// - `Directory`   → summary header and child entry names.
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

        PreviewContent::Highlighted(lines) => {
            // Each outer Vec<StyledSpan> is one source line.
            // We prepend a grey line-number span, then render each StyledSpan
            // with its assigned foreground colour.
            let text: Vec<Line> = lines
                .iter()
                .enumerate()
                .map(|(idx, spans)| {
                    let mut ratatui_spans = Vec::with_capacity(spans.len() + 1);

                    // Line number prefix (same style as plain-text path).
                    ratatui_spans.push(Span::styled(
                        format!("{:>4}  ", idx + 1),
                        Style::default().fg(Color::DarkGray),
                    ));

                    // Highlighted spans from syntect.
                    for s in spans {
                        let style = if let Some(fg) = s.fg {
                            Style::default().fg(fg)
                        } else {
                            Style::default()
                        };
                        ratatui_spans.push(Span::styled(s.text.clone(), style));
                    }

                    Line::from(ratatui_spans)
                })
                .collect();

            let p = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
            frame.render_widget(p, area);
        }

        PreviewContent::Binary(lines) => {
            // Binary metadata or image metadata lines — rendered as plain lines
            // with a subtle colour to distinguish them from text previews.
            let text: Vec<Line> = lines
                .iter()
                .map(|l| {
                    if l.is_empty() {
                        Line::from("")
                    } else if let Some((label, value)) = l.split_once(':') {
                        // "  Key  : value" → bold label, normal value.
                        Line::from(vec![
                            Span::styled(
                                format!("{label}:"),
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(value.to_owned()),
                        ])
                    } else {
                        Line::from(Span::styled(
                            l.as_str(),
                            Style::default().fg(Color::DarkGray),
                        ))
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
