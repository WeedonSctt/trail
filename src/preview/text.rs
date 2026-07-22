//! Text file preview provider.
//!
//! Provides preview content for text files. Phase 1 added plain-text output
//! with line numbers. Phase 5 upgrades the synchronous path to use `syntect`-
//! based syntax highlighting for files under `TEXT_SYNC_THRESHOLD`, and defers
//! large files (over `TEXT_SYNC_THRESHOLD`) to `workers/highlight.rs` via the
//! async worker pool.

use std::fs;
use std::path::Path;

use content_inspector::inspect;

use crate::app::state::{Entry, EntryKind};
use crate::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewProvider};

/// Byte threshold that separates synchronous (≤ threshold) from asynchronous
/// (> threshold) text preview.
///
/// Files at or under this size are highlighted synchronously on the UI thread;
/// larger files are deferred to the highlight worker. This value is hardcoded
/// here until Phase 7 moves it into `config.toml`.
///
/// Named constant per coding-standard §10: no magic numbers.
pub const TEXT_SYNC_THRESHOLD: usize = 256 * 1024; // 256 KB

/// Alias kept for backwards compatibility with code that referenced the old name.
pub const TEXT_PREVIEW_MAX_BYTES: usize = TEXT_SYNC_THRESHOLD;

/// Maximum number of lines shown in the plain-text (non-highlighted) fallback.
const TEXT_PREVIEW_MAX_LINES: usize = 500;

/// Synchronous/async preview provider for text files.
///
/// - Files ≤ `TEXT_SYNC_THRESHOLD`: highlighted synchronously via `syntect`.
/// - Files > `TEXT_SYNC_THRESHOLD`: `PreviewOutcome::Deferred` returned;
///   `workers::highlight::spawn_highlight` is called to do the work off-thread.
/// - Binary files: not handled here — `BinaryProvider` takes those.
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

    fn preview(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome {
        let size = entry
            .metadata
            .as_ref()
            .map(|m| m.len() as usize)
            .unwrap_or(0);

        if size > TEXT_SYNC_THRESHOLD {
            // Large file: spawn async highlight worker and show Loading placeholder.
            crate::workers::highlight::spawn_highlight(
                entry.path.clone(),
                ctx.generation,
                ctx.worker_tx.clone(),
            );
            PreviewOutcome::Deferred
        } else {
            // Small file: highlight synchronously on the UI thread.
            let content = crate::workers::highlight::highlight_text_sync(&entry.path);
            // highlight_text_sync may return Empty (0-byte or unreadable file);
            // fall back to plain-text builder in that case.
            let content = match content {
                PreviewContent::Empty => build_text_preview(&entry.path),
                other => other,
            };
            PreviewOutcome::Ready(content)
        }
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

    #[test]
    fn is_text_file_rejects_binary() {
        let mut f = NamedTempFile::new().unwrap();
        use std::io::Write;
        // Write a sequence of null bytes — content_inspector will flag these as binary.
        f.write_all(&[0u8; 64]).unwrap();
        assert!(!is_text_file(f.path()));
    }
}
