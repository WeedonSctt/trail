//! `PreviewProvider` trait and `PreviewRegistry`.
//!
//! Defines the contract that all preview providers implement, and the
//! registry that dispatches preview requests to the first matching provider.
//! New entry types (PDF, archive, etc.) implement `PreviewProvider` and
//! register an instance at startup — the core loop and render code do not
//! change.

use std::path::PathBuf;

use thiserror::Error;

use crate::app::state::Entry;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors from preview operations.
#[allow(dead_code)] // TODO(phase-5): Used by binary and image providers
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

/// The renderable content for the preview pane.
///
/// Each variant carries the data needed by `ui/preview_panel.rs` to draw
/// the pane without any further I/O. Phase 5 adds `Highlighted`, `Binary`,
/// and `Image` variants; for now only `Text` and `Directory` are produced.
#[derive(Debug, Clone, Default)]
pub enum PreviewContent {
    /// A placeholder shown before any entry is selected or while loading.
    #[default]
    Empty,

    /// A placeholder shown while an async worker result is in flight.
    ///
    /// Phase 5 uses this while `workers/highlight.rs` is running.
    Loading,

    /// Plain-text preview with pre-formatted lines.
    ///
    /// Each element is a formatted string (e.g. `" 1  first line of file"`).
    Text(Vec<String>),

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
    /// merges in (Phase 4/5).
    #[allow(dead_code)] // TODO(phase-4): Returned when workers are spawned
    Deferred,
}

// ── PreviewCtx ────────────────────────────────────────────────────────────────

/// Context passed to `PreviewProvider::preview`.
///
/// Carries shared resources that providers may need without storing them
/// on `AppState` directly.
#[derive(Debug)]
pub struct PreviewCtx {
    /// Whether hidden entries should appear in directory previews.
    pub show_hidden: bool,
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
