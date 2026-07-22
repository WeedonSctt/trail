//! Filesystem watcher.
//!
//! Wraps `notify`, watching `state.cwd` only, re-subscribing on directory
//! change. Debounces bursts into a single `WorkerMsg::FsChanged` after a
//! quiet window (default 200ms, configurable via Phase 7).
// TODO(phase-4): Implement notify wrapper with debounce and re-subscribe.
