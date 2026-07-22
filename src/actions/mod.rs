//! Action system: the `Action` enum and `apply(action, state)`.\n//!\n//! Every user-initiated mutation flows through an `Action` value, keeping\n//! the state machine testable independently of input handling.

pub mod clipboard;
pub mod fs_ops;
pub mod shell_exec;

use crate::app::state::{AppState, StateError};
use crate::input::command_parser::ParsedCommand;

/// Every user-initiated state change is represented as one of these variants.
///
/// Phase 1 implements the navigation actions; Phase 2 the search actions;
/// Phase 3 adds filesystem mutations, clipboard, and command execution.
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

    // ── Mode transitions ──────────────────────────────────────────────────
    /// Enter Search Mode (Phase 2 wires the actual filter logic).
    EnterSearch,
    /// Enter Command Mode (Phase 3 wires the actual command parser).
    EnterCommand,
    /// Exit the current mode, returning to Navigation.
    ExitMode,

    // ── Search Mode ───────────────────────────────────────────────────────
    /// Append `char` to the Search Mode query and re-run the fuzzy filter.
    SearchAppendChar(char),
    /// Delete the last character from the Search Mode query and re-run the
    /// fuzzy filter. No-op if the query is already empty.
    SearchDeleteChar,
    /// Move the filtered-list selection down by one row.
    SearchMoveDown,
    /// Move the filtered-list selection up by one row.
    SearchMoveUp,
    /// Confirm the current filtered selection: enter a directory or leave
    /// Search Mode if the selected entry is a file (file open is Phase 6).
    SearchConfirm,

    // ── Command Mode ──────────────────────────────────────────────────────
    /// Feed a single key event into the Command Mode buffer.
    CommandKey(crossterm::event::KeyEvent),

    // ── Filesystem mutations (Phase 3) ────────────────────────────────────
    /// Execute a validated, parsed command (dispatched after Command Mode submit).
    ExecuteCommand(ParsedCommand),
    /// Copy the absolute path of the selected entry to the yank buffer.
    CopyAbsPath,
    /// Copy the relative path of the selected entry to the yank buffer.
    CopyRelPath,
    /// Copy the filename of the selected entry to the yank buffer.
    CopyFilename,
    /// Begin the `dd` delete flow — sets `pending_delete = true`.
    BeginDelete,
    /// Confirm and execute the pending delete.
    ConfirmDelete,
    /// Cancel the pending delete confirmation.
    CancelDelete,
    /// Set `state.pending_nav_key` to begin a multi-key Navigation Mode
    /// sequence (`y` for clipboard, `d` for delete). The following key
    /// resolves the sequence in `keymap::navigation`.
    SetPendingNavKey(char),

    // ── Quit ─────────────────────────────────────────────────────────────
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

        // Mode transitions.
        Action::EnterSearch => {
            use crate::app::mode::Mode;
            state.mode = Mode::Search {
                query: String::new(),
                matches: Vec::new(),
            };
            // Entering Search Mode with an empty query shows all entries.
            state.apply_filter(String::new());
            state.dirty = true;
        }

        Action::EnterCommand => {
            use crate::app::mode::Mode;
            state.mode = Mode::Command {
                buffer: String::new(),
                cursor: 0,
                history_index: None,
            };
            state.error_message = None;
            state.dirty = true;
        }

        Action::ExitMode => {
            use crate::app::mode::Mode;
            if state.mode != Mode::Navigation {
                state.mode = Mode::Navigation;
                state.filter = None;
                state.pending_delete = false;
                state.error_message = None;
                state.pending_nav_key = None;
                state.dirty = true;
            }
        }

        // Search Mode actions.
        Action::SearchAppendChar(ch) => {
            use crate::app::mode::Mode;
            let new_query = if let Mode::Search { query, .. } = &state.mode {
                let mut q = query.clone();
                q.push(ch);
                q
            } else {
                return Ok(());
            };
            state.apply_filter(new_query);
        }

        Action::SearchDeleteChar => {
            use crate::app::mode::Mode;
            let new_query = if let Mode::Search { query, .. } = &state.mode {
                let mut q = query.clone();
                // Remove the last Unicode scalar (pop handles multi-byte chars).
                q.pop();
                q
            } else {
                return Ok(());
            };
            state.apply_filter(new_query);
        }

        Action::SearchMoveDown => {
            state.move_down();
        }

        Action::SearchMoveUp => {
            state.move_up();
        }

        Action::SearchConfirm => {
            use crate::app::mode::Mode;
            // Navigate into directory selections; leave mode for files
            // (actual file open is Phase 6).
            if let Some(entry) = state.selected_entry().cloned() {
                use crate::app::state::EntryKind;
                match entry.kind {
                    EntryKind::Dir => {
                        // Exit Search Mode first, then enter the directory.
                        state.mode = Mode::Navigation;
                        state.filter = None;
                        state.enter_dir(entry.path)?;
                    }
                    EntryKind::File | EntryKind::Symlink => {
                        // TODO(phase-6): Open file in $EDITOR.
                        // For now, just exit Search Mode.
                        state.mode = Mode::Navigation;
                        state.filter = None;
                        state.dirty = true;
                    }
                }
            }
        }

        // ── Command Mode ─────────────────────────────────────────────────────
        Action::CommandKey(key) => {
            use crate::app::mode::Mode;
            use crate::input::command_parser::{feed, FeedResult};

            // Extract buffer/cursor/history_index from the mode.
            let (buffer, cursor, history_index, is_shell) = if let Mode::Command {
                buffer,
                cursor,
                history_index,
            } = &mut state.mode
            {
                // Determine shell mode from the buffer's leading character.
                let is_shell = buffer.starts_with('!') || {
                    // Check if the raw key was '!' during initial entry.
                    // The mode's buffer is always the text after the sentinel,
                    // so we check the stored sentinel flag via the buffer prefix.
                    false
                };
                (buffer, cursor, history_index, is_shell)
            } else {
                return Ok(());
            };

            // We need owned copies to avoid borrow-checker issues when also
            // needing state for history/tab.
            let mut buf_owned = buffer.clone();
            let mut cur_owned = *cursor;
            let mut hist_owned = *history_index;
            let cwd = state.cwd.clone();

            let result = {
                // Temporarily move command_history and tab_state out of state.
                // They are put back below.
                let hist = std::mem::take(&mut state.command_history);
                let mut tab = std::mem::take(&mut state.tab_state);

                let res = feed(
                    key,
                    &mut buf_owned,
                    &mut cur_owned,
                    &mut hist_owned,
                    &mut tab,
                    &hist,
                    &cwd,
                    is_shell,
                );

                state.command_history = hist;
                state.tab_state = tab;
                res
            };

            // Write the possibly-mutated buffer back into the mode.
            if let Mode::Command {
                buffer,
                cursor,
                history_index,
            } = &mut state.mode
            {
                *buffer = buf_owned;
                *cursor = cur_owned;
                *history_index = hist_owned;
            }

            match result {
                FeedResult::Updated | FeedResult::Completion { .. } => {
                    state.dirty = true;
                }
                FeedResult::Cancel => {
                    state.mode = Mode::Navigation;
                    state.error_message = None;
                    state.dirty = true;
                }
                FeedResult::Submit(submitted_buf) => {
                    // Determine whether the buffer was a `!`-shell command or `:` command.
                    // The submitted buffer is the raw text including any leading `!`.
                    let (raw_buf, is_shell_submit) =
                        if let Some(rest) = submitted_buf.strip_prefix('!') {
                            (rest.to_owned(), true)
                        } else {
                            (submitted_buf.clone(), false)
                        };

                    // Parse the command.
                    let parse_result =
                        crate::input::command_parser::parse(&raw_buf, is_shell_submit);

                    // Push to history regardless of validity (so the user can
                    // edit and re-submit — but only non-empty strings).
                    if !submitted_buf.trim().is_empty() {
                        state.command_history.push(submitted_buf.clone());
                    }

                    // Exit Command Mode regardless.
                    state.mode = Mode::Navigation;
                    state.dirty = true;

                    match parse_result {
                        Ok(cmd) => {
                            state.error_message = None;
                            // Apply the parsed command.
                            apply(Action::ExecuteCommand(cmd), state)?;
                        }
                        Err(e) => {
                            // Surface validation error in the status bar.
                            state.error_message = Some(e.to_string());
                        }
                    }
                }
            }
        }

        // ── Filesystem command execution ──────────────────────────────────────
        Action::ExecuteCommand(cmd) => {
            execute_parsed_command(cmd, state)?;
        }

        // ── Clipboard ─────────────────────────────────────────────────────────
        Action::CopyAbsPath => {
            if let Some(entry) = state.selected_entry().cloned() {
                match clipboard::copy_absolute_path(&entry.path) {
                    Ok(s) => {
                        state.last_yank = Some(s);
                        state.error_message = None;
                        state.dirty = true;
                    }
                    Err(e) => {
                        state.error_message = Some(format!("yank: {e}"));
                        state.dirty = true;
                    }
                }
            }
        }

        Action::CopyRelPath => {
            if let Some(entry) = state.selected_entry().cloned() {
                let cwd = state.cwd.clone();
                match clipboard::copy_relative_path(&entry.path, &cwd) {
                    Ok(s) => {
                        state.last_yank = Some(s);
                        state.error_message = None;
                        state.dirty = true;
                    }
                    Err(e) => {
                        state.error_message = Some(format!("yank: {e}"));
                        state.dirty = true;
                    }
                }
            }
        }

        Action::CopyFilename => {
            if let Some(entry) = state.selected_entry().cloned() {
                match clipboard::copy_filename(&entry.path) {
                    Ok(s) => {
                        state.last_yank = Some(s);
                        state.error_message = None;
                        state.dirty = true;
                    }
                    Err(e) => {
                        state.error_message = Some(format!("yank: {e}"));
                        state.dirty = true;
                    }
                }
            }
        }

        // ── Delete with confirmation ───────────────────────────────────────────
        Action::BeginDelete => {
            if state.selected_entry().is_some() {
                state.pending_delete = true;
                state.error_message = None;
                state.dirty = true;
            }
        }

        Action::ConfirmDelete => {
            if !state.pending_delete {
                return Ok(());
            }
            state.pending_delete = false;
            if let Some(entry) = state.selected_entry().cloned() {
                match fs_ops::delete(&entry.path) {
                    Ok(()) => {
                        state.error_message = None;
                        // Refresh to reflect the deletion.
                        state.refresh()?;
                    }
                    Err(e) => {
                        state.error_message = Some(format!("delete: {e}"));
                        state.dirty = true;
                    }
                }
            }
        }

        Action::CancelDelete => {
            state.pending_delete = false;
            state.error_message = None;
            state.dirty = true;
        }

        Action::SetPendingNavKey(ch) => {
            state.pending_nav_key = Some(ch);
            state.dirty = true;
        }

        Action::Quit => {
            // Handled by the event loop checking the return value of dispatch;
            // nothing to do here at the state level.
        }
    }
    Ok(())
}

/// Executes a [`ParsedCommand`] against `state`, performing the corresponding
/// filesystem mutation (or surfacing a stub message for Phase-4+ commands).
fn execute_parsed_command(cmd: ParsedCommand, state: &mut AppState) -> Result<(), StateError> {
    let cwd = state.cwd.clone();

    match cmd {
        ParsedCommand::Mkdir(name) => match fs_ops::mkdir(&cwd, &name) {
            Ok(_) => {
                state.error_message = None;
                state.refresh()?;
            }
            Err(e) => {
                state.error_message = Some(format!("mkdir: {e}"));
                state.dirty = true;
            }
        },

        ParsedCommand::Touch(name) => match fs_ops::touch(&cwd, &name) {
            Ok(_) => {
                state.error_message = None;
                state.refresh()?;
            }
            Err(e) => {
                state.error_message = Some(format!("touch: {e}"));
                state.dirty = true;
            }
        },

        ParsedCommand::Rename(new_name) => {
            if let Some(entry) = state.selected_entry().cloned() {
                match fs_ops::rename(&entry.path, &new_name) {
                    Ok(_) => {
                        state.error_message = None;
                        state.refresh()?;
                    }
                    Err(e) => {
                        state.error_message = Some(format!("rename: {e}"));
                        state.dirty = true;
                    }
                }
            }
        }

        ParsedCommand::Mv(dest) => {
            if let Some(entry) = state.selected_entry().cloned() {
                match fs_ops::mv(&entry.path, &dest, &cwd) {
                    Ok(_) => {
                        state.error_message = None;
                        state.refresh()?;
                    }
                    Err(e) => {
                        state.error_message = Some(format!("mv: {e}"));
                        state.dirty = true;
                    }
                }
            }
        }

        ParsedCommand::Cp(dest) => {
            if let Some(entry) = state.selected_entry().cloned() {
                match fs_ops::cp(&entry.path, &dest, &cwd) {
                    Ok(_) => {
                        state.error_message = None;
                        state.refresh()?;
                    }
                    Err(e) => {
                        state.error_message = Some(format!("cp: {e}"));
                        state.dirty = true;
                    }
                }
            }
        }

        ParsedCommand::Git(_subcmd) => {
            // TODO(phase-4): Wire to the git worker.
            state.error_message = Some(":git — git worker not yet active (Phase 4)".to_owned());
            state.dirty = true;
        }

        ParsedCommand::Set { key, value: _ } => {
            // TODO(phase-7): Wire to config schema.
            state.error_message = Some(format!(
                ":set {key} — config schema not yet active (Phase 7)"
            ));
            state.dirty = true;
        }

        ParsedCommand::Shell(_cmd_str) => {
            // TODO(phase-6): Run via shell_exec::run_external.
            state.error_message =
                Some("!shell — shell execution not yet active (Phase 6)".to_owned());
            state.dirty = true;
        }
    }

    Ok(())
}
