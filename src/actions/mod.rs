//! Action system: the `Action` enum and `apply(action, state)`.
//!
//! Every user-initiated mutation flows through an `Action` value, keeping
//! the state machine testable independently of input handling.
// TODO(phase-1): Define the Action enum and apply() dispatcher.

pub mod clipboard;
pub mod fs_ops;
pub mod shell_exec;
