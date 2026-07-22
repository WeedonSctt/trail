//! Core application state: `AppState`, `Entry`, `EntryKind`.
//!
//! The central data model that every other module reads or mutates.
//! `state.dirty` is the invariant that drives the render loop: every
//! mutation sets it, every render clears it.
// TODO(phase-1): Define AppState, Entry, EntryKind, PreviewSlot, FilterState,
//                GitDirState, StatusBarState, and their impls.
