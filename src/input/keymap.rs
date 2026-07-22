//! Default and user-configured key bindings.
//!
//! Maps key events to `Action` values for each mode. Phase 7 adds TOML-driven
//! overrides; until then, only the hardcoded defaults are active.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::actions::Action;
use crate::app::state::AppState;
use crate::input::InputCtx;

/// Translates a `KeyEvent` in Navigation Mode into an `Action`.
///
/// Returns `None` if the key is not bound in Navigation Mode, so the caller
/// can decide whether to swallow or ignore it.
///
/// # Multi-key sequences
///
/// The following multi-key sequences are supported:
///
/// | Sequence | Action |
/// |----------|--------|
/// | `gg`     | Jump to top |
/// | `ya`     | Copy absolute path |
/// | `yr`     | Copy relative path |
/// | `yn`     | Copy filename |
/// | `dd`     | Delete selected (with confirmation) |
///
/// `pending_g` (in `InputCtx`) handles the `g g` prefix.
/// `state.pending_nav_key` handles `y*` and `dd` prefixes.
///
/// When `pending_delete` is set, `Enter` confirms and `Esc` cancels the
/// delete without entering this function's main branch (the event loop in
/// `main.rs` calls `apply` directly in that case).
pub fn navigation(key: KeyEvent, ctx: &mut InputCtx, state: &AppState) -> Option<Action> {
    // ── Pending-delete confirmation ───────────────────────────────────────────
    // While a delete is pending, only Enter (confirm) and Esc (cancel) are
    // meaningful. All other keys are swallowed to avoid accidental navigation.
    if state.pending_delete {
        return match key.code {
            KeyCode::Enter | KeyCode::Char('y') => Some(Action::ConfirmDelete),
            KeyCode::Esc | KeyCode::Char('n') => Some(Action::CancelDelete),
            _ => None,
        };
    }

    // ── Multi-key: `g g` (jump top) ──────────────────────────────────────────
    if ctx.pending_g {
        ctx.pending_g = false;
        return match key.code {
            KeyCode::Char('g') => Some(Action::JumpTop),
            // Any other key after `g` is consumed but produces no action.
            _ => None,
        };
    }

    // ── Multi-key: `y a` / `y r` / `y n` and `d d` ──────────────────────────
    if let Some(pending) = state.pending_nav_key {
        // A pending_nav_key is consumed by the next keypress regardless of
        // whether it forms a recognised sequence.
        return match (pending, key.code) {
            ('y', KeyCode::Char('a')) => Some(Action::CopyAbsPath),
            ('y', KeyCode::Char('r')) => Some(Action::CopyRelPath),
            ('y', KeyCode::Char('n')) => Some(Action::CopyFilename),
            ('d', KeyCode::Char('d')) => Some(Action::BeginDelete),
            // Unrecognised second key: consume without action.
            _ => None,
        };
        // Note: clearing `pending_nav_key` is done in `main.rs` after `apply`,
        // because we need to distinguish "second key consumed" from
        // "still waiting". The caller sets it via `SetPendingNavKey`.
    }

    match key.code {
        // Movement
        KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Char('G') => Some(Action::JumpBottom),

        // `g` — start of multi-key sequence (`gg` = jump top)
        KeyCode::Char('g') => {
            ctx.pending_g = true;
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

        // Multi-key prefix keys: `y` (clipboard) and `d` (delete).
        // These set `state.pending_nav_key`; the next key resolves the sequence.
        // We return a special action to signal the prefix.
        KeyCode::Char('y') => Some(Action::SetPendingNavKey('y')),
        KeyCode::Char('d') => Some(Action::SetPendingNavKey('d')),

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

/// Translates a `KeyEvent` in Search Mode into an `Action`.
///
/// Key binding summary:
/// - **Printable characters** — appended to the query via `SearchAppendChar`.
/// - **`Backspace` / `Ctrl-h`** — remove the last character from the query.
/// - **`j` / `Down`** — move the filtered-list selection down.
/// - **`k` / `Up`** — move the filtered-list selection up.
/// - **`Enter` / `l` / `Right`** — confirm: enter a directory or exit Search
///   Mode for a file (file open is Phase 6).
/// - **`Esc`** — exit Search Mode, restoring the full, unfiltered listing.
///
/// Returns `None` for unbound keys (e.g. modifier-only chords), allowing the
/// caller to silently swallow them.
pub fn search(key: KeyEvent) -> Option<Action> {
    match key.code {
        // Exit Search Mode, restoring the full unfiltered listing.
        KeyCode::Esc => Some(Action::ExitMode),

        // Confirm the current selection (enter dir or exit mode for files).
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => Some(Action::SearchConfirm),

        // Navigate within the filtered list.
        KeyCode::Char('j') | KeyCode::Down => Some(Action::SearchMoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::SearchMoveUp),

        // Delete the last character from the query.
        KeyCode::Backspace => Some(Action::SearchDeleteChar),
        // Ctrl-h is the traditional terminal backspace alias.
        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::SearchDeleteChar)
        }

        // Append any printable character to the query (no modifier, or Shift
        // for uppercase — explicitly exclude Ctrl chords so e.g. Ctrl-c is
        // not treated as a character append).
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::SearchAppendChar(ch))
        }

        _ => None,
    }
}
