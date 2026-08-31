//! Image file preview provider.
//!
//! Recognises image files by extension and hands them to
//! `workers/image_decode.rs`, which decodes them off-thread and builds the
//! encoder state for the terminal's inline-image protocol. The protocol itself
//! is resolved once per process by [`crate::preview::graphics`].
//!
//! Only `[preview] image_protocol = "none"` takes the synchronous path, which
//! reports metadata without decoding anything.

use std::path::Path;

use crate::app::state::{Entry, EntryKind};
use crate::preview::graphics::{self, ImageProtocol};
use crate::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewProvider};

/// Known image file extensions this provider handles.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "tiff", "tif", "webp", "avif", "svg",
];

/// Preview provider for image files.
///
/// Delegates the decode to the async worker pool, which sends
/// `WorkerMsg::ImageMeta` back to the UI thread. Falls back to a synchronous
/// metadata preview only when image previews are switched off.
pub struct ImageProvider;

impl PreviewProvider for ImageProvider {
    fn can_handle(&self, entry: &Entry) -> bool {
        if entry.kind != EntryKind::File {
            return false;
        }
        is_image_path(&entry.path)
    }

    fn preview(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome {
        if graphics::active() == ImageProtocol::None {
            // Previews are disabled: produce a compact metadata preview
            // synchronously so there is no unnecessary loading flash.
            let content = build_metadata_preview_sync(&entry.path, entry.metadata.as_ref());
            return PreviewOutcome::Ready(content);
        }

        crate::workers::image_decode::spawn_image_decode(
            entry.path.clone(),
            ctx.generation,
            ctx.worker_tx.clone(),
        );
        PreviewOutcome::Deferred
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
/// Used when image previews are disabled. Reads only filesystem metadata
/// (size, extension) — does not decode the image — so it is safe to call on
/// the UI thread.
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
        "  (image previews disabled)".to_owned(),
        "  Set [preview] image_protocol to re-enable them.".to_owned(),
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
