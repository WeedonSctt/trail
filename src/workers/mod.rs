//! Async worker pool: `WorkerMsg` enum, spawn/dispatch helpers, mpsc plumbing.
//!
//! Workers do anything that could be slow (git status, filesystem watching,
//! syntax highlighting, image decoding) and report results back to the UI
//! thread over a single `mpsc` channel drained once per UI tick.
//!
//! Architecture contract (§4 of `trail_architecture.md`):
//! - Every task sends its result as a `WorkerMsg` over the shared `Sender`.
//! - Every `WorkerMsg::Preview` / `WorkerMsg::ImageMeta` carries the
//!   `generation` it was spawned for; `merge()` compares it against
//!   `state.preview.generation` and drops the message if they don't match.
//! - Workers never touch `ratatui`/`crossterm` state directly.

pub mod fswatch;
pub mod git;
pub mod highlight;
pub mod image_decode;

// Re-export the git cache type so callers don't need to reach into the git
// submodule directly.
pub use git::GitCache;

use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::app::state::{AppState, GitDirState, GitFileStatus};
use crate::preview::provider::PreviewContent;

// ── Channel helpers ───────────────────────────────────────────────────────────

/// Creates the bounded `(Sender, Receiver)` pair used between the worker pool
/// and the UI thread.
///
/// The bound of 64 is generous enough to absorb short bursts (e.g. rapid
/// navigation through a large directory) without blocking worker tasks, while
/// still providing back-pressure if the UI thread falls behind.
pub fn channel() -> (mpsc::Sender<WorkerMsg>, mpsc::Receiver<WorkerMsg>) {
    mpsc::channel(64)
}

// ── WorkerMsg ─────────────────────────────────────────────────────────────────

/// All messages that worker tasks can send back to the UI thread.
///
/// The single-enum design (one channel, one receiver) was decided in the
/// implementation plan's Decision Log over per-domain channels, to keep the
/// event loop `select!` simple and avoid priority inversion.
#[derive(Debug)]
pub enum WorkerMsg {
    /// Git repository state for `path` has been computed.
    ///
    /// `file_statuses` maps entry filenames to their git status for the
    /// current directory. Both fields are `None` when the path is not inside
    /// a git repository.
    Git {
        /// The directory this result is for.
        path: PathBuf,
        /// Repository-level state (branch, dirty flag); `None` outside a repo.
        state: Option<GitDirState>,
        /// Per-file status map: filename → `GitFileStatus`.
        file_statuses: Vec<(String, GitFileStatus)>,
    },

    /// The filesystem watcher detected a change in `path` after the debounce
    /// window elapsed.
    FsChanged {
        /// The directory where changes were detected.
        path: PathBuf,
    },

    /// An async preview result is ready.
    ///
    /// `generation` must match `state.preview.generation` or the message is
    /// dropped by `merge()` — this is the generation-guard invariant.
    ///
    /// The path this preview is for (additional staleness guard).
    #[allow(dead_code)]
    Preview {
        /// The generation counter at the time the preview was requested.
        generation: u64,
        /// The path this preview is for (additional staleness guard).
        path: PathBuf,
        /// The rendered content.
        content: PreviewContent,
    },

    /// Image metadata (dimensions, format) is ready.
    ///
    /// Like `Preview`, guarded by `generation`.
    #[allow(dead_code)]
    ImageMeta {
        /// The generation counter at the time the metadata request was made.
        generation: u64,
        /// The path this metadata is for.
        path: PathBuf,
        /// The rendered metadata content.
        content: PreviewContent,
    },
}

// ── merge ─────────────────────────────────────────────────────────────────────

/// Applies a `WorkerMsg` to `state`, enforcing the generation-guard for
/// preview-related messages.
///
/// # Generation-guard invariant
///
/// Every time the user moves to a new entry, `state.preview.generation` is
/// incremented. Workers tag their results with the generation they were
/// spawned for. If `msg.generation != state.preview.generation` by the time
/// the result arrives, the selection has already moved on and the result is
/// silently dropped — never rendered.
///
/// This prevents stale previews from a slow worker for entry N appearing
/// while the user is already viewing entry N+k.
///
/// Callers should call this once per UI tick for each message drained from the
/// worker channel.
pub fn merge(msg: WorkerMsg, state: &mut AppState) {
    match msg {
        WorkerMsg::Git {
            path,
            state: git_state,
            file_statuses,
        } => {
            // Only apply if the result is for the directory we are currently in.
            if path != state.cwd {
                tracing::debug!(
                    ?path,
                    cwd = ?state.cwd,
                    "dropping stale Git result for different directory"
                );
                return;
            }

            state.git = git_state;

            // Apply per-file statuses to the matching entries.
            for entry in state.entries.iter_mut() {
                let status = file_statuses
                    .iter()
                    .find(|(name, _)| *name == entry.file_name)
                    .map(|(_, s)| s.clone());
                entry.git_status = status;
            }

            state.dirty = true;
            tracing::debug!(?path, "merged Git worker result");
        }

        WorkerMsg::FsChanged { path } => {
            // Handled by the event loop: the loop receives FsChanged, refreshes,
            // and re-spawns git. This arm is included for completeness but the
            // event loop treats FsChanged as a special case and calls
            // state.refresh() directly.
            tracing::debug!(?path, "FsChanged received in merge (handled by event loop)");
        }

        WorkerMsg::Preview {
            generation,
            path,
            content,
        } => {
            // Generation-guard: drop if the selection has moved on.
            if generation != state.preview.generation {
                tracing::debug!(
                    generation,
                    current = state.preview.generation,
                    "dropping stale Preview result (generation mismatch)"
                );
                return;
            }
            // Secondary path guard.
            if path != state.preview.for_path {
                tracing::debug!(
                    ?path,
                    current_path = ?state.preview.for_path,
                    "dropping stale Preview result (path mismatch)"
                );
                return;
            }
            state.preview.content = content;
            state.dirty = true;
            tracing::debug!(?path, generation, "merged Preview worker result");
        }

        WorkerMsg::ImageMeta {
            generation,
            path,
            content,
        } => {
            // Same generation-guard as Preview.
            if generation != state.preview.generation {
                tracing::debug!(
                    generation,
                    current = state.preview.generation,
                    "dropping stale ImageMeta result (generation mismatch)"
                );
                return;
            }
            if path != state.preview.for_path {
                tracing::debug!(
                    ?path,
                    current_path = ?state.preview.for_path,
                    "dropping stale ImageMeta result (path mismatch)"
                );
                return;
            }
            state.preview.content = content;
            state.dirty = true;
            tracing::debug!(?path, generation, "merged ImageMeta worker result");
        }
    }
}


