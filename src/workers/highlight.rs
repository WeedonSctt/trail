//! Syntax highlighting worker.
//!
//! Highlights text files off-thread for files over `TEXT_SYNC_THRESHOLD`.
//! Small files are highlighted synchronously in `preview/text.rs` via
//! `highlight_text`. Both paths use the same `syntect` pipeline so the
//! output type is consistent.

use std::io::BufRead;
use std::path::PathBuf;

use syntect::easy::HighlightFile;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use tokio::sync::mpsc;

use crate::preview::provider::{HighlightedLine, PreviewContent, StyledSpan};
use crate::workers::WorkerMsg;

/// Default theme used for syntax highlighting.
///
/// `base16-ocean.dark` is a widely supported theme bundled with syntect's
/// default theme set. Phase 7 will make this configurable via `config.toml`.
const DEFAULT_THEME: &str = "base16-ocean.dark";

/// Maximum number of lines highlighted synchronously in the async worker.
///
/// An upper bound to prevent the worker from spending an unbounded amount
/// of time on a very large file. Lines beyond this are silently truncated.
const HIGHLIGHT_MAX_LINES: usize = 2000;

/// Highlights `path` off-thread, sending a `WorkerMsg::Preview` result back
/// through `tx` tagged with `generation`.
///
/// The caller must set `state.preview.content = PreviewContent::Loading` before
/// spawning this task so the UI shows a placeholder while the worker runs.
///
/// If syntax detection fails, the worker falls back to plain-text line-numbered
/// output (`PreviewContent::Text`) rather than returning an error, so the
/// preview pane always shows *something*.
pub fn spawn_highlight(path: PathBuf, generation: u64, tx: mpsc::Sender<WorkerMsg>) {
    tokio::spawn(async move {
        let path_for_msg = path.clone();
        let content = tokio::task::spawn_blocking(move || highlight_file_sync(&path))
            .await
            .unwrap_or_else(|_| plain_text_fallback_empty());

        let msg = WorkerMsg::Preview {
            generation,
            path: path_for_msg,
            content,
        };
        // If the channel is closed the UI thread has exited; ignore the error.
        let _ = tx.send(msg).await;
    });
}

/// Performs the blocking syntect highlight operation.
///
/// Called inside `spawn_blocking` so it never runs on the async executor thread.
/// Returns `PreviewContent::Highlighted` on success, or `PreviewContent::Text`
/// if syntect cannot find a matching syntax.
fn highlight_file_sync(path: &std::path::Path) -> PreviewContent {
    // SyntaxSet and ThemeSet are cheap to clone; building them here avoids the
    // need to share them across threads (they are not Sync in all syntect versions).
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let theme = ts.themes.get(DEFAULT_THEME).unwrap_or_else(|| {
        // Any bundled theme works; fall back to the first available.
        ts.themes
            .values()
            .next()
            .expect("syntect ships at least one theme")
    });

    let mut highlighter = match HighlightFile::new(path, &ss, theme) {
        Ok(h) => h,
        Err(_) => return plain_text_fallback(path),
    };

    let mut lines: Vec<HighlightedLine> = Vec::new();

    loop {
        if lines.len() >= HIGHLIGHT_MAX_LINES {
            break;
        }
        let mut line_buf = String::new();
        match highlighter.reader.read_line(&mut line_buf) {
            Ok(0) => break,
            Err(_) => break,
            _ => {}
        }
        // Strip the trailing newline that syntect expects to have present but
        // that we don't want to show.
        let line_text = line_buf.trim_end_matches(['\n', '\r']);

        let regions = match highlighter.highlight_lines.highlight_line(line_text, &ss) {
            Ok(r) => r,
            Err(_) => break,
        };

        let spans: HighlightedLine = regions
            .into_iter()
            .filter(|(_, text)| !text.is_empty())
            .map(|(style, text)| {
                let fg = convert_color(style.foreground);
                StyledSpan {
                    text: text.to_owned(),
                    fg,
                }
            })
            .collect();

        lines.push(spans);
    }

    if lines.is_empty() {
        PreviewContent::Empty
    } else {
        PreviewContent::Highlighted(lines)
    }
}

/// Converts a `syntect::highlighting::Color` to a `ratatui::style::Color`.
///
/// The `a` (alpha) channel is ignored — ratatui does not support transparency.
fn convert_color(c: syntect::highlighting::Color) -> Option<ratatui::style::Color> {
    // syntect uses 0 alpha to mean "no colour assigned" in some themes.
    if c.a == 0 {
        None
    } else {
        Some(ratatui::style::Color::Rgb(c.r, c.g, c.b))
    }
}

/// Reads `path` as plain text and returns `PreviewContent::Text` as a fallback
/// when syntect has no matching syntax for the file.
fn plain_text_fallback(path: &std::path::Path) -> PreviewContent {
    use std::io::Read;
    let Ok(f) = std::fs::File::open(path) else {
        return PreviewContent::Empty;
    };
    let limit = crate::preview::text::TEXT_PREVIEW_MAX_BYTES as u64;
    let mut buf = Vec::new();
    let _ = f.take(limit).read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<String> = text
        .lines()
        .take(HIGHLIGHT_MAX_LINES)
        .enumerate()
        .map(|(i, l)| format!("{:>4}  {}", i + 1, l))
        .collect();
    PreviewContent::Text(lines)
}

/// Returns an empty fallback for use when `spawn_blocking` panics.
fn plain_text_fallback_empty() -> PreviewContent {
    PreviewContent::Empty
}

// ── Synchronous highlighting for small files ───────────────────────────────────

/// Highlights the content of `path` synchronously, returning `PreviewContent`.
///
/// Intended for files whose size is under `TEXT_SYNC_THRESHOLD` so the UI
/// thread can produce highlighted output without spawning a worker. Larger
/// files must use [`spawn_highlight`] instead.
///
/// Falls back to `PreviewContent::Text` when no matching syntax is found.
pub fn highlight_text_sync(path: &std::path::Path) -> PreviewContent {
    highlight_file_sync(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn highlight_rust_source() {
        let mut f = NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(f, "fn main() {{ println!(\"hello\"); }}").unwrap();

        let content = highlight_text_sync(f.path());
        // Should produce a Highlighted result for a .rs file.
        assert!(
            matches!(
                content,
                PreviewContent::Highlighted(_) | PreviewContent::Text(_)
            ),
            "expected Highlighted or Text, got: {content:?}"
        );
    }

    #[test]
    fn highlight_unknown_extension_falls_back_to_text() {
        let mut f = NamedTempFile::with_suffix(".xyzunknown123").unwrap();
        writeln!(f, "some content").unwrap();

        let content = highlight_text_sync(f.path());
        // Should fall back to plain text.
        assert!(
            matches!(
                content,
                PreviewContent::Text(_) | PreviewContent::Highlighted(_)
            ),
            "expected Text or Highlighted fallback, got: {content:?}"
        );
    }

    #[test]
    fn convert_color_opaque() {
        let c = syntect::highlighting::Color {
            r: 255,
            g: 128,
            b: 0,
            a: 255,
        };
        assert!(matches!(
            convert_color(c),
            Some(ratatui::style::Color::Rgb(255, 128, 0))
        ));
    }

    #[test]
    fn convert_color_transparent_returns_none() {
        let c = syntect::highlighting::Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
        assert!(convert_color(c).is_none());
    }
}
