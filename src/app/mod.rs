//! Application state management.
//!
//! Owns the core data model: current directory, selection, mode, navigation
//! history, tabs, and the dirty flag that drives rendering.

pub mod history;
pub mod mode;
pub mod state;
pub mod tabs;
