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
#[derive(Debug, Default)]
pub struct InputCtx {
    /// `true` after the first `g` is pressed in Navigation Mode, waiting for
    /// the second `g` to complete the `gg` (jump-to-top) sequence.
    pub pending_g: bool,
}

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

        Mode::Search { .. } => keymap::search(key),

        Mode::Command { .. } => {
            // Delegate entirely to the command_parser feed path via CommandKey.
            // The actual buffer manipulation and submit/cancel logic is handled
            // in actions::apply(Action::CommandKey) so it stays testable.
            Some(Action::CommandKey(key))
        }
    }
}
