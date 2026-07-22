//! Image file preview provider.
//!
//! Displays image metadata (format, dimensions, file size, colour mode) always,
//! and adds a pixel-preview hint when the terminal supports an inline-image
//! protocol (Kitty, iTerm2, or Sixel). Pixel rendering is delegated to
//! `workers/image_decode.rs` via the async worker pool.
//!
//! # Protocol detection
//!
//! The protocol probe runs once per process (lazily on the first image preview)
//! via `workers::image_decode::detect_image_protocol`. Subsequent calls read the
//! cached result without re-probing the environment.

use std::path::Path;
use std::sync::OnceLock;

use crate::app::state::{Entry, EntryKind};
use crate::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewProvider};
use crate::workers::image_decode::{detect_image_protocol, ImageProtocol};

/// Known image file extensions this provider handles.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "tiff", "tif", "webp", "avif", "svg",
];

/// Cached protocol detection result. Initialised on first image preview.
static DETECTED_PROTOCOL: OnceLock<ImageProtocol> = OnceLock::new();

/// Returns the cached protocol, detecting it the first time.
fn cached_protocol() -> ImageProtocol {
    *DETECTED_PROTOCOL.get_or_init(detect_image_protocol)
}

/// Preview provider for image files.
///
/// Always provides metadata lines (format, dimensions, size). When the terminal
/// supports an inline-image protocol, the decode is delegated to the async
/// worker pool which sends `WorkerMsg::ImageMeta` back to the UI thread.
pub struct ImageProvider;

impl PreviewProvider for ImageProvider {
    fn can_handle(&self, entry: &Entry) -> bool {
        if entry.kind != EntryKind::File {
            return false;
        }
        is_image_path(&entry.path)
    }

    fn preview(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome {
        let protocol = cached_protocol();

        if protocol != ImageProtocol::None {
            // Spawn async worker that decodes metadata (and in a future release,
            // pixel data). Show Loading placeholder until the result arrives.
            crate::workers::image_decode::spawn_image_decode(
                entry.path.clone(),
                ctx.generation,
                ctx.worker_tx.clone(),
            );
            PreviewOutcome::Deferred
        } else {
            // No inline-image support: produce a compact metadata preview
            // synchronously so there is no unnecessary loading flash.
            let content = build_metadata_preview_sync(&entry.path, entry.metadata.as_ref());
            PreviewOutcome::Ready(content)
        }
    }
}

/// Returns `true` if `path` has a recognised image extension.
pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Produces a compact metadata preview without spawning a worker.
///
/// Used when no inline-image protocol is available. Reads only filesystem
/// metadata (size, extension) — does not decode the image — so it is safe to
/// call on the UI thread.
fn build_metadata_preview_sync(
    path: &Path,
    metadata: Option<&std::fs::Metadata>,
) -> PreviewContent {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_uppercase();

    let size_str = metadata
        .map(|m| humansize::format_size(m.len(), humansize::DECIMAL))
        .or_else(|| {
            std::fs::metadata(path)
                .map(|m| humansize::format_size(m.len(), humansize::DECIMAL))
                .ok()
        })
        .unwrap_or_else(|| "unknown".to_owned());

    PreviewContent::Binary(vec![
        format!("  Type  : {} image", ext),
        format!("  Size  : {}", size_str),
        String::new(),
        "  (no inline image protocol detected)".to_owned(),
        "  Run trail in a Kitty, iTerm2, or Sixel terminal for pixel preview.".to_owned(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_path_recognises_extensions() {
        for ext in ["png", "jpg", "jpeg", "gif", "bmp", "webp"] {
            let name = format!("image.{ext}");
            let path = Path::new(&name);
            assert!(is_image_path(path), "expected image path for .{ext}");
        }
    }

    #[test]
    fn non_image_extensions_rejected() {
        for ext in ["rs", "txt", "md", "toml"] {
            let name = format!("file.{ext}");
            let path = Path::new(&name);
            assert!(!is_image_path(path), "should not be image for .{ext}");
        }
    }

    #[test]
    fn metadata_preview_sync_produces_binary_lines() {
        use tempfile::NamedTempFile;
        let f = NamedTempFile::with_suffix(".png").unwrap();
        let meta = std::fs::metadata(f.path()).ok();
        let content = build_metadata_preview_sync(f.path(), meta.as_ref());
        assert!(matches!(content, PreviewContent::Binary(_)));
    }
}
