//! The integration tests, as one test program.
//!
//! Each file directly under `tests/` would be its own executable, statically
//! linking all of Trail and its dependencies, so a change relinks every one and
//! each takes its own space in `target/`. `Cargo.toml` therefore sets
//! `autotests = false` and every suite is a module here instead. A new suite
//! that is not declared below is never compiled or run.
//!
//! Run one suite with `cargo test --test integration state_tests::`.
//!
//! Sharing a process means sharing process-wide state, which separate
//! executables hid. A test that changes such state takes the lock for it here.

use std::sync::{Mutex, MutexGuard, PoisonError};

mod bookmark_dest_tests;
mod command_parser_tests;
mod command_repeat_tests;
mod config_write_tests;
mod install_script_tests;
mod jump_back_tests;
mod marks_tests;
mod preview_tests;
mod render_snapshot_tests;
mod run_report_tests;
mod state_tests;
mod tab_panel_tests;
mod terminal_panel_tests;
mod test_gg_sequence;

/// Serialises the tests that touch `preview::graphics`.
///
/// The protocol and cell size are process-wide, so two image tests running
/// concurrently would clobber each other's configuration.
static GRAPHICS: Mutex<()> = Mutex::new(());

/// Takes [`GRAPHICS`], recovering rather than propagating poisoning so that one
/// failing image test does not cascade into the others.
fn graphics_lock() -> MutexGuard<'static, ()> {
    GRAPHICS.lock().unwrap_or_else(PoisonError::into_inner)
}
