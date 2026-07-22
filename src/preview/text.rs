//! Text file preview provider.
//!
//! Synchronous preview showing raw content with line numbers. Phase 1 shows
//! plain text only; Phase 5 upgrades this to `syntect`-based highlighting
//! for files under the size threshold.

use std::fs;
use std::path::Path;

use content_inspector::inspect;

use crate::app::state::{Entry, EntryKind};
use crate::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewProvider};

/// Maximum number of bytes read synchronously for the plain-text preview.
/// Files larger than this are read only up to this limit on the UI thread;
/// Phase 5 will add async highlighting for large files.
///
/// Named constant per coding-standard §10: no magic numbers.
pub const TEXT_PREVIEW_MAX_BYTES: usize = 256 * 1024; // 256 KB

/// Maximum number of lines shown in the plain-text preview.
const TEXT_PREVIEW_MAX_LINES: usize = 500;

/// Synchronous preview provider for text files.
///
/// Reads the first `TEXT_PREVIEW_MAX_BYTES` bytes of the file, detects
/// whether the content is text, and formats it as numbered lines.
/// Binary files are not handled here — `BinaryProvider` (Phase 5) takes
/// those via a `can_handle` check.
///
/// Phase 5 TODO: add `syntect`-based syntax highlighting for files under
/// `TEXT_SYNC_THRESHOLD`.
pub struct TextProvider;

impl PreviewProvider for TextProvider {
    fn can_handle(&self, entry: &Entry) -> bool {
        // Only handle regular files; directories go to DirectoryProvider.
        if entry.kind != EntryKind::File {
            return false;
        }
        // Peek at the content to determine if it's text.
        is_text_file(&entry.path)
    }

    fn preview(&self, entry: &Entry, _ctx: &PreviewCtx) -> PreviewOutcome {
        let content = build_text_preview(&entry.path);
        PreviewOutcome::Ready(content)
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
/// `" {n:>4}  {line}"`. Returns `PreviewContent::Empty` on I/O error.
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
        .map(|(i, line)| format!("{:>4}  {}", i + 1, line))
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
}
