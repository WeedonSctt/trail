//! Git status worker.
//!
//! Computes repo indicator, branch name, and optional per-file status.
//! Results are cached per directory and invalidated on `FsChanged` events.
// TODO(phase-4): Implement spawn_git_status(path, tx).
