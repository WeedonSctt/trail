//! Syntax highlighting worker.
//!
//! Highlights text files off-thread for files over `TEXT_SYNC_THRESHOLD`.
//! Small files are highlighted synchronously in `preview/text.rs`.
// TODO(phase-5): Implement spawn_highlight(path, generation, tx).
