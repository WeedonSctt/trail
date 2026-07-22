//! Default and user-configured key bindings.
//!
//! Maps key events to `Action` values for each mode. Phase 7 adds TOML-driven
//! overrides; until then, only the hardcoded defaults are active.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::actions::Action;

/// Translates a `KeyEvent` in Navigation Mode into an `Action`.
///
/// Returns `None` if the key is not bound in Navigation Mode, so the caller
/// can decide whether to swallow or ignore it.
///
/// The `pending_g` flag is used to implement the two-key `gg` binding
/// (jump to top): when the first `g` is pressed `pending_g` is set to
/// `true`; if the next key is also `g`, `JumpTop` is returned; any other
/// key clears the flag without producing an action.
///
/// # Multi-key sequences
///
/// The only multi-key sequence in Navigation Mode for Phase 1 is `g g`
/// (jump top). `G` (uppercase) is a single-key binding for jump bottom.
pub fn navigation(key: KeyEvent, pending_g: &mut bool) -> Option<Action> {
    // Handle the second key of the `gg` sequence.
    if *pending_g {
        *pending_g = false;
        return match key.code {
            KeyCode::Char('g') => Some(Action::JumpTop),
            // Any other key after `g` is consumed but produces no action.
            _ => None,
        };
    }

    match key.code {
        // Movement
        KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Char('G') => Some(Action::JumpBottom),

        // `g` — start of multi-key sequence (`gg` = jump top)
        KeyCode::Char('g') => {
            *pending_g = true;
            None
        }

        // Directory navigation
        KeyCode::Char('l') | KeyCode::Enter | KeyCode::Right => Some(Action::EnterOrOpen),
        KeyCode::Char('h') | KeyCode::Backspace | KeyCode::Left => Some(Action::GoParent),

        // History
        KeyCode::Char('u') => Some(Action::HistoryBack),
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::HistoryForward)
        }

        // Reload
        KeyCode::Char('R') => Some(Action::Refresh),

        // Hidden-file toggle
        KeyCode::Char('.') => Some(Action::ToggleHidden),

        // Mode transitions
        KeyCode::Char('/') => Some(Action::EnterSearch),
        KeyCode::Char(':') => Some(Action::EnterCommand),

        // Quit
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),

        // Escape in Navigation Mode is a no-op (nothing to cancel).
        KeyCode::Esc => None,

        _ => None,
    }
}
