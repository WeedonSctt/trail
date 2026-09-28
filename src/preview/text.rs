//! Text file preview provider.
//!
//! Provides preview content for text files, and for files not yet known to be
//! anything else. Every preview is deferred to `workers/highlight.rs` via the
//! async worker pool — no blocking I/O is performed on the UI thread — and the
//! `Loading` placeholder is shown until the worker delivers its result.
//!
//! `can_handle` therefore takes any regular file except one already classified
//! as binary: the worker has to read the file anyway, so it is the cheapest
//! place to find out which it is. [`is_text_file`] is the probe it uses.

use std::fs;
use std::path::Path;

use content_inspector::inspect;

use crate::app::state::{Entry, EntryKind};
use crate::preview::provider::{
    sanitize, PreviewContent, PreviewCtx, PreviewOutcome, PreviewProvider,
};

/// Most bytes of a file the preview will read.
///
/// Every text preview is off-thread now, so this is no longer a
/// synchronous/asynchronous boundary — it is the ceiling that keeps one read
/// bounded, and a preview that hits it is reported as truncated.
///
/// Named constant per coding-standard §10: no magic numbers.
pub const TEXT_SYNC_THRESHOLD: usize = 256 * 1024; // 256 KB

/// Alias kept for backwards compatibility with code that referenced the old name.
pub const TEXT_PREVIEW_MAX_BYTES: usize = TEXT_SYNC_THRESHOLD;

/// Maximum number of lines shown in the plain-text (non-highlighted) fallback.
#[allow(dead_code)]
const TEXT_PREVIEW_MAX_LINES: usize = 500;

/// Preview provider for text files and for files not yet classified.
///
/// Always returns `PreviewOutcome::Deferred`: `workers::highlight` reads the
/// file, decides whether it is text, and highlights it if so. A file already
/// known to be binary is left to `BinaryProvider`, which needs no worker.
pub struct TextProvider;

impl PreviewProvider for TextProvider {
    fn can_handle(&self, entry: &Entry) -> bool {
        // Only handle regular files; directories go to DirectoryProvider.
        if entry.kind != EntryKind::File {
            return false;
        }
        // Take anything not already known to be binary. An unclassified file
        // lands here rather than on the binary provider because the worker this
        // spawns has to read the file to preview it anyway, and can answer the
        // question on the way past — whereas answering it here would mean
        // reading from disk on the UI thread.
        entry.is_text != Some(false)
    }

    fn preview(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome {
        // Always spawn the worker, regardless of file size.
        //
        // Files under a size threshold used to be highlighted synchronously on
        // the UI thread, which stalled the event loop for any file big enough to
        // take perceptible time. Routing every file through the worker keeps the
        // thread free; the `Loading` placeholder covers the async window, and the
        // generation-guard in `workers::merge` discards results for a selection
        // the user has already left.
        crate::workers::highlight::spawn_highlight(
            entry.path.clone(),
            ctx.generation,
            ctx.worker_tx.clone(),
            ctx.max_preview_lines,
        );
        PreviewOutcome::Deferred
    }
}

/// Returns `true` if `path` is a readable text file (not binary).
///
/// Reads the first 8 KB and passes it to `content_inspector` to detect
/// binary content. Returns `false` on I/O errors.
pub fn is_text_file(path: &Path) -> bool {
    use std::io::Read;
    let mut buf = [0u8; 8192];
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let n = f.read(&mut buf).unwrap_or(0);
    inspect(&buf[..n]).is_text()
}

/// Builds a `PreviewContent::Text` for `path`.
///
/// Reads up to `TEXT_PREVIEW_MAX_BYTES` and formats each line as
/// `\" {n:>4}  {line}\"`. Returns `PreviewContent::Empty` on I/O error.
///
/// Used as a fallback when the syntect path returns `Empty`, and by the async
/// highlight worker's plain-text fallback path.
// Integration tests call this function via `trail::preview::text::build_text_preview`.
// The binary itself no longer needs it (all text previews are async), so the
// compiler warns about dead code; suppress that warning rather than removing
// the function from the public API.
#[allow(dead_code)]
pub fn build_text_preview(path: &Path) -> PreviewContent {
    use std::io::Read;
    let Ok(f) = fs::File::open(path) else {
        return PreviewContent::Empty;
    };

    let mut buf = Vec::with_capacity(TEXT_PREVIEW_MAX_BYTES.min(65536));
    // Read at most TEXT_PREVIEW_MAX_BYTES to keep the UI thread responsive.
    let mut limited = f.take(TEXT_PREVIEW_MAX_BYTES as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return PreviewContent::Empty;
    }

    // Attempt lossy UTF-8 conversion.
    let text = String::from_utf8_lossy(&buf);

    let lines: Vec<String> = text
        .lines()
        .take(TEXT_PREVIEW_MAX_LINES)
        .enumerate()
        .map(|(i, line)| format!("{:>4}  {}", i + 1, sanitize(line)))
        .collect();

    PreviewContent::Text(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn text_preview_numbers_lines() {
        let mut f = NamedTempFile::new().unwrap();
        use std::io::Write;
        writeln!(f, "hello").unwrap();
        writeln!(f, "world").unwrap();

        let content = build_text_preview(f.path());
        if let PreviewContent::Text(lines) = content {
            assert_eq!(lines.len(), 2);
            assert!(lines[0].contains("hello"));
            assert!(lines[0].contains("   1"));
            assert!(lines[1].contains("world"));
        } else {
            panic!("expected Text variant");
        }
    }

    #[test]
    fn is_text_file_detects_text() {
        let mut f = NamedTempFile::new().unwrap();
        use std::io::Write;
        write!(f, "plain text content").unwrap();
        assert!(is_text_file(f.path()));
    }

    #[test]
    fn is_text_file_rejects_binary() {
        let mut f = NamedTempFile::new().unwrap();
        use std::io::Write;
        // Write a sequence of null bytes — content_inspector will flag these as binary.
        f.write_all(&[0u8; 64]).unwrap();
        assert!(!is_text_file(f.path()));
    }
}
