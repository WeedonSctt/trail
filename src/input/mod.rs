//! Input handling: dispatches keystrokes by current mode.
//!
//! Routes key events to the appropriate handler based on `state.mode`:
//! Navigation → `keymap::navigation`, Search → `keymap::search`,
//! Command → `command_parser::feed`.
// TODO(phase-1): Implement dispatch(key, state, ctx) -> Option<Action>.

pub mod command_parser;
pub mod keymap;
