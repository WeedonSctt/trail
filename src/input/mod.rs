//! Input handling: dispatches keystrokes by current mode.
//!
//! Routes key events to the appropriate handler based on `state.mode`:
//! Navigation → `keymap::navigation`, Search → `keymap::search`,
//! Command → `command_parser::feed` (Phase 3).

pub mod command_parser;
pub mod keymap;

use crossterm::event::{KeyEvent, KeyEventKind};

use crate::actions::Action;
use crate::app::state::AppState;

/// Mutable context shared with the keymap across ticks.
///
/// Holds any state that spans multiple key events (e.g. multi-key sequences)
/// without polluting `AppState` for concerns that are purely input-layer.
///
/// Multi-key sequences such as `gg`, `ya`, `yr`, `yn`, and `dd` are handled
/// through `AppState::pending_nav_key` (which the status bar can render) rather
/// than `InputCtx` fields, so this struct is currently empty. It is kept as a
/// named type so callers do not need updating if input-layer state is added
/// in the future.
#[derive(Debug, Default)]
pub struct InputCtx {}

/// Dispatches `key` to the appropriate mode handler.
///
/// Returns `Some(Action)` when the key maps to an action, `None` otherwise.
/// The caller is responsible for calling `actions::apply` with the returned
/// action and handling any resulting error.
///
/// Only `KeyEventKind::Press` events produce actions; `Release` and `Repeat`
/// are ignored so crossterm's Windows double-fire doesn't double-process.
pub fn dispatch(key: KeyEvent, state: &AppState, ctx: &mut InputCtx) -> Option<Action> {
    // Ignore non-press events (crossterm fires Press + Release on Windows).
    if key.kind != KeyEventKind::Press {
        return None;
    }

    use crate::app::mode::Mode;
    match &state.mode {
        Mode::Navigation => keymap::navigation(key, ctx, state),

        Mode::Search { .. } => keymap::search(key, &state.config.keymap),

        Mode::Command { .. } => {
            // Delegate entirely to the command_parser feed path via CommandKey.
            // The actual buffer manipulation and submit/cancel logic is handled
            // in actions::apply(Action::CommandKey) so it stays testable.
            Some(Action::CommandKey(key))
        }
    }
}
