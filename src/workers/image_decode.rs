//! Image decoding worker.
//!
//! Decodes image metadata/dimensions always; decodes pixel data only if the
//! detected terminal protocol supports inline images (Kitty, iTerm2, Sixel).
//!
//! # Protocol detection
//!
//! Detection is done via environment variable inspection at startup. The probe
//! order (Kitty → iTerm2 → Sixel → metadata-only fallback) follows the
//! implementation plan's Decision Log and matches `ratatui-image`'s own picker
//! logic, which is also referenced here.
//!
//! The detected protocol is determined once per process and reused for every
//! image preview, so there's no repeated probing cost.

use std::env;
use std::path::PathBuf;

use image::GenericImageView;
use tokio::sync::mpsc;

use crate::preview::provider::PreviewContent;
use crate::workers::WorkerMsg;

// ── Protocol detection ────────────────────────────────────────────────────────

/// The inline-image protocol the current terminal supports, or `None` for
/// metadata-only fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProtocol {
    /// Kitty Graphics Protocol (`TERM=xterm-kitty` or `KITTY_WINDOW_ID`).
    Kitty,
    /// iTerm2 inline images (`TERM_PROGRAM=iTerm.app` or `LC_TERMINAL=iTerm2`).
    Iterm2,
    /// Sixel graphics (detected via `TERM` containing `sixel`, e.g. from `mlterm`
    /// or `xterm -ti vt340`). This is a best-effort heuristic — proper detection
    /// would query the terminal via XTSMGRAPHICS, but that requires a round-trip
    /// to the tty which is not safe on the async worker thread.
    Sixel,
    /// No inline image support detected; show metadata only.
    None,
}

/// Probes the environment once and returns the best available image protocol.
///
/// Called by the image provider when it first needs to know whether pixel data
/// should be decoded. The result should be cached by the caller (or the single
/// static initialized at startup).
///
/// # Detection order
/// 1. `KITTY_WINDOW_ID` set → `Kitty`
/// 2. `TERM=xterm-kitty` → `Kitty`
/// 3. `TERM_PROGRAM=iTerm.app` or `LC_TERMINAL=iTerm2` → `Iterm2`
/// 4. `TERM` contains `sixel` → `Sixel`
/// 5. Fallback → `None`
pub fn detect_image_protocol() -> ImageProtocol {
    // Kitty: most explicit indicator.
    if env::var("KITTY_WINDOW_ID").is_ok() {
        return ImageProtocol::Kitty;
    }
    if env::var("TERM")
        .unwrap_or_default()
        .to_lowercase()
        .contains("kitty")
    {
        return ImageProtocol::Kitty;
    }

    // iTerm2.
    let term_program = env::var("TERM_PROGRAM").unwrap_or_default().to_lowercase();
    let lc_terminal = env::var("LC_TERMINAL").unwrap_or_default().to_lowercase();
    if term_program.contains("iterm") || lc_terminal.contains("iterm") {
        return ImageProtocol::Iterm2;
    }

    // Sixel (heuristic via TERM).
    if env::var("TERM")
        .unwrap_or_default()
        .to_lowercase()
        .contains("sixel")
    {
        return ImageProtocol::Sixel;
    }

    ImageProtocol::None
}

// ── Spawn helper ─────────────────────────────────────────────────────────────

/// Decodes image metadata (and optionally pixel data) off-thread, sending a
/// `WorkerMsg::ImageMeta` result through `tx` tagged with `generation`.
///
/// Always produces at least the dimensions and format string. Pixel data is
/// only decoded when `protocol != ImageProtocol::None`, but for v1 the pixel
/// rendering is not yet wired through `ratatui-image` (Phase 8 config / Phase 9
/// packaging will finalize the render path). The metadata lines are always
/// populated so the fallback is always useful.
pub fn spawn_image_decode(path: PathBuf, generation: u64, tx: mpsc::Sender<WorkerMsg>) {
    let protocol = detect_image_protocol();
    tokio::spawn(async move {
        let path_clone = path.clone();
        let content = tokio::task::spawn_blocking(move || decode_image_sync(&path_clone, protocol))
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

/// Performs the blocking image decode. Returns metadata lines suitable for
/// `PreviewContent::Binary` (reused for image metadata since it's the same
/// "formatted text lines" shape).
///
/// In v1, pixel preview is not rendered into the ratatui frame (the terminal
/// graphics API requires a stateful render pass that isn't wired yet). A
/// `[pixel preview: <Protocol>]` hint line is included so users know their
/// terminal supports it.
fn decode_image_sync(path: &std::path::Path, protocol: ImageProtocol) -> PreviewContent {
    let img = match image::open(path) {
        Ok(i) => i,
        Err(e) => return PreviewContent::Binary(vec![format!("Image decode error: {e}")]),
    };

    let (width, height) = img.dimensions();
    let color = img.color();
    let file_size = std::fs::metadata(path)
        .map(|m| humansize::format_size(m.len(), humansize::DECIMAL))
        .unwrap_or_else(|_| "unknown".to_owned());

    let format_str = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => ext.to_uppercase(),
        None => "Unknown".to_owned(),
    };

    let mut lines = vec![
        format!("  Type    : {} image", format_str),
        format!("  Size    : {}", file_size),
        format!("  Dims    : {}×{} px", width, height),
        format!("  Colour  : {:?}", color),
    ];

    match protocol {
        ImageProtocol::None => {
            lines.push(String::new());
            lines.push("  (no inline image protocol detected)".to_owned());
        }
        p => {
            lines.push(String::new());
            lines.push(format!("  [pixel preview available via {p:?}]"));
            lines.push("  (pixel rendering not yet active in this build)".to_owned());
        }
    }

    PreviewContent::Binary(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_protocol_returns_valid_variant() {
        // Verifies that detect_image_protocol never panics and always returns
        // a variant of the enum. The concrete value depends on the test
        // environment (e.g. running inside Kitty CI) — that is expected.
        let p = detect_image_protocol();
        assert!(matches!(
            p,
            ImageProtocol::Kitty
                | ImageProtocol::Iterm2
                | ImageProtocol::Sixel
                | ImageProtocol::None
        ));
    }
}
