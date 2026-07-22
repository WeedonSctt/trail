//! Trail library crate.
//!
//! Exposes the public module API so that integration tests in `tests/` can
//! import and exercise state, actions, input, and preview logic without
//! running the binary.
//!
//! The binary entry point is `src/main.rs` and adds only the terminal-
//! lifecycle glue on top of this library.

#![forbid(unsafe_code)]

pub mod actions;
pub mod app;
pub mod cli;
pub mod config;
pub mod input;
pub mod plugin;
pub mod preview;
pub mod session;
pub mod ui;
pub mod workers;
