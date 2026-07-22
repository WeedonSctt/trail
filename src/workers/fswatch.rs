//! Filesystem watcher.
//!
//! Wraps `notify`, watching `state.cwd` only. Re-subscribes when the
//! directory changes. Debounces bursts of events (e.g. a `git checkout`) into
//! a single `WorkerMsg::FsChanged` after a quiet window.
//!
//! # Debounce
//!
//! Default quiet window: 200 ms (`DEFAULT_DEBOUNCE_MS`). This value matches
//! the implementation plan's decision log. Phase 7 will expose it as a
//! config key (`fs_watch_debounce_ms`).
//!
//! # Re-subscription
//!
//! Watching is directory-scoped: when the user navigates to a new directory,
//! the caller drops the `FsWatchHandle` to cancel the current watch and calls
//! `spawn_fswatch` again with the new path.

use std::path::PathBuf;
use std::time::Duration;

use notify::event::{EventKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio::time;

use crate::workers::WorkerMsg;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Default quiet-window for debouncing filesystem events (milliseconds).
///
/// Phase 7 will move this into the config schema as `fs_watch_debounce_ms`.
pub const DEFAULT_DEBOUNCE_MS: u64 = 200;

// ── FsWatchHandle ─────────────────────────────────────────────────────────────

/// An opaque handle to a running filesystem watch task.
///
/// Dropping this value cancels the underlying Tokio task, which in turn drops
/// the `notify` watcher. The UI thread should drop its current `FsWatchHandle`
/// before calling `spawn_fswatch` with a new path.
pub struct FsWatchHandle {
    /// The Tokio task running the watch loop.
    task: JoinHandle<()>,
}

impl FsWatchHandle {
    /// Cancels the watch task.
    #[allow(dead_code)]
    pub fn cancel(self) {
        self.task.abort();
    }
}

impl Drop for FsWatchHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// ── spawn_fswatch ─────────────────────────────────────────────────────────────

/// Spawns a Tokio task that watches `path` for filesystem changes and sends
/// `WorkerMsg::FsChanged` over `tx` after the debounce window elapses.
///
/// Returns an `FsWatchHandle` that the caller should drop when navigating to a
/// new directory. The task exits automatically when the handle is dropped.
///
/// If the watcher cannot be created (e.g. too many open file descriptors), the
/// error is logged at `debug` level and `None` is returned — the UI remains
/// functional, just without automatic refresh.
pub fn spawn_fswatch(
    path: PathBuf,
    tx: Sender<WorkerMsg>,
    debounce_ms: u64,
) -> Option<FsWatchHandle> {
    // Bridge: notify uses a synchronous callback; we bridge to Tokio via an
    // unbounded tokio mpsc so the callback is non-blocking (it just sends to
    // an in-memory queue that the async task drains).
    let (bridge_tx, mut bridge_rx) =
        tokio::sync::mpsc::unbounded_channel::<notify::Result<Event>>();

    let mut watcher = match RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            // Ignore send error: the task may have been cancelled.
            let _ = bridge_tx.send(res);
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            tracing::debug!(?path, "failed to create fs watcher: {e}");
            return None;
        }
    };

    // Watch only the current directory (non-recursive).
    if let Err(e) = watcher.watch(&path, RecursiveMode::NonRecursive) {
        tracing::debug!(?path, "failed to watch directory: {e}");
        return None;
    }

    let debounce = Duration::from_millis(debounce_ms);
    let watch_path = path.clone();

    let task = tokio::spawn(async move {
        // Keep the watcher alive for the duration of this task.
        // Dropping it here when the task is aborted removes the OS watch.
        let _watcher = watcher;

        loop {
            // Wait for the first event.
            let first = bridge_rx.recv().await;
            match first {
                None => break, // bridge_tx dropped (watcher task ended)
                Some(Ok(event)) if !is_relevant_event(&event) => {
                    continue; // skip access-only events
                }
                Some(Err(e)) => {
                    tracing::debug!(?watch_path, "fs watch error: {e}");
                    continue;
                }
                Some(Ok(_)) => {
                    // Relevant event received — start the debounce window.
                    // Drain any further events that arrive within the window.
                    let deadline = time::Instant::now() + debounce;
                    loop {
                        match time::timeout_at(deadline, bridge_rx.recv()).await {
                            Ok(Some(_)) => {
                                // More events within the window — reset is handled
                                // by just draining; the outer sleep covers the window.
                            }
                            Ok(None) => {
                                // bridge_tx dropped.
                                return;
                            }
                            Err(_) => {
                                // Debounce window elapsed with no more events.
                                break;
                            }
                        }
                    }

                    let msg = WorkerMsg::FsChanged {
                        path: watch_path.clone(),
                    };
                    if tx.send(msg).await.is_err() {
                        // Receiver dropped (UI thread exited) — stop watching.
                        break;
                    }
                    tracing::debug!(?watch_path, "FsChanged sent after debounce");
                }
            }
        }
    });

    Some(FsWatchHandle { task })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns `true` for filesystem events that should trigger a directory refresh.
///
/// Access-only events (reads) are excluded to avoid churning the UI when a
/// file is merely being read (e.g. during a syntax-highlight preview).
fn is_relevant_event(event: &Event) -> bool {
    match &event.kind {
        EventKind::Create(_)
        | EventKind::Remove(_)
        | EventKind::Modify(ModifyKind::Name(_))
        | EventKind::Modify(ModifyKind::Data(_))
        | EventKind::Modify(ModifyKind::Metadata(_)) => true,
        // Access events, Other, and Any are not relevant.
        _ => false,
    }
}
