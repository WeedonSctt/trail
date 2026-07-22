//! Git status worker.
//!
//! Computes repo indicator, branch name, and optional per-file status.
//! Results are cached per directory and invalidated on `FsChanged` events.
//!
//! Spawns a Tokio task that calls `gix` synchronously inside `spawn_blocking`
//! so the UI thread is never blocked. Sends a `WorkerMsg::Git` back over the
//! provided sender when done.
//!
//! # Feature dependencies
//!
//! The `gix` dependency must include the `status` feature (see `Cargo.toml`)
//! for `Repository::is_dirty()` to compile.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::Sender;
use tokio::task;

use crate::app::state::{GitDirState, GitFileStatus};
use crate::workers::WorkerMsg;

// ── Cache ─────────────────────────────────────────────────────────────────────

/// A cached git result for a directory.
#[derive(Debug, Clone)]
pub struct CachedGit {
    dir_state: Option<GitDirState>,
    file_statuses: Vec<(String, GitFileStatus)>,
}

/// Shared git cache keyed by canonical directory path.
///
/// Invalidated when a `WorkerMsg::FsChanged` arrives for the same path.
/// `Arc<Mutex<…>>` lets the main thread own the cache while worker tasks
/// populate it.
pub type GitCache = Arc<Mutex<HashMap<PathBuf, CachedGit>>>;

/// Creates a new, empty git cache.
pub fn new_cache() -> GitCache {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Removes the cached git result for `path`, forcing a recompute on the next
/// `spawn_git_status` call.
///
/// Called when `WorkerMsg::FsChanged` arrives for the watched directory.
pub fn invalidate(cache: &GitCache, path: &Path) {
    if let Ok(mut c) = cache.lock() {
        c.remove(path);
        tracing::debug!(?path, "git cache invalidated");
    }
}

// ── spawn_git_status ──────────────────────────────────────────────────────────

/// Spawns a Tokio task that computes git status for `path` and sends the
/// result as a `WorkerMsg::Git` over `tx`.
///
/// If the result is already in `cache` it is sent immediately without
/// spawning a new blocking task.
///
/// The task is fire-and-forget; the caller does not await it. If `tx` is
/// closed (the UI thread has exited), the result is silently dropped.
pub fn spawn_git_status(path: PathBuf, tx: Sender<WorkerMsg>, cache: GitCache) {
    // Check the cache before spawning a blocking task.
    if let Ok(c) = cache.lock() {
        if let Some(cached) = c.get(&path) {
            let msg = WorkerMsg::Git {
                path: path.clone(),
                state: cached.dir_state.clone(),
                file_statuses: cached.file_statuses.clone(),
            };
            let tx2 = tx.clone();
            tokio::spawn(async move {
                // Ignore send error: UI thread may have exited.
                let _ = tx2.send(msg).await;
            });
            return;
        }
    }

    // Not cached — compute in a blocking thread.
    tokio::spawn(async move {
        let path2 = path.clone();
        let result = task::spawn_blocking(move || compute_git_status(&path2)).await;

        let (dir_state, file_statuses) = match result {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("git worker task panicked: {e}");
                return;
            }
        };

        // Populate the cache.
        if let Ok(mut c) = cache.lock() {
            c.insert(
                path.clone(),
                CachedGit {
                    dir_state: dir_state.clone(),
                    file_statuses: file_statuses.clone(),
                },
            );
        }

        let _ = tx
            .send(WorkerMsg::Git {
                path,
                state: dir_state,
                file_statuses,
            })
            .await;
    });
}

// ── Computation (runs inside spawn_blocking) ──────────────────────────────────

/// Computes the git status for `path` synchronously.
///
/// Returns `(None, [])` when `path` is not inside a git repository, or when
/// the repository cannot be opened (e.g. permissions, corrupt objects).
fn compute_git_status(path: &Path) -> (Option<GitDirState>, Vec<(String, GitFileStatus)>) {
    // Walk up from `path` looking for a `.git` directory.
    let repo = match gix::discover(path) {
        Ok(r) => r,
        Err(_) => {
            tracing::debug!(?path, "no git repository found");
            return (None, vec![]);
        }
    };

    // Resolve HEAD → branch name.
    let branch = match repo.head_name() {
        Ok(Some(name)) => name.shorten().to_string(),
        Ok(None) => "HEAD".to_owned(), // detached HEAD
        Err(e) => {
            tracing::debug!(?path, "failed to read HEAD: {e}");
            "HEAD".to_owned()
        }
    };

    // Determine whether the working tree has uncommitted changes.
    let is_dirty = match repo.is_dirty() {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!(?path, "failed to check dirty state: {e}");
            false
        }
    };

    let dir_state = Some(GitDirState { branch, is_dirty });

    // Compute per-file statuses for entries in `path`.
    let file_statuses = compute_file_statuses(&repo, path);

    (dir_state, file_statuses)
}

/// Computes per-file git statuses for direct children of `dir`.
///
/// Only files immediately inside `dir` (non-recursive) are included, since
/// the nav panel only shows the current directory level.
///
/// This implementation uses a lightweight heuristic: it checks the index for
/// tracked files and marks files not found on disk as `Deleted`. Untracked
/// files remain unlisted (callers can interpret absent = untracked).
fn compute_file_statuses(repo: &gix::Repository, dir: &Path) -> Vec<(String, GitFileStatus)> {
    let workdir = match repo.work_dir() {
        Some(p) => p.to_owned(),
        None => return vec![], // bare repo
    };

    let index = match repo.index_or_empty() {
        Ok(i) => i,
        Err(e) => {
            tracing::debug!("failed to read git index: {e}");
            return vec![];
        }
    };

    let mut statuses: Vec<(String, GitFileStatus)> = Vec::new();

    for entry in index.entries() {
        // entry.path() returns the repo-relative path as raw bytes.
        let rel_path_bytes = entry.path(&index);
        let rel_path_str = match std::str::from_utf8(rel_path_bytes) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let abs_path = workdir.join(rel_path_str);

        // Only include direct children of `dir`.
        let parent = match abs_path.parent() {
            Some(p) => p,
            None => continue,
        };
        if parent != dir {
            continue;
        }

        let file_name = match abs_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };

        // Lightweight status: check whether the file exists on disk.
        let status = if abs_path.exists() {
            // For Phase 4 a shallow existence check is sufficient; Phase 7
            // can expose a config flag to enable full mtime/content comparison.
            GitFileStatus::Clean
        } else {
            GitFileStatus::Deleted
        };

        statuses.push((file_name, status));
    }

    statuses
}
