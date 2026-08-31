//! Fixture-driven preview tests for Phase 5.
//!
//! Exercises each of the four `PreviewProvider` implementations against real
//! fixture files in `tests/fixtures/`. All tests are deterministic: no
//! wall-clock time, no network, no ambient machine state.
//!
//! Inline images are covered only through the Halfblocks protocol, which
//! needs no terminal support. Whether Kitty, iTerm2 or Sixel escape sequences
//! actually paint pixels cannot be asserted without the terminal in question,
//! so that matrix stays a manual release gate.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Serialises the tests that touch `preview::graphics`.
///
/// The protocol and cell size are process-wide, so two image tests running
/// concurrently in this binary would clobber each other's configuration.
static GRAPHICS: Mutex<()> = Mutex::new(());

/// Takes [`GRAPHICS`], recovering rather than propagating poisoning so that one
/// failing image test does not cascade into the others.
fn graphics_lock() -> MutexGuard<'static, ()> {
    GRAPHICS.lock().unwrap_or_else(PoisonError::into_inner)
}

// ── Text provider tests ───────────────────────────────────────────────────────

#[test]
fn text_provider_plain_txt_produces_content() {
    let path = Path::new("tests/fixtures/sample.txt");
    assert!(path.exists(), "fixture file missing: {}", path.display());

    let content = trail::preview::text::build_text_preview(path);
    match content {
        trail::preview::provider::PreviewContent::Text(lines) => {
            assert!(!lines.is_empty(), "expected non-empty text preview");
            assert!(
                lines.iter().any(|l| l.contains("Hello")),
                "expected 'Hello' in text preview"
            );
        }
        other => panic!("expected Text variant, got: {other:?}"),
    }
}

#[test]
fn text_provider_rs_file_is_text() {
    let path = Path::new("tests/fixtures/sample.rs");
    assert!(path.exists(), "fixture file missing: {}", path.display());

    assert!(
        trail::preview::text::is_text_file(path),
        "expected .rs file to be detected as text"
    );
}

#[test]
fn text_provider_rs_highlight_produces_highlighted_or_text() {
    let path = Path::new("tests/fixtures/sample.rs");
    assert!(path.exists(), "fixture file missing: {}", path.display());

    let content = trail::workers::highlight::highlight_text_sync(path);
    // Either Highlighted (syntect matched .rs) or Text (fallback) is acceptable.
    assert!(
        matches!(
            content,
            trail::preview::provider::PreviewContent::Highlighted(_)
                | trail::preview::provider::PreviewContent::Text(_)
        ),
        "unexpected variant: {content:?}"
    );

    // Ensure the content is non-empty whichever branch was taken.
    match content {
        trail::preview::provider::PreviewContent::Highlighted(ref lines) => {
            assert!(!lines.is_empty(), "highlighted output must not be empty");
        }
        trail::preview::provider::PreviewContent::Text(ref lines) => {
            assert!(!lines.is_empty(), "text fallback must not be empty");
        }
        _ => {}
    }
}

// ── Binary provider tests ─────────────────────────────────────────────────────

#[test]
fn binary_provider_produces_metadata_lines() {
    let path = Path::new("tests/fixtures/sample_binary.bin");
    assert!(path.exists(), "fixture file missing: {}", path.display());

    let meta = std::fs::metadata(path).unwrap();
    let content = trail::preview::binary::build_binary_preview(path, Some(&meta));

    match content {
        trail::preview::provider::PreviewContent::Binary(lines) => {
            let combined = lines.join("\n");
            assert!(
                combined.contains("Type"),
                "expected 'Type' label in binary preview"
            );
            assert!(
                combined.contains("Size"),
                "expected 'Size' label in binary preview"
            );
        }
        other => panic!("expected Binary variant, got: {other:?}"),
    }
}

#[test]
fn binary_provider_handles_missing_metadata_gracefully() {
    let path = Path::new("tests/fixtures/sample_binary.bin");
    assert!(path.exists(), "fixture file missing: {}", path.display());

    // Pass None — provider should fall back to fs::metadata call.
    let content = trail::preview::binary::build_binary_preview(path, None);
    assert!(
        matches!(content, trail::preview::provider::PreviewContent::Binary(_)),
        "expected Binary variant"
    );
}

// ── Image provider tests ──────────────────────────────────────────────────────

#[test]
fn image_provider_recognises_png_extension() {
    let path = Path::new("tests/fixtures/sample.png");
    assert!(path.exists(), "fixture file missing: {}", path.display());

    assert!(
        trail::preview::image::is_image_path(path),
        "expected .png to be recognised as an image path"
    );
}

/// Drives the image provider end to end for both of its outcomes.
///
/// The two cases share one test because they are two halves of one decision,
/// and the whole test holds [`GRAPHICS`] because the protocol it selects is
/// process-wide.
// `graphics_lock` is a plain `Mutex`, and this test holds it across `.await`.
// That is safe here: `#[tokio::test]` gives every test its own current-thread
// runtime, so blocking on the guard cannot stall another test's progress.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn image_provider_renders_pixels_and_honours_being_switched_off() {
    use trail::app::state::{Entry, EntryKind};
    use trail::preview::graphics::{self, DEFAULT_CELL_SIZE};
    use trail::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewProvider};

    let _guard = graphics_lock();
    let path = Path::new("tests/fixtures/sample.png");
    assert!(path.exists(), "fixture file missing: {}", path.display());

    let entry = Entry {
        path: path.to_path_buf(),
        file_name: "sample.png".to_owned(),
        kind: EntryKind::File,
        metadata: std::fs::metadata(path).ok(),
        is_hidden: false,
        git_status: None,
        is_text: Some(false),
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let ctx = PreviewCtx {
        show_hidden: false,
        worker_tx: tx,
        generation: 7,
        text_sync_threshold_bytes: 256 * 1024,
    };
    let provider = trail::preview::image::ImageProvider;

    // 1. Halfblocks is the protocol every terminal supports, so it is the one
    //    case that is deterministic on CI. The provider must defer to the
    //    worker, and the worker must come back with a drawable image.
    graphics::configure("halfblocks", DEFAULT_CELL_SIZE);
    assert!(
        matches!(provider.preview(&entry, &ctx), PreviewOutcome::Deferred),
        "an enabled protocol must decode off-thread"
    );

    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("image decode timed out")
        .expect("worker channel closed");

    match msg {
        trail::workers::WorkerMsg::ImageMeta {
            generation,
            content,
            ..
        } => {
            assert_eq!(generation, 7, "result must carry the requesting generation");
            assert!(
                matches!(content, PreviewContent::Image(_)),
                "expected a pixel preview, got {content:?}"
            );
        }
        other => panic!("expected ImageMeta, got {other:?}"),
    }

    // 2. Switched off, the provider answers synchronously with metadata and
    //    never touches the worker pool.
    graphics::configure("none", DEFAULT_CELL_SIZE);
    match provider.preview(&entry, &ctx) {
        PreviewOutcome::Ready(PreviewContent::Binary(lines)) => {
            assert!(
                lines.iter().any(|l| l.contains("disabled")),
                "expected the disabled notice, got {lines:?}"
            );
        }
        other => panic!("expected a ready metadata preview, got {other:?}"),
    }

    // Leave the shared state as the rest of the suite expects to find it.
    graphics::configure("auto", DEFAULT_CELL_SIZE);
}

/// A preview must survive the pane changing size, in both directions.
///
/// This is the regression test for images vanishing on terminal resize: the
/// encoder state is built once, off-thread, and then re-encoded on the UI
/// thread for whatever area the pane currently has. If that re-encode does not
/// happen, or paints outside the area it was handed, a real terminal ends up
/// with the surrounding UI drawn over the image and drops it entirely.
///
/// Halfblocks is used because it needs no terminal support and maps one buffer
/// cell to one cell of the image, so the footprint is directly observable.
#[test]
fn image_preview_refits_itself_when_the_pane_is_resized() {
    use image::{ImageBuffer, Rgb};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::StatefulWidget;
    use ratatui_image::StatefulImage;
    use trail::preview::graphics::{self, ImagePreview};

    /// The character the halfblocks protocol paints with.
    const HALF_BLOCK: &str = "\u{2580}";

    let _guard = graphics_lock();

    // Pin the cell size so the expected geometry does not depend on whichever
    // terminal happens to be running the suite.
    graphics::configure("halfblocks", (7, 14));
    let source = ImageBuffer::from_pixel(800, 600, Rgb::<u8>([200, 30, 30])).into();
    let mut preview = ImagePreview {
        protocol: graphics::build(source).expect("halfblocks always builds an encoder"),
        caption: String::new(),
    };

    // The image is drawn into a sub-rect of the screen, exactly as the preview
    // panel draws it inside its border. Painting outside that rect is what
    // makes a terminal discard the image.
    let screen = Rect::new(0, 0, 120, 40);

    // Shrink well past the image's natural size, then grow back.
    let mut footprints = Vec::new();
    for (width, height) in [(60, 30), (40, 20), (16, 8), (4, 2), (40, 20), (60, 30)] {
        let area = Rect::new(4, 2, width, height);
        let mut buf = Buffer::empty(screen);
        StatefulImage::new(None).render(area, &mut buf, &mut preview.protocol);

        let mut painted = 0usize;
        for y in screen.top()..screen.bottom() {
            for x in screen.left()..screen.right() {
                if buf[(x, y)].symbol() != HALF_BLOCK {
                    continue;
                }
                painted += 1;
                assert!(
                    x >= area.left() && x < area.right() && y >= area.top() && y < area.bottom(),
                    "pane {width}x{height}: painted cell ({x}, {y}) escaped {area:?}"
                );
            }
        }

        assert!(
            painted > 0,
            "the image disappeared at pane {width}x{height}"
        );
        footprints.push(painted);
    }

    // Shrinking the pane must shrink the image and growing it must grow the
    // image back, which only holds if every resize re-encodes rather than
    // reusing the previous frame's data.
    assert!(
        footprints[0] > footprints[1]
            && footprints[1] > footprints[2]
            && footprints[2] > footprints[3],
        "footprint did not shrink with the pane: {footprints:?}"
    );
    assert_eq!(
        (footprints[4], footprints[5]),
        (footprints[1], footprints[0]),
        "growing back to a previous size must restore its footprint: {footprints:?}"
    );

    graphics::configure("auto", graphics::AUTO_CELL_SIZE);
}

// ── Directory provider tests ──────────────────────────────────────────────────

#[test]
fn directory_provider_counts_fixture_dir_entries() {
    let fixtures_dir = Path::new("tests/fixtures");
    assert!(fixtures_dir.exists(), "fixtures directory missing");

    let content = trail::preview::directory::build_directory_preview(fixtures_dir, false);

    match content {
        trail::preview::provider::PreviewContent::Directory {
            file_count,
            dir_count,
            ..
        } => {
            // We have at least the fixture files we created.
            assert!(
                file_count + dir_count > 0,
                "expected at least one entry in fixtures/"
            );
        }
        other => panic!("expected Directory variant, got: {other:?}"),
    }
}

// ── Generation-guard regression tests ────────────────────────────────────────

#[test]
fn generation_guard_drops_stale_preview() {
    use trail::app::state::AppState;
    use trail::preview::provider::PreviewContent;
    use trail::workers::{merge, WorkerMsg};

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();

    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    // Set generation to 5 — simulating 5 prior selection changes.
    state.preview.generation = 5;
    state.preview.for_path = dir.path().join("a.txt");

    // A message from generation 3 (stale) — must be dropped.
    let stale_msg = WorkerMsg::Preview {
        generation: 3,
        path: dir.path().join("a.txt"),
        content: PreviewContent::Text(vec!["stale".to_owned()]),
    };
    merge(stale_msg, &mut state);
    // Content should NOT have been updated.
    assert!(
        !matches!(&state.preview.content, PreviewContent::Text(lines) if lines.iter().any(|l| l == "stale")),
        "stale preview should have been dropped"
    );

    // A message from the current generation (5) — must be applied.
    let current_msg = WorkerMsg::Preview {
        generation: 5,
        path: dir.path().join("a.txt"),
        content: PreviewContent::Text(vec!["current".to_owned()]),
    };
    state.dirty = false; // reset before merge
    merge(current_msg, &mut state);
    assert!(
        matches!(&state.preview.content, PreviewContent::Text(lines) if lines.iter().any(|l| l == "current")),
        "current-generation preview should have been applied"
    );
    assert!(state.dirty, "merge should set dirty=true");
}

#[test]
fn generation_guard_drops_stale_image_meta() {
    use trail::app::state::AppState;
    use trail::preview::provider::PreviewContent;
    use trail::workers::{merge, WorkerMsg};

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("img.png"), b"fake").unwrap();

    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.preview.generation = 10;
    state.preview.for_path = dir.path().join("img.png");

    // Stale ImageMeta — generation mismatch.
    let stale = WorkerMsg::ImageMeta {
        generation: 7,
        path: dir.path().join("img.png"),
        content: PreviewContent::Binary(vec!["stale image".to_owned()]),
    };
    merge(stale, &mut state);
    assert!(
        !matches!(&state.preview.content, PreviewContent::Binary(lines) if lines.iter().any(|l| l.contains("stale"))),
        "stale image meta should have been dropped"
    );

    // Current-generation ImageMeta.
    let current = WorkerMsg::ImageMeta {
        generation: 10,
        path: dir.path().join("img.png"),
        content: PreviewContent::Binary(vec!["current image".to_owned()]),
    };
    state.dirty = false;
    merge(current, &mut state);
    assert!(
        matches!(&state.preview.content, PreviewContent::Binary(lines) if lines.iter().any(|l| l.contains("current"))),
        "current image meta should have been applied"
    );
    assert!(state.dirty, "merge should set dirty=true");
}
