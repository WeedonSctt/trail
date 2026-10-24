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
use crate::preview::provider::{line_number_gutter, PreviewContent};
use crate::ui::theme;

/// Draws the preview panel into `area`.
///
/// Renders the content from `state.preview.content`:
/// - `Empty`       → a plain bordered panel with no text (nothing selected,
///   or a file that could not be read).
/// - `Loading`     → a "Loading…" italic placeholder.
/// - `Text(lines)` → numbered lines of plain text; `(empty file)` for none.
/// - `Highlighted` → syntax-highlighted lines from the worker.
/// - `Binary`      → metadata lines for binary or image files.
/// - `Image`       → the decoded image plus a one-line caption.
/// - `Directory`   → summary header and child entry names.
///
/// Line-based content is scrolled by slicing: only the rows the pane can show
/// are turned into `Line`s, starting at `state.preview.scroll`. The offset is
/// therefore a logical line rather than a wrapped row — and it keeps per-frame
/// work proportional to the pane, not to the length of the file. A long line
/// still wraps onto several rows; the pane's width is recorded alongside its
/// height so the scroll limit and the footer can count those rows.
///
/// Takes `state` mutably because `Image` re-encodes itself when the pane
/// changes size, and because the pane's size is recorded here for the scroll
/// actions to clamp against.
pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let styles = theme::resolve(&state.config.theme);
    let title = if let Some(entry) = state.selected_entry() {
        format!(" {} ", entry.file_name)
    } else {
        " Preview ".to_owned()
    };

    let block = Block::default()
        .title(title.clone())
        .borders(Borders::ALL)
        .border_style(styles.border)
        .border_type(BorderType::Rounded);

    // Trail's version, on the far end of the same border row, when
    // `[general] show_version` asks for it. This is the frame's last unoccupied
    // surface: the navigation panel's border carries the directory name and the
    // sort badge, and the status bar's right section already overflows at 80
    // columns. A dev build takes the uncommitted-changes colour, so a build
    // under test stands out from a release before the label is even read.
    let badge_style = if crate::version::is_release() {
        styles.status
    } else {
        styles.git_dirty
    };
    let block = match version_badge(state, title.chars().count(), area.width) {
        Some(badge) => block
            .title_top(Line::from(Span::styled(format!(" {badge} "), badge_style)).right_aligned()),
        None => block,
    };

    // Resolve the scroll offset before borrowing `content` below: the pane
    // height is only known here, and the clamp needs the whole slot.
    let inner = block.inner(area);
    let height = usize::from(inner.height);
    state.preview.viewport_height = height;
    state.preview.viewport_width = inner.width;
    state.preview.clamp_scroll();
    let offset = state.preview.scroll;
    let footer = scroll_footer(&state.preview);
    let footer_width = footer.as_ref().map_or(0, |f| f.chars().count());
    let block = match footer {
        Some(footer) => {
            block.title_bottom(Line::from(Span::styled(footer, styles.status)).right_aligned())
        }
        None => block,
    };
    // A preview cut short by `[preview] max_lines` or the byte ceiling says so
    // in words on the bottom border, where it is visible at any scroll position
    // — the footer's `+` alone is easy to miss, and a pane that stops is
    // otherwise indistinguishable from a file that ends.
    let limits = (
        state.config.preview.max_lines,
        state.config.general.text_sync_threshold_kb,
    );
    let block = match truncation_notice(&state.preview, limits, footer_width, area.width) {
        Some(notice) => {
            block.title_bottom(Line::from(Span::styled(notice, styles.git_dirty)).left_aligned())
        }
        None => block,
    };
    let placeholder = Style::default()
        .fg(theme::parse_color(&state.config.theme.hidden))
        .add_modifier(Modifier::ITALIC);

    match &mut state.preview.content {
        PreviewContent::Empty => {
            frame.render_widget(block, area);
        }

        PreviewContent::Loading => {
            let p = Paragraph::new("Loading…").style(placeholder).block(block);
            frame.render_widget(p, area);
        }

        // A file with nothing in it says so, rather than drawing the same blank
        // pane as a file that could not be shown.
        PreviewContent::Text(lines) if lines.is_empty() => {
            frame.render_widget(
                Paragraph::new(EMPTY_FILE).style(placeholder).block(block),
                area,
            );
        }
        PreviewContent::Highlighted(lines) if lines.is_empty() => {
            frame.render_widget(
                Paragraph::new(EMPTY_FILE).style(placeholder).block(block),
                area,
            );
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
                    ratatui_spans.push(Span::styled(line_number_gutter(idx + 1), styles.status));

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

/// Trail's version as the top border should show it, or `None` when it is
/// switched off or there is no room for it beside the file name.
///
/// The string is [`crate::version::VERSION`], the compile-time label that also
/// leads `--version`, so a dev build reads as one (`v1.10.1-dev.3+f7863b6`)
/// rather than passing for the release it descends from. Nothing is read,
/// computed or stored per frame.
///
/// Budgeted like the navigation panel's sort badge: the file name is what the
/// pane is *about*, so when the two cannot share the border the version stands
/// down. ratatui clips overlapping titles rather than reflowing them, so
/// without this a long name and the badge would overwrite each other.
fn version_badge(state: &AppState, title_width: usize, area_width: u16) -> Option<String> {
    if !state.config.general.show_version {
        return None;
    }
    let badge = format!("v{}", crate::version::VERSION);
    // The row less its two border columns, then: the name, the badge, a space
    // each side of the badge, and one column between the two so they never
    // touch.
    let inner = usize::from(area_width).saturating_sub(2);
    (title_width + badge.chars().count() + 3 <= inner).then_some(badge)
}

/// What the pane shows for a file with no content.
const EMPTY_FILE: &str = "(empty file)";

/// The bottom border's notice that the preview stops before the file does, or
/// `None` when the whole file is loaded.
///
/// Names which limit cut it and the key that raises it, so the reader knows the
/// rest exists and how to see more of it. `limits` is `([preview] max_lines,
/// [general] text_sync_threshold_kb)`: a preview holding exactly `max_lines`
/// lines stopped on the line cap, anything shorter on the byte ceiling.
/// Shortened, then dropped, when the border cannot hold it beside the position
/// footer — the footer's `+` still marks the truncation then.
fn truncation_notice(
    slot: &PreviewSlot,
    (max_lines, max_kb): (usize, usize),
    footer_width: usize,
    area_width: u16,
) -> Option<String> {
    if !slot.truncated {
        return None;
    }
    let loaded = slot.content.scrollable_len()?;
    let (limit, key) = if loaded >= max_lines {
        (format!("first {loaded} lines"), "max_lines")
    } else {
        (format!("first {max_kb} KB"), "text_sync_threshold_kb")
    };
    // The row less its two border columns and one column between the two
    // titles so they never touch.
    let room = usize::from(area_width).saturating_sub(2 + footer_width + 1);
    [
        format!(" preview limit: {limit} · {key} "),
        format!(" preview limit: {limit} "),
        " preview limit ".to_owned(),
    ]
    .into_iter()
    .find(|notice| notice.chars().count() <= room)
}

/// Builds the pane's bottom-right position indicator, or `None` when the
/// content fits and nothing was left unread.
///
/// The form is `first–last/total`, with a trailing `+` when the file continues
/// past the loaded window (see [`PreviewSlot::truncated`]) — so a preview cut
/// by `[preview] max_lines` says so instead of pretending to be the whole file.
/// `last` is the last line drawn whole, so wrapped lines shrink the range
/// rather than it overstating what is on screen.
fn scroll_footer(slot: &PreviewSlot) -> Option<String> {
    let total = slot.content.scrollable_len()?;
    if slot.scroll == 0 && slot.max_scroll() == 0 && !slot.truncated {
        return None;
    }
    let first = slot.scroll + 1;
    let last = (slot.scroll + slot.lines_visible_from(slot.scroll)).min(total);
    let more = if slot.truncated { "+" } else { "" };
    Some(format!(" {first}–{last}/{total}{more} "))
}
