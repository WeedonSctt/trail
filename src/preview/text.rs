//! Text file preview provider.
//!
//! Synchronous preview showing raw content with line numbers. Phase 1 shows
//! plain text only; Phase 5 upgrades this to `syntect`-based highlighting
//! for files under the size threshold.
// TODO(phase-1): Implement synchronous text preview (raw content + line numbers).
// TODO(phase-5): Add syntect-based syntax highlighting.
