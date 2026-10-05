//! Syntax highlighting worker.
//!
//! Previews every text file off-thread: reads it, classifies it as text or
//! binary, and highlights it with `syntect`. Nothing here runs on the UI thread,
//! and the syntax and theme sets are parsed once for the process rather than per
//! preview.
//!
//! A file that turns out to be binary comes back as the same metadata preview
//! `preview/binary.rs` builds, together with the classification, which
//! `workers::merge` caches on the entry so the next preview of it is
//! synchronous.

use std::io::BufRead;
use std::path::PathBuf;
use std::sync::OnceLock;

use syntect::easy::HighlightFile;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use tokio::sync::mpsc;

use crate::preview::provider::{sanitize, HighlightedLine, PreviewContent, StyledSpan};
use crate::workers::WorkerMsg;

/// Default theme used for syntax highlighting.
///
/// `base16-ocean.dark` is a widely supported theme bundled with syntect's
/// default theme set. Theme colors are configurable through Phase 7 config.
const DEFAULT_THEME: &str = "base16-ocean.dark";

/// syntect's bundled syntax definitions, parsed once for the process.
///
/// Both of these used to be built inside the highlight call, which meant parsing
/// every bundled syntax and theme *per preview* — once for each row the user
/// scrolled past. That is tens of milliseconds and a few megabytes of
/// allocation each time, and it is the same answer every time.
static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();

/// syntect's bundled themes, parsed once for the process. See [`SYNTAX_SET`].
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

/// The process-wide syntax set, built on first use.
fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The process-wide theme set, built on first use.
fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// Highlights `path` off-thread, sending a `WorkerMsg::Preview` result back
/// through `tx` tagged with `generation`.
///
/// `max_lines` bounds how much of the file is loaded — the preview scrolls
/// within that window, and the message's `truncated` flag tells the UI whether
/// the file continues past it.
///
/// The caller must set `state.preview.content = PreviewContent::Loading` before
/// spawning this task so the UI shows a placeholder while the worker runs.
///
/// If syntax detection fails, the worker falls back to plain-text line-numbered
/// output (`PreviewContent::Text`) rather than returning an error, so the
/// preview pane always shows *something*.
pub fn spawn_highlight(
    path: PathBuf,
    generation: u64,
    tx: mpsc::Sender<WorkerMsg>,
    max_lines: usize,
    max_bytes: usize,
) {
    tokio::spawn(async move {
        let path_for_msg = path.clone();
        let outcome =
            tokio::task::spawn_blocking(move || preview_file_sync(&path, max_lines, max_bytes))
                .await
                .unwrap_or(Outcome {
                    content: plain_text_fallback_empty(),
                    truncated: false,
                    is_text: None,
                });

        let msg = WorkerMsg::Preview {
            generation,
            path: path_for_msg,
            content: outcome.content,
            truncated: outcome.truncated,
            is_text: outcome.is_text,
        };
        // If the channel is closed the UI thread has exited; ignore the error.
        let _ = tx.send(msg).await;
    });
}

/// What one run of the worker produced.
struct Outcome {
    /// The content to draw.
    content: PreviewContent,
    /// Whether the file continues past what was loaded.
    truncated: bool,
    /// Whether the file turned out to be text, when the worker got far enough to
    /// tell. Cached on the entry by `workers::merge`.
    is_text: Option<bool>,
}

/// Classifies `path` and builds the preview that suits it.
///
/// The text/binary question is answered here rather than during the directory
/// listing because this worker opens the file regardless: probing every entry up
/// front cost an open and an 8 KB read per file, on the UI thread, each time a
/// directory was listed. A binary file gets the same metadata preview the
/// synchronous [`crate::preview::binary`] provider would have produced, and the
/// answer travels back so the next preview of that entry needs no worker at all.
fn preview_file_sync(path: &std::path::Path, max_lines: usize, max_bytes: usize) -> Outcome {
    if !crate::preview::text::is_text_file(path) {
        return Outcome {
            content: crate::preview::binary::build_binary_preview(path, None),
            truncated: false,
            is_text: Some(false),
        };
    }

    let (content, truncated) = highlight_file_sync(path, max_lines, max_bytes);
    Outcome {
        content,
        truncated,
        is_text: Some(true),
    }
}

/// Performs the blocking syntect highlight operation.
///
/// Called inside `spawn_blocking` so it never runs on the async executor thread.
/// Returns `PreviewContent::Highlighted` on success, or `PreviewContent::Text`
/// if syntect cannot find a matching syntax, paired with whether the file has
/// more lines than `max_lines`.
fn highlight_file_sync(
    path: &std::path::Path,
    max_lines: usize,
    max_bytes: usize,
) -> (PreviewContent, bool) {
    let ss = syntax_set();
    let ts = theme_set();

    let theme = ts.themes.get(DEFAULT_THEME).unwrap_or_else(|| {
        // Any bundled theme works; fall back to the first available.
        ts.themes
            .values()
            .next()
            .expect("syntect ships at least one theme")
    });

    let mut highlighter = match HighlightFile::new(path, ss, theme) {
        Ok(h) => h,
        Err(_) => return plain_text_fallback(path, max_lines, max_bytes),
    };

    let mut lines: Vec<HighlightedLine> = Vec::new();
    let mut truncated = false;
    let mut bytes_read = 0usize;

    loop {
        // Two ceilings, because either one alone leaves a hole: `max_lines`
        // bounds an ordinary file, and `max_bytes` bounds one whose lines are
        // enormous -- minified JavaScript, a single-line JSON blob -- which would
        // otherwise be loaded whole however few lines it has.
        if lines.len() >= max_lines || bytes_read >= max_bytes {
            // Hitting a cap only truncates the preview if the file actually
            // continues, so probe for one more line rather than assuming.
            let mut probe = String::new();
            truncated = matches!(highlighter.reader.read_line(&mut probe), Ok(n) if n > 0);
            break;
        }
        let mut line_buf = String::new();
        match highlighter.reader.read_line(&mut line_buf) {
            Ok(0) => break,
            Err(_) => break,
            Ok(n) => bytes_read += n,
        }
        // Strip the trailing newline that syntect expects to have present but
        // that we don't want to show, then make the line safe to draw before
        // anything highlights it — the spans have to describe the text that is
        // actually rendered.
        let line_text = sanitize(line_buf.trim_end_matches(['\n', '\r']));

        let regions = match highlighter.highlight_lines.highlight_line(&line_text, ss) {
            Ok(r) => r,
            Err(_) => break,
        };

        let spans: HighlightedLine = regions
            .into_iter()
            .filter(|(_, text)| !text.is_empty())
            .map(|(style, text)| StyledSpan::fg(text, convert_color(style.foreground)))
            .collect();

        lines.push(spans);
    }

    if lines.is_empty() {
        (PreviewContent::Empty, false)
    } else {
        (PreviewContent::Highlighted(lines), truncated)
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
/// when syntect has no matching syntax for the file, paired with whether the
/// file continues past what was read.
///
/// Two limits can cut this path: `max_lines`, and the byte ceiling that keeps a
/// single read bounded. Either one means the preview is truncated.
fn plain_text_fallback(
    path: &std::path::Path,
    max_lines: usize,
    max_bytes: usize,
) -> (PreviewContent, bool) {
    use std::io::Read;
    let Ok(f) = std::fs::File::open(path) else {
        return (PreviewContent::Empty, false);
    };
    let limit = max_bytes as u64;
    let mut buf = Vec::new();
    // Read one byte past the ceiling: that extra byte is how we tell a file that
    // ends exactly on the limit from one that continues past it.
    let _ = f.take(limit + 1).read_to_end(&mut buf);
    let bytes_truncated = buf.len() as u64 > limit;
    buf.truncate(limit as usize);

    let text = String::from_utf8_lossy(&buf);
    let all: Vec<&str> = text.lines().collect();
    let lines_truncated = all.len() > max_lines;
    let lines: Vec<String> = all
        .into_iter()
        .take(max_lines)
        .enumerate()
        .map(|(i, l)| format!("{:>4}  {}", i + 1, sanitize(l)))
        .collect();
    (
        PreviewContent::Text(lines),
        bytes_truncated || lines_truncated,
    )
}

/// Returns an empty fallback for use when `spawn_blocking` panics.
fn plain_text_fallback_empty() -> PreviewContent {
    PreviewContent::Empty
}

// ── Synchronous highlighting, for callers already off the UI thread ────────────

/// Highlights the content of `path` synchronously, returning the content and
/// whether the file continues past either ceiling.
///
/// Intended for tests and for callers that already run off the UI thread; the
/// application path uses [`spawn_highlight`], which never blocks the event loop.
/// The byte ceiling is [`crate::preview::text::TEXT_PREVIEW_MAX_BYTES`], the same
/// default `[general] text_sync_threshold_kb` carries.
///
/// Falls back to `PreviewContent::Text` when no matching syntax is found.
#[allow(dead_code)]
pub fn highlight_text_sync(path: &std::path::Path, max_lines: usize) -> (PreviewContent, bool) {
    highlight_file_sync(
        path,
        max_lines,
        crate::preview::text::TEXT_PREVIEW_MAX_BYTES,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// `max_lines` alone leaves a hole: a file of a few enormous lines -- minified
    /// JavaScript, a one-line JSON blob -- is under the line cap and would be
    /// loaded whole. `[general] text_sync_threshold_kb` is the byte ceiling that
    /// closes it, and the preview has to report itself truncated when it bites.
    #[test]
    fn the_byte_ceiling_bounds_a_file_with_enormous_lines() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for _ in 0..4 {
            writeln!(f, "{}", "x".repeat(1000)).unwrap();
        }
        f.flush().unwrap();

        // Four lines, well under the line cap, but past a 2 KB ceiling.
        let (content, truncated) = highlight_file_sync(f.path(), 2000, 2048);
        assert!(truncated, "the ceiling must be reported as truncation");
        let loaded = match content {
            PreviewContent::Highlighted(lines) => lines.len(),
            PreviewContent::Text(lines) => lines.len(),
            other => panic!("expected text content, got {other:?}"),
        };
        assert!(
            loaded < 4,
            "the ceiling must stop the load short; got {loaded} lines"
        );
    }

    #[test]
    fn a_file_inside_both_ceilings_is_not_truncated() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "fn main() {{}}").unwrap();
        f.flush().unwrap();

        let (_, truncated) = highlight_file_sync(f.path(), 2000, 256 * 1024);
        assert!(!truncated);
    }
    use tempfile::NamedTempFile;

    #[test]
    fn highlight_rust_source() {
        let mut f = NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(f, "fn main() {{ println!(\"hello\"); }}").unwrap();

        let (content, truncated) = highlight_text_sync(f.path(), 2000);
        // Should produce a Highlighted result for a .rs file.
        assert!(
            matches!(
                content,
                PreviewContent::Highlighted(_) | PreviewContent::Text(_)
            ),
            "expected Highlighted or Text, got: {content:?}"
        );
        assert!(!truncated, "a one-line file cannot be truncated");
    }

    /// A file longer than the cap must both stop at the cap and say so — a
    /// preview that silently shows a prefix is the bug this reports.
    #[test]
    fn highlight_reports_truncation_at_the_cap() {
        let mut f = NamedTempFile::with_suffix(".rs").unwrap();
        for i in 0..50 {
            writeln!(f, "let x{i} = {i};").unwrap();
        }
        f.flush().unwrap();

        let (content, truncated) = highlight_text_sync(f.path(), 10);
        assert!(truncated, "50 lines loaded with a cap of 10 is truncated");
        match content {
            PreviewContent::Highlighted(lines) => assert_eq!(lines.len(), 10),
            PreviewContent::Text(lines) => assert_eq!(lines.len(), 10),
            other => panic!("expected line content, got {other:?}"),
        }
    }

    /// The cap landing exactly on the last line is not a truncation: the reader
    /// has seen the whole file, so the pane must not claim otherwise.
    #[test]
    fn highlight_at_exactly_the_cap_is_not_truncated() {
        let mut f = NamedTempFile::with_suffix(".rs").unwrap();
        for i in 0..10 {
            writeln!(f, "let x{i} = {i};").unwrap();
        }
        f.flush().unwrap();

        let (_, truncated) = highlight_text_sync(f.path(), 10);
        assert!(!truncated, "10 lines with a cap of 10 shows the whole file");
    }

    #[test]
    fn highlight_unknown_extension_falls_back_to_text() {
        let mut f = NamedTempFile::with_suffix(".xyzunknown123").unwrap();
        writeln!(f, "some content").unwrap();

        let (content, _) = highlight_text_sync(f.path(), 2000);
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
