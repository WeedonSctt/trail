//! Action system: the `Action` enum and `apply(action, state)`.
//!
//! Every user-initiated mutation flows through an `Action` value, keeping
//! the state machine testable independently of input handling.

pub mod clipboard;
pub mod fs_ops;
pub mod shell_exec;

use crate::app::state::{AppState, StateError};

/// Every user-initiated state change is represented as one of these variants.
///
/// Phase 1 implements the navigation actions; later phases add filesystem
/// mutations, clipboard, and shell execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    // ── Navigation ──────────────────────────────────────────────────────────
    /// Move the selection cursor down one row.
    MoveDown,
    /// Move the selection cursor up one row.
    MoveUp,
    /// Jump the selection to the first entry.
    JumpTop,
    /// Jump the selection to the last entry.
    JumpBottom,
    /// Enter the selected directory (or open file in editor — stub until Phase 6).
    EnterOrOpen,
    /// Navigate to the parent directory.
    GoParent,
    /// Navigate back in the directory history (`u`).
    HistoryBack,
    /// Navigate forward in the directory history (`Ctrl-r`).
    HistoryForward,
    /// Reload the current directory listing.
    Refresh,
    /// Toggle visibility of hidden files.
    ToggleHidden,

    // ── Mode transitions ─────────────────────────────────────────────────────
    /// Enter Search Mode (Phase 2 wires the actual filter logic).
    EnterSearch,
    /// Enter Command Mode (Phase 3 wires the actual command parser).
    EnterCommand,
    /// Exit the current mode, returning to Navigation.
    ExitMode,

    // ── Quit ─────────────────────────────────────────────────────────────────
    /// Quit the application normally (writes `--cwd-file` in Phase 6).
    Quit,
}

/// Applies `action` to `state`, returning an error if a filesystem operation
/// fails.
///
/// This is the single entry point for all state mutations from the UI thread.
/// Callers should call this rather than mutating `AppState` directly, so that
/// tests can drive state through `Action` values without a running terminal.
///
/// # Errors
///
/// Returns [`StateError`] if a navigation or directory-loading action fails.
pub fn apply(action: Action, state: &mut AppState) -> Result<(), StateError> {
    match action {
        Action::MoveDown => state.move_down(),
        Action::MoveUp => state.move_up(),
        Action::JumpTop => state.jump_top(),
        Action::JumpBottom => state.jump_bottom(),

        Action::EnterOrOpen => {
            if let Some(entry) = state.selected_entry().cloned() {
                use crate::app::state::EntryKind;
                match entry.kind {
                    EntryKind::Dir => {
                        state.enter_dir(entry.path)?;
                    }
                    EntryKind::File | EntryKind::Symlink => {
                        // TODO(phase-6): Open file in $EDITOR via shell_exec::run_external.
                        // For Phase 1 this is intentionally a no-op for files.
                    }
                }
            }
        }

        Action::GoParent => {
            state.go_parent()?;
        }

        Action::HistoryBack => {
            state.history_back()?;
        }

        Action::HistoryForward => {
            state.history_forward()?;
        }

        Action::Refresh => {
            state.refresh()?;
        }

        Action::ToggleHidden => {
            state.toggle_hidden()?;
        }

        // Mode transitions — functional wiring in Phase 2 / Phase 3.
        Action::EnterSearch => {
            use crate::app::mode::Mode;
            state.mode = Mode::Search {
                query: String::new(),
                matches: Vec::new(),
            };
            state.dirty = true;
        }

        Action::EnterCommand => {
            use crate::app::mode::Mode;
            state.mode = Mode::Command {
                buffer: String::new(),
                cursor: 0,
                history_index: None,
            };
            state.dirty = true;
        }

        Action::ExitMode => {
            use crate::app::mode::Mode;
            if state.mode != Mode::Navigation {
                state.mode = Mode::Navigation;
                state.filter = None;
                state.dirty = true;
            }
        }

        Action::Quit => {
            // Handled by the event loop checking the return value of dispatch;
            // nothing to do here at the state level.
        }
    }
    Ok(())
}
