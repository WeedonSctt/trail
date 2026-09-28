//! `PreviewProvider` trait and `PreviewRegistry`.
//!
//! Defines the contract that all preview providers implement, and the
//! registry that dispatches preview requests to the first matching provider.
//! New entry types (PDF, archive, etc.) implement `PreviewProvider` and
//! register an instance at startup — the core loop and render code do not
//! change.
//!
//! Phase 5 adds `Highlighted` and `Binary` variants to `PreviewContent` and
//! enriches `PreviewCtx` with the worker sender and generation counter so
//! providers can spawn async tasks.

use std::path::PathBuf;

use ratatui::style::Color;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::app::state::Entry;
use crate::preview::graphics::ImagePreview;
use crate::workers::WorkerMsg;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors from preview operations.
///
/// Currently unused in built-in providers (which return graceful fallbacks
/// rather than propagating errors). Defined here for use by plugin-provided
/// `PreviewProvider` implementations (Phase 8).
// clippy: dead_code — reserved for plugin-provided PreviewProvider impls (Phase 8)
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum PreviewError {
    /// An I/O error occurred while reading file content.
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

// ── PreviewContent ────────────────────────────────────────────────────────────

/// A single span of text with an optional foreground colour for highlighted
/// preview rendering.
///
/// Used by `PreviewContent::Highlighted` to carry the output of `syntect`
/// syntax highlighting. Each `StyledSpan` maps to a ratatui `Span`.
#[derive(Debug, Clone)]
pub struct StyledSpan {
    /// The text content of this span.
    pub text: String,
    /// Optional foreground colour (RGB). `None` means "use default foreground".
    pub fg: Option<Color>,
}

/// A single highlighted line, made up of one or more [`StyledSpan`]s.
pub type HighlightedLine = Vec<StyledSpan>;

// ── Making file content safe to draw ──────────────────────────────────────────

/// Columns a tab is expanded to in preview text.
///
/// Not tab-stop aware: each tab becomes this many spaces wherever it appears.
/// The alternative is tracking the column across styled spans that are built
/// independently, for a difference nobody reading a preview would notice.
///
/// Named constant per coding-standard §10: no magic numbers.
const TAB_WIDTH: usize = 4;

/// Shown in place of a control character.
const CONTROL_PLACEHOLDER: char = '·';

/// Returns `line` with everything a terminal would *act on* replaced by
/// something it will print.
///
/// A file reaches the text preview when `content_inspector` finds no NUL bytes
/// in its first 8 KB, which is not the same as finding nothing dangerous in it:
/// ESC, BEL, CR and the other control characters all survive that test. They
/// used to be written into the render buffer verbatim and handed to the
/// terminal, which obeyed them rather than drawing them — stray characters
/// anywhere on screen, including over the panel borders and the listing, and
/// gone again after any full redraw. That is the artifact this removes, and with
/// it the reason the renderer cleared the whole screen on every frame.
///
/// Tabs become spaces (a terminal advances to its own next tab stop, which no
/// pane layout can predict) and every other control character becomes a middle
/// dot, so the line keeps its length in cells and the preview stays honest about
/// what is in the file.
///
/// Bidirectional and zero-width characters are left alone: they are legitimate
/// text content, and the terminal draws rather than executes them.
pub fn sanitize(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for ch in line.chars() {
        match ch {
            '\t' => (0..TAB_WIDTH).for_each(|_| out.push(' ')),
            ch if ch.is_control() => out.push(CONTROL_PLACEHOLDER),
            ch => out.push(ch),
        }
    }
    out
}

/// The renderable content for the preview pane.
///
/// Each variant carries the data needed by `ui/preview_panel.rs` to draw
/// the pane without any further I/O.
#[derive(Debug, Clone, Default)]
pub enum PreviewContent {
    /// A placeholder shown before any entry is selected or while loading.
    #[default]
    Empty,

    /// A placeholder shown while an async worker result is in flight.
    ///
    /// Displayed while `workers/highlight.rs`, `workers/image_decode.rs`, or
    /// the binary metadata worker are running.
    Loading,

    /// Plain-text preview with pre-formatted lines.
    ///
    /// Each element is a formatted string (e.g. `"   1  first line of file"`).
    /// Used for small text files where syntect highlighting is not available
    /// (e.g. unknown syntax) and as the async-deferred fallback on error.
    Text(Vec<String>),

    /// Syntax-highlighted text preview (Phase 5).
    ///
    /// Each outer element is a line; each inner element is a styled span.
    /// Line numbers are prepended as an unstyled span by the render code.
    Highlighted(Vec<HighlightedLine>),

    /// Binary file metadata (Phase 5).
    ///
    /// Each string is one line of formatted metadata (size, type, modified
    /// timestamp, etc.).
    Binary(Vec<String>),

    /// Pixel image preview drawn through a terminal graphics protocol.
    ///
    /// Produced by `workers/image_decode.rs` once the image has been decoded
    /// off-thread. Carries its own encoder state, so the render path needs no
    /// further I/O.
    Image(ImagePreview),

    /// Directory preview: a summary block followed by entry names.
    Directory {
        /// Total number of files (non-directories).
        file_count: usize,
        /// Total number of subdirectories.
        dir_count: usize,
        /// Number of hidden entries.
        hidden_count: usize,
        /// Up to the first N entry names for quick inspection.
        entries: Vec<String>,
    },
}

/// Number of lines the directory preview spends on its summary header before
/// the first entry name: the counts line and the blank line under it.
///
/// The header scrolls with the entry list rather than staying pinned, so it is
/// part of the scrollable length.
const DIRECTORY_HEADER_LINES: usize = 2;

impl PreviewContent {
    /// Number of logical lines this content occupies, or `None` when it does
    /// not scroll.
    ///
    /// `None` covers the placeholders and `Image`: an image is drawn as pixels
    /// into the whole pane, so a line offset is meaningless, and returning
    /// `None` for `Loading` is what stops a deferred re-preview of the same
    /// file from resetting a scroll position that is about to be valid again.
    pub fn scrollable_len(&self) -> Option<usize> {
        match self {
            Self::Empty | Self::Loading | Self::Image(_) => None,
            Self::Text(lines) | Self::Binary(lines) => Some(lines.len()),
            Self::Highlighted(lines) => Some(lines.len()),
            Self::Directory { entries, .. } => Some(entries.len() + DIRECTORY_HEADER_LINES),
        }
    }
}

// ── PreviewOutcome ────────────────────────────────────────────────────────────

/// The result of calling `PreviewProvider::preview`.
///
/// `Ready` means the provider produced content synchronously.
/// `Deferred` means it spawned a worker task; the UI should show
/// `PreviewContent::Loading` until the matching `WorkerMsg` arrives.
#[derive(Debug)]
pub enum PreviewOutcome {
    /// Synchronous path: content is ready immediately.
    Ready(PreviewContent),
    /// Asynchronous path: a worker task was spawned.
    ///
    /// The UI renders `PreviewContent::Loading` until the worker result
    /// merges in via `workers::merge`.
    Deferred,
}

// ── PreviewCtx ────────────────────────────────────────────────────────────────

/// Context passed to `PreviewProvider::preview`.
///
/// Carries shared resources that providers may need without storing them on
/// `AppState` directly. Phase 5 adds `worker_tx` and `generation` so async
/// providers can tag their results with the current generation counter and
/// send them back over the shared worker channel.
#[derive(Debug)]
pub struct PreviewCtx {
    /// Whether hidden entries should appear in directory previews.
    pub show_hidden: bool,
    /// Sender half of the shared worker channel. Async providers use this to
    /// send `WorkerMsg::Preview` / `WorkerMsg::ImageMeta` results back to the
    /// UI thread.
    pub worker_tx: mpsc::Sender<WorkerMsg>,
    /// The generation counter at the time the preview was requested.
    ///
    /// Async providers tag their `WorkerMsg` with this value; `workers::merge`
    /// drops results whose generation no longer matches `state.preview.generation`.
    pub generation: u64,
    /// Maximum file size to preview synchronously on the UI thread.
    #[allow(dead_code)]
    pub text_sync_threshold_bytes: usize,
    /// Maximum number of lines a text preview loads, from `[preview] max_lines`.
    ///
    /// The preview scrolls within this window; a file with more lines is
    /// reported as truncated rather than silently cut. Read when the preview is
    /// requested, so a `:set` of the key takes effect from the next preview on.
    pub max_preview_lines: usize,
}

// ── PreviewProvider trait ─────────────────────────────────────────────────────

/// Contract for all preview providers.
///
/// A provider can handle a subset of entries (e.g. text files, images) and
/// produces `PreviewOutcome::Ready` for synchronous content or
/// `PreviewOutcome::Deferred` when it kicks off a worker task.
///
/// Implementations are `Send + Sync` so the registry can be constructed
/// once on the UI thread and shared across its lifetime.
pub trait PreviewProvider: Send + Sync {
    /// Returns `true` if this provider can preview `entry`.
    fn can_handle(&self, entry: &Entry) -> bool;

    /// Produces a preview for `entry`.
    ///
    /// May return `PreviewOutcome::Deferred` if it spawns a worker; otherwise
    /// returns `PreviewOutcome::Ready(content)`.
    ///
    /// Implementations on the synchronous path must not perform blocking I/O
    /// beyond small/fast reads (a few hundred KB). Larger reads must go
    /// through the worker pool.
    fn preview(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome;
}

// ── PreviewRegistry ───────────────────────────────────────────────────────────

/// Ordered list of `PreviewProvider`s; first match wins.
///
/// Built once at startup via `register_defaults()` (and extended by plugins
/// in Phase 8). The main loop calls `preview_for` on every selection change.
pub struct PreviewRegistry {
    providers: Vec<Box<dyn PreviewProvider>>,
}

impl PreviewRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Appends `provider` to the end of the ordered list.
    ///
    /// Providers registered later have lower priority (first-match wins).
    pub fn register(&mut self, provider: Box<dyn PreviewProvider>) {
        self.providers.push(provider);
    }

    /// Finds the first registered provider that can handle `entry` and
    /// returns its `PreviewOutcome`.
    ///
    /// Returns `PreviewOutcome::Ready(PreviewContent::Empty)` if no provider
    /// matches (e.g. an empty directory or an unknown type).
    pub fn preview_for(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome {
        for provider in &self.providers {
            if provider.can_handle(entry) {
                return provider.preview(entry, ctx);
            }
        }
        PreviewOutcome::Ready(PreviewContent::Empty)
    }
}

impl Default for PreviewRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_leaves_ordinary_text_alone() {
        assert_eq!(sanitize("fn main() { todo!() }"), "fn main() { todo!() }");
        // Not control characters: the terminal draws these.
        assert_eq!(sanitize("héllo — wörld"), "héllo — wörld");
    }

    /// The artifact this removes: an ESC in a file that `content_inspector`
    /// calls text reached the terminal as an escape sequence and painted
    /// wherever it liked.
    #[test]
    fn sanitize_defuses_escape_sequences() {
        let attack = "before\u{1b}[2J\u{1b}[Hafter";
        let safe = sanitize(attack);
        assert!(!safe.contains('\u{1b}'), "no escape may survive: {safe:?}");
        assert_eq!(safe, "before·[2J·[Hafter");
    }

    #[test]
    fn sanitize_replaces_carriage_returns_and_bells() {
        // A CR would send the cursor back to the start of the row mid-pane.
        assert_eq!(sanitize("a\rb\u{7}c"), "a·b·c");
    }

    #[test]
    fn sanitize_expands_tabs_so_the_terminal_cannot_choose() {
        assert_eq!(sanitize("\tindented"), "    indented");
        assert_eq!(sanitize("a\tb").chars().count(), 1 + TAB_WIDTH + 1);
    }
}
