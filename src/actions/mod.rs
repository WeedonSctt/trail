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
    /// Enter the selected directory, or open the selected file in the configured
    /// editor. For directories this is handled synchronously in `apply`; for
    /// files it returns [`Action::RunExternal`] via the `apply_enter_or_open`
    /// helper so the event loop can call `shell_exec::run_external` on the
    /// async path.
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

    // ── Shell integration (Phase 6) ───────────────────────────────────────
    /// Run an external process using the terminal suspend/resume sequence.
    ///
    /// Produced by `apply` for file-open and `!<shell command>` actions;
    /// handled by the event loop in `main.rs` which calls
    /// `shell_exec::run_external` and forces a full redraw on return.
    RunExternal {
        /// The argument vector: `argv[0]` is the program, `argv[1..]` are args.
        argv: Vec<String>,
        /// Working directory for the subprocess. Typically `state.cwd`.
        cwd: std::path::PathBuf,
    },

    // ── Quit ─────────────────────────────────────────────────────────────
    /// Quit the application normally (writes `--cwd-file` in Phase 6).
    Quit,
    /// Cancel the application without writing `--cwd-file`.
    ///
    /// Mapped to `Ctrl-C`. The shell wrapper does nothing and the parent
    /// shell stays in its original directory.
    Cancel,

    // ── OS open ─────────────────────────────────────────────────────────
    /// Open the selected entry with the OS default handler.
    ///
    /// On macOS uses `open`, on Linux `xdg-open`, on Windows `explorer`.
    /// Bound to `o` in Navigation Mode.
    OpenWithOs,

    // ── Tab management (Phase 8) ──────────────────────────────────────
    /// Open a new tab rooted at the current working directory.
    NewTab,
    /// Close the currently active tab. No-op if only one tab is open.
    CloseTab,
    /// Switch focus to the next tab, wrapping from the last to the first.
    SwitchTabNext,
    /// Switch focus to the previous tab, wrapping from the first to the last.
    SwitchTabPrev,
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
                        // Phase 6: open the file in the configured editor.
                        // We cannot call shell_exec::run_external here because
                        // apply() is synchronous and run_external manipulates
                        // the terminal. Instead we store the RunExternal action
                        // in state so the event loop can execute it.
                        let editor = state.config.general.editor.clone();
                        state.pending_external = Some(Action::RunExternal {
                            argv: vec![editor, entry.path.display().to_string()],
                            cwd: state.cwd.clone(),
                        });
                        state.dirty = true;
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
                        // Phase 6: open in editor via the RunExternal mechanism.
                        state.mode = Mode::Navigation;
                        state.filter = None;
                        let editor = state.config.general.editor.clone();
                        state.pending_external = Some(Action::RunExternal {
                            argv: vec![editor, entry.path.display().to_string()],
                            cwd: state.cwd.clone(),
                        });
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

        Action::RunExternal { .. } => {
            // RunExternal is never dispatched through apply() — the event loop
            // in main.rs intercepts it and calls shell_exec::run_external
            // directly. If it reaches here, it's a caller bug; log and ignore.
            tracing::debug!("RunExternal reached apply() — should be handled by event loop");
        }

        Action::Quit | Action::Cancel => {
            // Handled by the event loop; nothing to do at the state level.
        }

        // ── OS open ───────────────────────────────────────────────────────────
        Action::OpenWithOs => {
            if let Some(entry) = state.selected_entry().cloned() {
                #[cfg(target_os = "macos")]
                let argv = vec!["open".to_owned(), entry.path.display().to_string()];
                #[cfg(target_os = "linux")]
                let argv = vec!["xdg-open".to_owned(), entry.path.display().to_string()];
                #[cfg(target_os = "windows")]
                let argv = vec![
                    "cmd.exe".to_owned(),
                    "/C".to_owned(),
                    "start".to_owned(),
                    String::new(), // window title (required by start)
                    entry.path.display().to_string(),
                ];
                #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
                let argv = vec!["xdg-open".to_owned(), entry.path.display().to_string()];

                state.pending_external = Some(Action::RunExternal {
                    argv,
                    cwd: state.cwd.clone(),
                });
                state.dirty = true;
            }
        }

        // ── Tab management (Phase 8) ──────────────────────────────────────────
        Action::NewTab => {
            state.open_tab(None)?;
            state.dirty = true;
        }

        Action::CloseTab => {
            // close_tab returns false when only one tab remains; ignore.
            let _ = state.close_tab()?;
            state.dirty = true;
        }

        Action::SwitchTabNext => {
            state.switch_tab_next()?;
            state.dirty = true;
        }

        Action::SwitchTabPrev => {
            state.switch_tab_prev()?;
            state.dirty = true;
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

        ParsedCommand::Git(subcmd) => {
            #[cfg(unix)]
            let argv = vec!["sh".to_owned(), "-c".to_owned(), format!("git {subcmd}")];
            #[cfg(windows)]
            let argv = vec!["cmd".to_owned(), "/C".to_owned(), format!("git {subcmd}")];

            state.pending_external = Some(Action::RunExternal {
                argv,
                cwd: state.cwd.clone(),
            });
            state.dirty = true;
        }

        ParsedCommand::Set { key, value } => match state.config.set_value(&key, &value) {
            Ok(()) => {
                state.error_message = None;
                state.dirty = true;
            }
            Err(e) => {
                state.error_message = Some(format!("set: {e}"));
                state.dirty = true;
            }
        },

        ParsedCommand::Shell(cmd_str) => {
            // Phase 6: run via shell_exec::run_external through the event loop.
            // Route through the OS shell interpreter so that builtins, pipelines,
            // quoted arguments, and variable expansions all work correctly.
            // Direct argv-split would fail for any non-trivial shell command.
            #[cfg(windows)]
            let argv: Vec<String> = vec![
                "cmd.exe".to_owned(),
                "/C".to_owned(),
                cmd_str.clone(),
            ];
            #[cfg(not(windows))]
            let argv: Vec<String> = vec![
                "sh".to_owned(),
                "-c".to_owned(),
                cmd_str.clone(),
            ];

            if cmd_str.trim().is_empty() {
                state.error_message = Some("!: empty command".to_owned());
                state.dirty = true;
            } else {
                state.pending_external = Some(Action::RunExternal {
                    argv,
                    cwd: state.cwd.clone(),
                });
                state.dirty = true;
            }
        }

        ParsedCommand::Bookmark(name) => {
            // Use the supplied name, or fall back to the cwd base-name.
            let bookmark_name = if name.is_empty() {
                state
                    .cwd
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("bookmark")
                    .to_owned()
            } else {
                name
            };
            match state.bookmark_add(bookmark_name.clone()) {
                Ok(()) => {
                    state.error_message = Some(format!("bookmark added: {bookmark_name}"));
                    state.dirty = true;
                }
                Err(e) => {
                    state.error_message = Some(format!("bookmark: {e}"));
                    state.dirty = true;
                }
            }
        }

        ParsedCommand::Jump(name) => match state.bookmark_jump(&name) {
            Ok(true) => {
                state.error_message = None;
            }
            Ok(false) => {
                state.error_message = Some(format!("bookmark '{name}' not found"));
                state.dirty = true;
            }
            Err(e) => {
                state.error_message = Some(format!("jump: {e}"));
                state.dirty = true;
            }
        },
    }

    Ok(())
}
