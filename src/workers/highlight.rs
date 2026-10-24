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

use std::path::PathBuf;
use std::sync::OnceLock;

use syntect::easy::HighlightLines;
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
/// Returns `PreviewContent::Highlighted`, paired with whether the file
/// continues past what was loaded. An empty file is `Highlighted` with no
/// lines, which the renderer labels as such; `Empty` is reserved for a file
/// that could not be read at all.
///
/// The file is read as bytes up to `max_bytes` and decoded by
/// [`crate::preview::text::decode_text`] before anything is highlighted, so a
/// file that is not UTF-8 — ANSI or UTF-16, both common on Windows — is shown
/// rather than coming back blank.
fn highlight_file_sync(
    path: &std::path::Path,
    max_lines: usize,
    max_bytes: usize,
) -> (PreviewContent, bool) {
    let Some((text, bytes_truncated)) = read_capped(path, max_bytes) else {
        return (PreviewContent::Empty, false);
    };

    let ss = syntax_set();
    let ts = theme_set();

    let theme = ts.themes.get(DEFAULT_THEME).unwrap_or_else(|| {
        // Any bundled theme works; fall back to the first available.
        ts.themes
            .values()
            .next()
            .expect("syntect ships at least one theme")
    });
    let first_line = text.lines().next().unwrap_or_default();
    let syntax = ss
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .or_else(|| ss.find_syntax_by_first_line(first_line))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut lines: Vec<HighlightedLine> = Vec::new();
    // Two ceilings, because either one alone leaves a hole: `max_lines` bounds
    // an ordinary file, and `max_bytes` (applied by the read above) bounds one
    // whose lines are enormous -- minified JavaScript, a single-line JSON blob
    // -- which would otherwise be loaded whole however few lines it has.
    let mut truncated = bytes_truncated;
    for raw in text.lines() {
        if lines.len() >= max_lines {
            truncated = true;
            break;
        }
        // Make the line safe to draw before anything highlights it — the spans
        // have to describe the text that is actually rendered.
        let line_text = sanitize(raw);
        let Ok(regions) = highlighter.highlight_line(&line_text, ss) else {
            // The rest of the file is not shown, so the pane must say so.
            truncated = true;
            break;
        };

        let spans: HighlightedLine = regions
            .into_iter()
            .filter(|(_, text)| !text.is_empty())
            .map(|(style, text)| StyledSpan {
                text: text.to_owned(),
                fg: convert_color(style.foreground),
            })
            .collect();

        lines.push(spans);
    }

    (PreviewContent::Highlighted(lines), truncated)
}

/// Reads at most `max_bytes` of `path` and decodes them, returning the text
/// and whether the file continues past the ceiling. `None` if it cannot be read.
fn read_capped(path: &std::path::Path, max_bytes: usize) -> Option<(String, bool)> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    // Read one byte past the ceiling: that extra byte is how we tell a file that
    // ends exactly on the limit from one that continues past it.
    file.take(max_bytes as u64 + 1).read_to_end(&mut buf).ok()?;
    let truncated = buf.len() > max_bytes;
    buf.truncate(max_bytes);
    Some((crate::preview::text::decode_text(&buf), truncated))
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

    fn loaded_lines(content: &PreviewContent) -> usize {
        match content {
            PreviewContent::Highlighted(lines) => lines.len(),
            other => panic!("expected highlighted content, got {other:?}"),
        }
    }

    /// A file that is not UTF-8 used to come back with no lines at all — the
    /// same blank pane as an empty file.
    #[test]
    fn a_non_utf8_file_is_previewed_not_blanked() {
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        f.write_all(b"caf\xe9\nol\xe9\n").unwrap();
        f.flush().unwrap();

        let (content, truncated) = highlight_text_sync(f.path(), 2000);
        assert_eq!(loaded_lines(&content), 2);
        assert!(!truncated);
    }

    /// A bad byte part-way down used to end the preview there without marking
    /// it truncated; now every line is read.
    #[test]
    fn a_bad_byte_mid_file_does_not_cut_the_preview() {
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        f.write_all(&b"ok\n".repeat(50)).unwrap();
        f.write_all(b"\xff\nafter\n").unwrap();
        f.flush().unwrap();

        let (content, truncated) = highlight_text_sync(f.path(), 2000);
        assert_eq!(loaded_lines(&content), 52);
        assert!(!truncated);
    }

    #[test]
    fn an_empty_file_has_no_lines_and_is_not_truncated() {
        let f = NamedTempFile::with_suffix(".txt").unwrap();
        let (content, truncated) = highlight_text_sync(f.path(), 2000);
        assert_eq!(loaded_lines(&content), 0);
        assert!(!truncated);
    }

    /// One line longer than the byte ceiling used to be loaded whole and
    /// reported complete.
    #[test]
    fn a_single_line_past_the_byte_ceiling_is_truncated() {
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        f.write_all(&[b'a'; 4096]).unwrap();
        f.flush().unwrap();

        let (content, truncated) = highlight_file_sync(f.path(), 2000, 1024);
        assert!(truncated);
        match content {
            PreviewContent::Highlighted(lines) => {
                let shown: usize = lines[0].iter().map(|s| s.text.len()).sum();
                assert_eq!(shown, 1024);
            }
            other => panic!("expected highlighted content, got {other:?}"),
        }
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
