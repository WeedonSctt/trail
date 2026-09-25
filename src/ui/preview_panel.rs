//! Preview panel rendering.
//!
//! Renders the preview of the currently selected entry: syntax-highlighted
//! text, binary or image metadata, a directory summary, or — for image files
//! in a capable terminal — the image itself, drawn through the inline-image
//! protocol resolved by `preview::graphics`.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;
use ratatui_image::StatefulImage;

use crate::app::state::{AppState, PreviewSlot};
use crate::preview::provider::PreviewContent;
use crate::ui::theme;

/// Draws the preview panel into `area`.
///
/// Renders the content from `state.preview.content`:
/// - `Empty`       → a plain bordered panel with no text.
/// - `Loading`     → a "Loading…" italic placeholder.
/// - `Text(lines)` → numbered lines of plain text.
/// - `Highlighted` → syntax-highlighted lines from the worker.
/// - `Binary`      → metadata lines for binary or image files.
/// - `Image`       → the decoded image plus a one-line caption.
/// - `Directory`   → summary header and child entry names.
///
/// Line-based content is scrolled by slicing: only the rows the pane can show
/// are turned into `Line`s, starting at `state.preview.scroll`. The offset is
/// therefore a logical line rather than a wrapped row, which is what makes it
/// exactly clampable — and it keeps per-frame work proportional to the pane,
/// not to the length of the file.
///
/// Takes `state` mutably because `Image` re-encodes itself when the pane
/// changes size, and because the pane's height is recorded here for the scroll
/// actions to clamp against.
pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let styles = theme::resolve(&state.config.theme);
    let title = if let Some(entry) = state.selected_entry() {
        format!(" {} ", entry.file_name)
    } else {
        " Preview ".to_owned()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(styles.border)
        .border_type(BorderType::Rounded);

    // Resolve the scroll offset before borrowing `content` below: the pane
    // height is only known here, and the clamp needs the whole slot.
    let height = usize::from(block.inner(area).height);
    state.preview.viewport_height = height;
    state.preview.clamp_scroll();
    let offset = state.preview.scroll;
    let block = match scroll_footer(&state.preview) {
        Some(footer) => {
            block.title_bottom(Line::from(Span::styled(footer, styles.status)).right_aligned())
        }
        None => block,
    };

    match &mut state.preview.content {
        PreviewContent::Empty => {
            frame.render_widget(block, area);
        }

        PreviewContent::Loading => {
            let p = Paragraph::new("Loading…")
                .style(
                    Style::default()
                        .fg(theme::parse_color(&state.config.theme.hidden))
                        .add_modifier(Modifier::ITALIC),
                )
                .block(block);
            frame.render_widget(p, area);
        }

        PreviewContent::Text(lines) => {
            let text: Vec<Line> = lines
                .iter()
                .skip(offset)
                .take(height)
                .map(|l| {
                    // Split into line-number prefix and content for styling.
                    if let Some((num_part, rest)) = l.split_once("  ") {
                        Line::from(vec![
                            Span::styled(format!("{num_part}  "), styles.status),
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
                .skip(offset)
                .take(height)
                .map(|(idx, spans)| {
                    let mut ratatui_spans = Vec::with_capacity(spans.len() + 1);

                    // Line number prefix (same style as plain-text path).
                    // `enumerate` runs before `skip`, so the number is the line's
                    // position in the file, not in the visible slice.
                    ratatui_spans.push(Span::styled(format!("{:>4}  ", idx + 1), styles.status));

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
                .skip(offset)
                .take(height)
                .map(|l| {
                    if l.is_empty() {
                        Line::from("")
                    } else if let Some((label, value)) = l.split_once(':') {
                        // "  Key  : value" → bold label, normal value.
                        Line::from(vec![
                            Span::styled(
                                format!("{label}:"),
                                styles.command.add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(value.to_owned()),
                        ])
                    } else {
                        Line::from(Span::styled(l.as_str(), styles.status))
                    }
                })
                .collect();

            let p = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
            frame.render_widget(p, area);
        }

        PreviewContent::Image(preview) => {
            // The image is drawn into the block's interior, reserving the last
            // row for the caption. `render_stateful_widget` is what triggers
            // the resize-and-encode inside ratatui-image, which is why this
            // arm needs `&mut`.
            let inner = block.inner(area);
            frame.render_widget(block, area);

            if inner.height == 0 || inner.width == 0 {
                return;
            }

            let (image_area, caption_area) = if inner.height > 1 {
                (
                    Rect {
                        height: inner.height - 1,
                        ..inner
                    },
                    Some(Rect {
                        y: inner.y + inner.height - 1,
                        height: 1,
                        ..inner
                    }),
                )
            } else {
                (inner, None)
            };

            frame.render_stateful_widget(
                StatefulImage::new(None),
                image_area,
                &mut preview.protocol,
            );

            if let Some(caption_area) = caption_area {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        preview.caption.as_str(),
                        styles.status,
                    ))),
                    caption_area,
                );
            }
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
                Span::styled(format!("{dir_count}"), styles.directory),
                Span::raw(" dirs, "),
                Span::styled(format!("{file_count}"), styles.git_clean),
                Span::raw(" files"),
                if *hidden_count > 0 {
                    Span::styled(format!(", {hidden_count} hidden"), styles.status)
                } else {
                    Span::raw("")
                },
            ]));
            lines.push(Line::from(""));

            for name in entries {
                let style = if name.ends_with('/') {
                    styles.directory
                } else {
                    Style::default()
                };
                lines.push(Line::from(Span::styled(format!("  {name}"), style)));
            }

            // The summary header scrolls with the list rather than staying
            // pinned, so the offset applies to the assembled lines.
            let visible: Vec<Line> = lines.into_iter().skip(offset).take(height).collect();

            let p = Paragraph::new(visible).block(block);
            frame.render_widget(p, area);
        }
    }
}

/// Builds the pane's bottom-right position indicator, or `None` when the
/// content fits and nothing was left unread.
///
/// The form is `first–last/total`, with a trailing `+` when the file continues
/// past the loaded window (see [`PreviewSlot::truncated`]) — so a preview cut
/// by `[preview] max_lines` says so instead of pretending to be the whole file.
fn scroll_footer(slot: &PreviewSlot) -> Option<String> {
    let total = slot.content.scrollable_len()?;
    if total <= slot.viewport_height && !slot.truncated {
        return None;
    }
    let first = slot.scroll + 1;
    let last = (slot.scroll + slot.viewport_height).min(total);
    let more = if slot.truncated { "+" } else { "" };
    Some(format!(" {first}–{last}/{total}{more} "))
}
