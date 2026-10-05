//! Binary file preview provider.
//!
//! Displays file metadata (size, type, modification timestamp) for non-text,
//! non-image binary files, from the metadata the directory listing already
//! collected — so this provider performs no I/O of its own.
//!
//! Entry routing:
//! - `ImageProvider` matches image files first (registered before
//!   `BinaryProvider` in the registry).
//! - `TextProvider` takes every file *not yet* classified, because the worker it
//!   spawns reads the file and can classify it on the way past.
//! - `BinaryProvider` therefore serves files already known to be binary: the
//!   second and later previews of one, after `workers::merge` cached what that
//!   worker found.

use std::path::Path;

use crate::app::state::{Entry, EntryKind};
use crate::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewProvider};

/// Known image extensions handled by `ImageProvider`.
///
/// `BinaryProvider::can_handle` returns `false` for these so the image provider
/// gets first pick.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "tiff", "tif", "webp", "avif", "svg",
];

/// Preview provider for binary (non-text, non-image) files.
///
/// Returns file metadata (size, kind, modification time) synchronously from
/// cached `Entry::metadata`. No worker task is spawned because the metadata
/// was already collected by the directory listing.
pub struct BinaryProvider;

impl PreviewProvider for BinaryProvider {
    fn can_handle(&self, entry: &Entry, _ctx: &PreviewCtx) -> bool {
        if entry.kind != EntryKind::File {
            return false;
        }
        // Defer to ImageProvider for image files.
        if is_image_path(&entry.path) {
            return false;
        }
        // Only a file already *known* to be binary. `TextProvider` runs first
        // and takes every unclassified file, so this arm is reached on the
        // second and later previews of a binary file — the highlight worker
        // classified it the first time and `workers::merge` cached that.
        entry.is_text == Some(false)
    }

    fn preview(&self, entry: &Entry, _ctx: &PreviewCtx) -> PreviewOutcome {
        let content = build_binary_preview(&entry.path, entry.metadata.as_ref());
        PreviewOutcome::Ready(content)
    }
}

/// Returns `true` if `path` has a known image file extension.
fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Builds a `PreviewContent::Binary` describing `path`.
///
/// Reads metadata from the already-cached `metadata` if present, or performs a
/// fresh `fs::metadata` call if not. Returns `PreviewContent::Empty` only if
/// neither source works.
pub fn build_binary_preview(path: &Path, metadata: Option<&std::fs::Metadata>) -> PreviewContent {
    let owned;
    let meta: &std::fs::Metadata = match metadata {
        Some(m) => m,
        None => {
            owned = match std::fs::metadata(path) {
                Ok(m) => m,
                Err(e) => {
                    return PreviewContent::Binary(vec![format!("  Cannot read metadata: {e}")])
                }
            };
            &owned
        }
    };

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_uppercase();

    let mut lines = vec![format!("  Type     : {} binary", ext)];
    lines.extend(size_and_modified(meta));

    // Include a hex dump hint if the file is non-empty.
    if meta.len() > 0 {
        lines.push(String::new());
        lines.push("  (binary content — no text preview)".to_owned());
    }

    PreviewContent::Binary(lines)
}

/// The `Size` and `Modified` lines for a file, in the `"  Label : value"` form
/// the preview panel draws with a bold label.
///
/// Uses `metadata` when the listing already has it, else reads it. Says why
/// when it cannot be read rather than returning nothing. Makes no claim about
/// what kind of file it is, which is why the failed-external-preview pane uses
/// this rather than [`build_binary_preview`]: a Markdown file whose previewer
/// failed is not a binary.
pub fn file_metadata_lines(path: &Path, metadata: Option<&std::fs::Metadata>) -> Vec<String> {
    match metadata {
        Some(meta) => size_and_modified(meta),
        None => match std::fs::metadata(path) {
            Ok(meta) => size_and_modified(&meta),
            Err(e) => vec![format!("  Cannot read metadata: {e}")],
        },
    }
}

/// The two lines shared by every metadata block.
///
/// Formatted by `metafmt`, the same as the listing's details column, so a file
/// cannot be described one way in the pane and another in the list.
fn size_and_modified(meta: &std::fs::Metadata) -> Vec<String> {
    vec![
        format!("  Size     : {}", crate::metafmt::size(meta.len())),
        format!(
            "  Modified : {}",
            crate::metafmt::modified(meta.modified().ok())
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn binary_preview_includes_size() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(&[0u8; 1024]).unwrap();
        let meta = std::fs::metadata(f.path()).unwrap();
        let content = build_binary_preview(f.path(), Some(&meta));
        if let PreviewContent::Binary(lines) = content {
            let combined = lines.join("\n");
            assert!(
                combined.contains("1.02 kB")
                    || combined.contains("1 kB")
                    || combined.contains("1024"),
                "expected size info in: {combined}"
            );
        } else {
            panic!("expected Binary variant");
        }
    }

    #[test]
    fn is_image_path_recognises_png() {
        let p = Path::new("photo.PNG");
        assert!(is_image_path(p));
    }

    #[test]
    fn is_image_path_ignores_rs() {
        let p = Path::new("main.rs");
        assert!(!is_image_path(p));
    }
}
