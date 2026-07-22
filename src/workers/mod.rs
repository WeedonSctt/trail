//! Async worker pool: `WorkerMsg` enum, spawn/dispatch helpers, mpsc plumbing.
//!
//! Workers do anything that could be slow (git status, filesystem watching,
//! syntax highlighting, image decoding) and report results back to the UI
//! thread over a single `mpsc` channel.
// TODO(phase-4): Define WorkerMsg enum, set up mpsc channel, implement merge().

pub mod fswatch;
pub mod git;
pub mod highlight;
pub mod image_decode;
