//! Image decoding worker.
//!
//! Decodes an image file off the UI thread and turns it into a
//! `PreviewContent::Image` carrying the encoder state for the terminal's
//! inline-image protocol. Which protocol that is, and whether pixel previews
//! are enabled at all, is decided by [`crate::preview::graphics`].
//!
//! Decoding is the expensive part (a large JPEG can take tens of milliseconds),
//! which is why it runs on `spawn_blocking` and reports back through
//! `WorkerMsg::ImageMeta` tagged with the preview generation. Resizing and
//! encoding for the pane's current size happen later, on the UI thread, and are
//! cached by `ratatui-image` until the pane changes size.

use std::path::PathBuf;

use image::GenericImageView;
use tokio::sync::mpsc;

use crate::preview::graphics::{self, ImagePreview};
use crate::preview::provider::PreviewContent;
use crate::workers::WorkerMsg;

// ── Spawn helper ─────────────────────────────────────────────────────────────

/// Decodes `path` off-thread, sending a `WorkerMsg::ImageMeta` result through
/// `tx` tagged with `generation`.
///
/// The generation tag is what lets `workers::merge` drop a result whose
/// selection has already been abandoned.
pub fn spawn_image_decode(path: PathBuf, generation: u64, tx: mpsc::Sender<WorkerMsg>) {
    tokio::spawn(async move {
        let path_clone = path.clone();
        let content = tokio::task::spawn_blocking(move || decode_image_sync(&path_clone))
            .await
            .unwrap_or_else(|_| PreviewContent::Binary(vec!["[image decode error]".to_owned()]));

        let msg = WorkerMsg::ImageMeta {
            generation,
            path: path.clone(),
            content,
        };
        // Ignore send errors — the UI thread may have exited.
        let _ = tx.send(msg).await;
    });
}

// ── Blocking decode ───────────────────────────────────────────────────────────

/// Performs the blocking decode and builds the renderable preview.
///
/// Falls back to `PreviewContent::Binary` metadata lines when the file cannot
/// be decoded (an unsupported format such as SVG, or a truncated file) or when
/// image previews are disabled, so the pane always shows something useful.
fn decode_image_sync(path: &std::path::Path) -> PreviewContent {
    let image = match image::open(path) {
        Ok(image) => image,
        Err(e) => {
            return PreviewContent::Binary(vec![
                format!("  Type    : {} image", format_label(path)),
                format!("  Size    : {}", file_size(path)),
                String::new(),
                format!("  Cannot decode: {e}"),
            ])
        }
    };

    let (width, height) = image.dimensions();
    let colour = image.color();
    let (cell_width, cell_height) = graphics::cell_size();
    let protocol = graphics::active();

    let caption = format!(
        "  {} · {}×{} px · {} · {} {}×{}",
        format_label(path),
        width,
        height,
        file_size(path),
        protocol.label(),
        cell_width,
        cell_height,
    );

    match graphics::build(image) {
        Some(protocol) => PreviewContent::Image(ImagePreview { protocol, caption }),
        // Image previews are switched off via `[preview] image_protocol`.
        None => PreviewContent::Binary(vec![
            format!("  Type    : {} image", format_label(path)),
            format!("  Size    : {}", file_size(path)),
            format!("  Dims    : {width}×{height} px"),
            format!("  Colour  : {colour:?}"),
            String::new(),
            "  (image previews disabled: [preview] image_protocol = \"none\")".to_owned(),
        ]),
    }
}

/// The uppercased file extension, used as a human-readable format label.
fn format_label(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_uppercase())
        .unwrap_or_else(|| "Unknown".to_owned())
}

/// The file size formatted for display, or `"unknown"` if it cannot be read.
fn file_size(path: &std::path::Path) -> String {
    std::fs::metadata(path)
        .map(|m| humansize::format_size(m.len(), humansize::DECIMAL))
        .unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Writes a tiny valid PNG to a temp file and returns the handle.
    fn sample_png() -> tempfile::NamedTempFile {
        let image = image::DynamicImage::new_rgb8(4, 3);
        let mut bytes = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("encode sample png");

        let mut file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        file.write_all(&bytes.into_inner()).expect("write png");
        file.flush().expect("flush png");
        file
    }

    #[test]
    fn decodes_a_real_image_into_an_image_preview() {
        let file = sample_png();
        // The test process is not a terminal, so detection lands on
        // Halfblocks — which still produces a drawable preview.
        match decode_image_sync(file.path()) {
            PreviewContent::Image(preview) => {
                assert!(
                    preview.caption.contains("4×3 px"),
                    "caption should carry the decoded dimensions, got {:?}",
                    preview.caption
                );
            }
            other => panic!("expected an image preview, got {other:?}"),
        }
    }

    #[test]
    fn undecodable_file_falls_back_to_metadata() {
        let mut file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        file.write_all(b"not actually a png").expect("write");
        file.flush().expect("flush");

        match decode_image_sync(file.path()) {
            PreviewContent::Binary(lines) => {
                assert!(
                    lines.iter().any(|l| l.contains("Cannot decode")),
                    "expected a decode failure line, got {lines:?}"
                );
            }
            other => panic!("expected metadata fallback, got {other:?}"),
        }
    }

    #[test]
    fn format_label_falls_back_when_there_is_no_extension() {
        assert_eq!(format_label(std::path::Path::new("photo.JpEg")), "JPEG");
        assert_eq!(format_label(std::path::Path::new("photo")), "Unknown");
    }
}
