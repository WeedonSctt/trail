//! Binary file preview provider.
//!
//! Displays file metadata (size, type, modification timestamp) for non-text,
//! non-image binary files. The metadata read is done on a worker task to avoid
//! blocking the UI thread on slow filesystems.
//!
//! Entry routing:
//! - `ImageProvider` matches image files first (registered before `BinaryProvider`
//!   in the registry).
//! - `BinaryProvider` catches everything else that `TextProvider` and
//!   `DirectoryProvider` did not handle — i.e. any file whose first 8 KB looks
//!   binary to `content_inspector`.

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
    fn can_handle(&self, entry: &Entry) -> bool {
        if entry.kind != EntryKind::File {
            return false;
        }
        // Defer to ImageProvider for image files.
        if is_image_path(&entry.path) {
            return false;
        }
        // All non-text regular files.
        // `TextProvider` runs before us in the registry, so if we're called
        // with a file, `TextProvider::can_handle` returned false — meaning the
        // file is binary.
        true
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
    use chrono::{DateTime, Local};

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

    let size_str = humansize::format_size(meta.len(), humansize::DECIMAL);

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_uppercase();

    let modified = meta
        .modified()
        .ok()
        .map(|t| {
            let dt: DateTime<Local> = t.into();
            dt.format("%Y-%m-%d %H:%M").to_string()
        })
        .unwrap_or_else(|| "unknown".to_owned());

    let mut lines = vec![
        format!("  Type     : {} binary", ext),
        format!("  Size     : {}", size_str),
        format!("  Modified : {}", modified),
    ];

    // Include a hex dump hint if the file is non-empty.
    if meta.len() > 0 {
        lines.push(String::new());
        lines.push("  (binary content — no text preview)".to_owned());
    }

    PreviewContent::Binary(lines)
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
