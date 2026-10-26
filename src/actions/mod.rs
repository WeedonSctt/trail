//! Action system: the `Action` enum and `apply(action, state)`.
//!
//! Every user-initiated mutation flows through an `Action` value, keeping
//! the state machine testable independently of input handling.

pub mod clipboard;
pub mod fs_ops;
pub mod shell_exec;
pub mod transfer;

use std::path::{Path, PathBuf};

use crate::app::mode::Mode;
use crate::app::state::{AppState, StateError};
use crate::input::command_parser::ParsedCommand;
use transfer::{TransferItem, TransferKind};

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
    /// Return to where the newest jump (`:jump`) was made from (`U`),
    /// skipping the directories visited since.
    ReturnFromJump,
    /// Reload the current directory listing.
    Refresh,
    /// Toggle visibility of hidden files.
    ToggleHidden,

    // ── Listing order ───────────────────────────────────────────────────────
    /// Re-order the active tab's listing by the given key.
    SetSortBy(crate::app::sort::SortBy),
    /// Flip the active tab's listing order.
    ToggleSortReverse,
    /// Toggle whether directories are grouped ahead of files in the active tab.
    ToggleDirsFirst,
    /// Step the details column through `none → size → modified → both → none`.
    CycleEntryDetails,

    // ── Preview scrolling ───────────────────────────────────────────────────
    /// Scroll the preview pane down one line, leaving the selection alone.
    PreviewScrollDown,
    /// Scroll the preview pane up one line, leaving the selection alone.
    PreviewScrollUp,
    /// Scroll the preview pane down one screenful (less a two-line overlap).
    PreviewPageDown,
    /// Scroll the preview pane up one screenful (less a two-line overlap).
    PreviewPageUp,
    /// Scroll the preview pane back to its first line.
    PreviewScrollTop,
    /// Scroll the preview pane to the last loaded line, which is the end of the
    /// file only when the preview is not truncated.
    PreviewScrollBottom,

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
    /// Do the last file operation again (`@`): the newest `:`/`!` command, or
    /// `dd`, `pp` or `pm`, whichever came last — see [`Repeatable`]. Like
    /// Vim's `.`, it acts on whatever is selected — or marked — now.
    RepeatLastCommand,

    // ── Filesystem mutations (Phase 3) ────────────────────────────────────
    /// Execute a validated, parsed command (dispatched after Command Mode submit).
    ExecuteCommand(ParsedCommand),
    /// Copy the absolute path of the selected entry to the yank buffer.
    CopyAbsPath,
    /// Copy the relative path of the selected entry to the yank buffer.
    CopyRelPath,
    /// Copy the filename of the selected entry to the yank buffer.
    CopyFilename,
    /// Copy the content of the selected entry to the yank buffer: the text of
    /// a file, or the listing of a directory.
    CopyContent,
    /// Begin the `dd` delete flow — sets `pending_delete = true`.
    BeginDelete,
    /// Confirm and execute the pending delete.
    ConfirmDelete,
    /// Cancel the pending delete confirmation.
    CancelDelete,
    /// Mark or unmark the selected entry, then move down (`Space`), so a run
    /// of entries is marked by pressing it repeatedly.
    ToggleMark,
    /// Unmark everything (`Esc` in Navigation Mode, `:unmark`).
    ClearMarks,
    /// Copy (`pp`) or move (`pm`) every marked entry into `cwd`.
    PasteMarks(TransferKind),
    /// Enter visual mode (`v`), anchoring a range at the selection.
    EnterVisual,
    /// Add the visual range to the marks and return to Navigation Mode, then
    /// apply the follow-up action if there is one — so `h`/`l` in visual mode
    /// mark the range and navigate in one key.
    CommitVisual(Option<Box<Action>>),
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
        /// Whether to hold the screen after the command finishes so its output
        /// can be read. Resolved from `[general] shell_pause` for `!` and
        /// `:git`, and always [`shell_exec::ShellPause::Never`] for the editor
        /// and OS-handler opens, which draw their own screens.
        pause: shell_exec::ShellPause,
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

/// What `@` repeats: the last file operation, typed or pressed.
///
/// Only operations are recorded — commands and the keys that change files
/// (`dd`, `pp`, `pm`). Moving, marking and yanking are not, so `j` between a
/// `dd` and an `@` does not replace the `dd`; it is what makes `dd`, `j`, `@`
/// work the way Vim's `.` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repeatable {
    /// A `:` command without its `:`, or a `!` command with its `!`.
    CommandLine(String),
    /// A key's action, replayed as if the key were pressed again. A repeated
    /// `dd` therefore still asks for confirmation.
    Key(Action),
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

        Action::PreviewScrollDown => state.scroll_preview_lines(1),
        Action::PreviewScrollUp => state.scroll_preview_lines(-1),
        Action::PreviewPageDown => state.scroll_preview_pages(1),
        Action::PreviewPageUp => state.scroll_preview_pages(-1),
        Action::PreviewScrollTop => state.scroll_preview_to_edge(false),
        Action::PreviewScrollBottom => state.scroll_preview_to_edge(true),

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
                            // An editor owns the screen while it runs and leaves
                            // nothing to read behind it.
                            pause: shell_exec::ShellPause::Never,
                        });
                        state.dirty = true;
                    }
                }
            }
        }

        Action::GoParent => {
            state.go_parent()?;
        }

        // At either end of the history these used to do nothing at all, which
        // reads as a key that is not working — the same reason a tab switch
        // with one tab says so.
        Action::HistoryBack => {
            if state.history.can_go_back() {
                state.history_back()?;
            } else {
                state.notify("no earlier directory — u goes back once you have moved");
            }
        }

        Action::HistoryForward => {
            if state.history.can_go_forward() {
                state.history_forward()?;
            } else {
                state.notify("no later directory — Ctrl-r goes forward after u");
            }
        }

        Action::ReturnFromJump => {
            let before = state.cwd.clone();
            match state.return_from_jump()? {
                None => state.notify("no jump to return from — :jump makes one"),
                Some(0) => {}
                Some(skipped) if state.cwd == before => state.notify(format!(
                    "no jump to return from — {skipped} led to directories that are gone"
                )),
                Some(skipped) => state.notify(format!(
                    "skipped {skipped} jump(s) to directories that are gone"
                )),
            }
        }

        Action::Refresh => {
            state.refresh()?;
        }

        Action::ToggleHidden => {
            state.toggle_hidden()?;
        }

        Action::SetSortBy(by) => {
            apply_sort(state, |s| s.by = by);
        }

        Action::ToggleSortReverse => {
            apply_sort(state, |s| s.reverse = !s.reverse);
        }

        Action::ToggleDirsFirst => {
            apply_sort(state, |s| s.dirs_first = !s.dirs_first);
        }

        Action::CycleEntryDetails => {
            // Read at render time, so unlike the sort keys this needs nothing
            // rebuilt — the next frame simply draws the new column.
            let next = state.config.navigation.details().next();
            state.config.navigation.entry_details = next.as_str().to_owned();
            state.notify(format!("details: {}", next.as_str()));
            state.dirty = true;
        }

        // Mode transitions.
        Action::EnterSearch => {
            state.mode = Mode::Search {
                query: String::new(),
                matches: Vec::new(),
            };
            // Entering Search Mode with an empty query shows all entries.
            state.apply_filter(String::new());
            state.dirty = true;
        }

        Action::EnterCommand => {
            state.mode = Mode::Command {
                buffer: String::new(),
                cursor: 0,
                history_index: None,
            };
            state.clear_notice();
            state.dirty = true;
        }

        Action::ExitMode => {
            if state.mode != Mode::Navigation {
                state.mode = Mode::Navigation;
                state.clear_filter();
                state.pending_delete = false;
                state.clear_notice();
                state.pending_nav_key = None;
                state.dirty = true;
            }
        }

        // Search Mode actions.
        Action::SearchAppendChar(ch) => {
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
            if let Some(entry) = state.selected_entry().cloned() {
                use crate::app::state::EntryKind;
                match entry.kind {
                    EntryKind::Dir => {
                        // Exit Search Mode first, then enter the directory —
                        // with the selection still on it, so that is what the
                        // parent remembers and `h` comes back to.
                        state.mode = Mode::Navigation;
                        state.clear_filter();
                        state.enter_dir(entry.path)?;
                    }
                    EntryKind::File | EntryKind::Symlink => {
                        // Phase 6: open in editor via the RunExternal mechanism.
                        state.mode = Mode::Navigation;
                        state.clear_filter();
                        let editor = state.config.general.editor.clone();
                        state.pending_external = Some(Action::RunExternal {
                            argv: vec![editor, entry.path.display().to_string()],
                            cwd: state.cwd.clone(),
                            pause: shell_exec::ShellPause::Never,
                        });
                        state.dirty = true;
                    }
                }
            }
        }

        // ── Command Mode ─────────────────────────────────────────────────────
        Action::CommandKey(key) => {
            use crate::input::command_parser::{feed_with_sources, CompletionSources, FeedResult};

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

            let plugin_actions: Vec<String> = state
                .plugin_engine
                .as_ref()
                .map(|e| e.action_names().map(|s| s.to_string()).collect())
                .unwrap_or_default();
            let plugin_action_refs: Vec<&str> = plugin_actions.iter().map(|s| s.as_str()).collect();
            let bookmarks: Vec<(String, PathBuf)> = state
                .bookmark_store
                .as_ref()
                .map(|store| {
                    store
                        .list()
                        .map(|(name, path)| (name.to_owned(), path.to_owned()))
                        .collect()
                })
                .unwrap_or_default();

            let result = {
                // Temporarily move command_history and tab_state out of state.
                // They are put back below.
                let hist = std::mem::take(&mut state.command_history);
                let mut tab = std::mem::take(&mut state.tab_state);

                let res = feed_with_sources(
                    key,
                    &mut buf_owned,
                    &mut cur_owned,
                    &mut hist_owned,
                    &mut tab,
                    &hist,
                    &cwd,
                    is_shell,
                    &CompletionSources {
                        plugin_actions: &plugin_action_refs,
                        bookmarks: &bookmarks,
                    },
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
                    state.clear_notice();
                    state.dirty = true;
                }
                FeedResult::Submit(submitted_buf) => {
                    // Push to history regardless of validity (so the user can
                    // edit and re-submit — but only non-empty strings).
                    if !submitted_buf.trim().is_empty() {
                        state.command_history.push(submitted_buf.clone());
                    }

                    // Exit Command Mode regardless.
                    state.mode = Mode::Navigation;
                    state.dirty = true;

                    if !submitted_buf.trim().is_empty() {
                        state.last_repeatable =
                            Some(Repeatable::CommandLine(submitted_buf.clone()));
                    }
                    run_command_line(state, &submitted_buf)?;
                }
            }
        }

        Action::RepeatLastCommand => match state.last_repeatable.clone() {
            Some(Repeatable::CommandLine(line)) => {
                state.clear_notice();
                run_command_line(state, &line)?;
            }
            Some(Repeatable::Key(action)) => apply(action, state)?,
            None => state.notify("nothing to repeat yet — @ repeats a command, dd, pp or pm"),
        },

        // ── Filesystem command execution ──────────────────────────────────────
        Action::ExecuteCommand(cmd) => {
            execute_parsed_command(cmd, state)?;
        }

        // ── Clipboard ─────────────────────────────────────────────────────────
        Action::CopyAbsPath => {
            if let Some(entry) = state.selected_entry().cloned() {
                record_yank(state, clipboard::absolute_path_text(&entry.path));
            }
        }

        Action::CopyRelPath => {
            if let Some(entry) = state.selected_entry().cloned() {
                // Relative to where Trail was launched, not to the directory
                // being browsed — see `AppState::launch_dir`. Against `cwd`
                // this yanked the bare file name, duplicating `yn`.
                let base = state.launch_dir.clone();
                record_yank(state, clipboard::relative_path_text(&entry.path, &base));
            }
        }

        Action::CopyFilename => {
            if let Some(entry) = state.selected_entry().cloned() {
                record_yank(state, clipboard::filename_text(&entry.path));
            }
        }

        Action::CopyContent => {
            if let Some(entry) = state.selected_entry().cloned() {
                let show_hidden = state.show_hidden;
                record_yank(state, clipboard::content_text(&entry.path, show_hidden));
            }
        }

        // ── Delete with confirmation ───────────────────────────────────────────
        Action::BeginDelete => {
            let has_target = state.selected_entry().is_some()
                || !state.marks.is_empty()
                || matches!(state.mode, Mode::Visual { .. });
            if has_target {
                state.last_repeatable = Some(Repeatable::Key(Action::BeginDelete));
                state.pending_delete = true;
                state.clear_notice();
                state.dirty = true;
            }
        }

        Action::ConfirmDelete => {
            if !state.pending_delete {
                return Ok(());
            }
            state.pending_delete = false;
            // What `dd` deletes, most specific first: a visual range is what the
            // user is looking at right now, then the marks, then the selection.
            if matches!(state.mode, Mode::Visual { .. }) {
                let range = crate::app::marks::visual_paths(state);
                state.mode = Mode::Navigation;
                delete_paths(state, range)?;
            } else if !state.marks.is_empty() {
                let marked = state.marks.paths().cloned().collect();
                delete_paths(state, marked)?;
            } else if let Some(entry) = state.selected_entry().cloned() {
                let mode = configured_delete_mode(state);
                match fs_ops::delete(&entry.path, mode) {
                    Ok(()) => {
                        // Say where it went: with a recycle bin in play, "gone"
                        // and "recoverable" are different outcomes and the user
                        // is entitled to know which one they got.
                        state.notify(match mode {
                            fs_ops::DeleteMode::Trash => {
                                format!("moved to the recycle bin: {}", entry.file_name)
                            }
                            fs_ops::DeleteMode::Permanent => {
                                format!("deleted: {}", entry.file_name)
                            }
                        });
                        // Refresh to reflect the deletion.
                        state.refresh()?;
                    }
                    Err(e) => {
                        state.set_error(format!("delete: {e}"));
                    }
                }
            }
        }

        Action::CancelDelete => {
            state.pending_delete = false;
            state.clear_notice();
            state.dirty = true;
        }

        Action::ToggleMark => {
            if let Some(entry) = state.selected_entry().cloned() {
                state.marks.toggle(&entry.path);
                state.move_down();
                state.dirty = true;
            }
        }

        Action::ClearMarks => clear_marks(state),

        Action::EnterVisual => {
            if state.mode == Mode::Navigation {
                if let Some(entry) = state.selected_entry() {
                    state.mode = Mode::Visual {
                        anchor: entry.path.clone(),
                    };
                    state.dirty = true;
                }
            }
        }

        Action::CommitVisual(then) => {
            if matches!(state.mode, Mode::Visual { .. }) {
                let range = crate::app::marks::visual_paths(state);
                state.marks.extend(range);
                state.mode = Mode::Navigation;
                state.dirty = true;
            }
            if let Some(next) = then {
                apply(*next, state)?;
            }
        }

        Action::PasteMarks(kind) => {
            state.last_repeatable = Some(Repeatable::Key(Action::PasteMarks(kind)));
            paste_marks(state, kind);
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
                    // The OS handler opens its own window; there is no output
                    // here to hold the screen for.
                    pause: shell_exec::ShellPause::Never,
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
            if !state.close_tab()? {
                state.notify(LAST_TAB_HINT);
            }
            state.dirty = true;
        }

        Action::SwitchTabNext => {
            if state.tab_manager.is_single() {
                state.notify(SINGLE_TAB_HINT);
            } else {
                state.switch_tab_next()?;
            }
            state.dirty = true;
        }

        Action::SwitchTabPrev => {
            if state.tab_manager.is_single() {
                state.notify(SINGLE_TAB_HINT);
            } else {
                state.switch_tab_prev()?;
            }
            state.dirty = true;
        }
    }
    Ok(())
}

/// Parses and runs one command line exactly as typed — `:` commands without
/// their `:`, shell commands with their `!`. Shared by `Enter` in Command Mode
/// and by `@`, so a repeated command cannot behave differently from the
/// original.
///
/// # Errors
///
/// Returns [`StateError`] if the command's own action fails that way; a parse
/// error is reported in the status bar instead.
fn run_command_line(state: &mut AppState, line: &str) -> Result<(), StateError> {
    let (text, is_shell) = match line.strip_prefix('!') {
        Some(rest) => (rest, true),
        None => (line, false),
    };
    match crate::input::command_parser::parse(text, is_shell) {
        Ok(cmd) => {
            state.clear_notice();
            apply(Action::ExecuteCommand(cmd), state)
        }
        Err(e) => {
            state.set_error(e.to_string());
            Ok(())
        }
    }
}

/// Expands an `@name` destination through the bookmark store, or returns
/// `dest` unchanged when it does not start with `@`.
///
/// `@docs` is the bookmark's directory and `@docs\next\route` a route under it.
/// An unknown name is an error, reported here — never a folder literally named
/// `@docs`, which is reached as `.\@docs` instead. Returns `None` after
/// reporting.
fn expand_destination(state: &mut AppState, kind: TransferKind, dest: &str) -> Option<PathBuf> {
    let Some(after_at) = dest.strip_prefix('@') else {
        return Some(PathBuf::from(dest));
    };
    let (name, route) = match after_at.find(std::path::is_separator) {
        Some(split) => (&after_at[..split], &after_at[split + 1..]),
        None => (after_at, ""),
    };
    let base = state
        .bookmark_store
        .as_ref()
        .and_then(|store| store.get(name))
        .map(Path::to_path_buf);
    match base {
        Some(base) if route.is_empty() => Some(base),
        Some(base) => Some(base.join(route)),
        None => {
            state.set_error(format!(
                "{}: no bookmark named '{name}' — :bookmark {name} makes one",
                transfer_verb(kind)
            ));
            None
        }
    }
}

/// Saves this session's `:set` changes into the config file (`:config write`).
///
/// Into the file this session loaded; with none, into `trail.toml` in the
/// config directory, which is then remembered so the next start loads it —
/// a written setting that the next start ignored would be worse than none.
fn write_config(state: &mut AppState) {
    use crate::config::persist;

    if state.settings.changes().is_empty() {
        state.notify("nothing to write — no :set changes this session");
        return;
    }
    let config_dir = crate::paths::config_dir();
    let Some(path) = persist::target_file(state.settings.file.as_deref(), config_dir.as_deref())
    else {
        state.set_error("config: no config file loaded, and no config directory to create one in");
        return;
    };
    if let Err(e) = persist::write_settings(&path, state.settings.changes()) {
        state.set_error(format!("config: {e}"));
        return;
    }

    let count = state.settings.changes().len();
    state.settings.clear();
    let shown = crate::pathfmt::display(&path);
    if state.settings.file.is_none() {
        if let Some(state_file) = state.settings.remember_in.clone() {
            if let Err(e) = crate::config::last_used::remember(&state_file, &path) {
                state.set_error(format!(
                    "config: wrote {shown}, but could not remember it for next time: {e}"
                ));
                return;
            }
        }
        state.settings.file = Some(path);
    }
    state.notify(format!("wrote {count} setting(s) to {shown}"));
}

/// Unmarks everything, saying how many marks went. Silent when there were
/// none, because `Esc` reaches here on every press in Navigation Mode.
fn clear_marks(state: &mut AppState) {
    let count = state.marks.clear();
    if count > 0 {
        state.notify(format!("{count} unmarked"));
    }
}

/// Copies or moves every marked entry into `cwd`, under its own name.
///
/// Marks that have vanished since they were made go into the job anyway, so
/// the report counts them and [`transfer::finish`] drops them from the set.
fn paste_marks(state: &mut AppState, kind: TransferKind) {
    if state.marks.is_empty() {
        state.notify(NOTHING_MARKED_HINT);
        return;
    }
    let cwd = state.cwd.clone();
    let items = state
        .marks
        .paths()
        .map(|source| TransferItem {
            source: source.clone(),
            dest: cwd.join(source.file_name().unwrap_or_default()),
        })
        .collect();
    transfer::start(state, kind, "paste", items);
}

/// Deletes `paths` — the marks, or a visual range — after the single
/// confirmation `dd` asked for.
///
/// Each entry goes the way `[general] delete_mode` says, the same as a `dd` of
/// one. Deleted and vanished entries leave the mark set; one that could not be
/// deleted stays marked, so the user can see which and try again.
///
/// # Errors
///
/// Returns [`StateError`] only if re-listing the directory afterwards fails.
fn delete_paths(state: &mut AppState, paths: Vec<PathBuf>) -> Result<(), StateError> {
    let mode = configured_delete_mode(state);
    let mut deleted = 0usize;
    let mut gone = 0usize;
    let mut first_failure = None;
    let mut failed = 0usize;

    for path in paths {
        if path.symlink_metadata().is_err() {
            gone += 1;
            state.marks.remove(&path);
            continue;
        }
        match fs_ops::delete(&path, mode) {
            Ok(()) => {
                deleted += 1;
                state.marks.remove(&path);
            }
            Err(e) => {
                failed += 1;
                first_failure.get_or_insert(e.to_string());
            }
        }
    }

    let verb = match mode {
        fs_ops::DeleteMode::Trash => "moved to the recycle bin",
        fs_ops::DeleteMode::Permanent => "deleted",
    };
    let mut report = format!("{deleted} {verb}");
    if gone > 0 {
        report.push_str(&format!(", {gone} gone"));
    }
    match first_failure {
        Some(reason) => state.set_error(format!("{report}, {failed} failed ({reason})")),
        None => state.notify(report),
    }
    state.refresh()
}

/// Shown when a paste is asked for with nothing marked.
const NOTHING_MARKED_HINT: &str = "nothing is marked — Space marks the selection";

/// The verb a transfer is reported under: `cp` or `mv`.
fn transfer_verb(kind: TransferKind) -> &'static str {
    match kind {
        TransferKind::Copy => "cp",
        TransferKind::Move => "mv",
    }
}

/// Starts a `:cp`/`:mv` of the selected entry to `dest`.
///
/// Only the destination is resolved here; the copy or move itself runs in the
/// background (see [`transfer`]) and reports when it is done.
fn transfer_selection(state: &mut AppState, kind: TransferKind, dest: &str) {
    let Some(dest) = expand_destination(state, kind, dest) else {
        return;
    };
    if !state.marks.is_empty() {
        transfer_marks(state, kind, &dest);
        return;
    }
    let Some(entry) = state.selected_entry().cloned() else {
        return;
    };
    let target = fs_ops::resolve_target(&entry.path, &dest, &state.cwd);
    transfer::start(
        state,
        kind,
        transfer_verb(kind),
        vec![TransferItem {
            source: entry.path,
            dest: target,
        }],
    );
}

/// Starts a `:cp`/`:mv` of every marked entry to `dest`.
///
/// One mark behaves exactly like the selection would: `dest` may be a new
/// name. Several need `dest` to be an existing directory, for the same reason
/// a pattern does — several entries cannot all become one path.
fn transfer_marks(state: &mut AppState, kind: TransferKind, dest: &Path) {
    let verb = transfer_verb(kind);
    let marked: Vec<PathBuf> = state.marks.paths().cloned().collect();
    if marked.len() > 1 && !fs_ops::resolve_dest(dest, &state.cwd).is_dir() {
        state.set_error(format!(
            "{verb}: '{}' is not a directory, and {} entries are marked",
            crate::pathfmt::display(dest),
            marked.len()
        ));
        return;
    }
    let items = marked
        .into_iter()
        .map(|source| TransferItem {
            dest: fs_ops::resolve_target(&source, dest, &state.cwd),
            source,
        })
        .collect();
    transfer::start(state, kind, verb, items);
}

/// Starts a `:cp`/`:mv` of every entry in `cwd` matching `pattern`.
///
/// `dest` must be an existing directory: a pattern names several files, and
/// several files cannot be moved onto one path. Checking it first is what stops
/// `:mv *.md notes.md` from copying each match over the last.
///
/// A failure part-way through does not undo what already succeeded -- there is
/// no transaction to roll back to -- so the report says what became of each.
fn transfer_matches(state: &mut AppState, kind: TransferKind, pattern: &str, dest: &str) {
    let verb = transfer_verb(kind);
    let cwd = state.cwd.clone();
    let Some(dest_expanded) = expand_destination(state, kind, dest) else {
        return;
    };

    let matches = match fs_ops::matching_entries(&cwd, pattern) {
        Ok(matches) => matches,
        Err(e) => {
            state.set_error(format!("{verb}: {e}"));
            return;
        }
    };
    if matches.is_empty() {
        state.set_error(format!("{verb}: nothing matches '{pattern}'"));
        return;
    }

    let dest_path = fs_ops::resolve_dest(&dest_expanded, &cwd);
    if !dest_path.is_dir() {
        state.set_error(format!(
            "{verb}: '{dest}' is not a directory, and '{pattern}' matches {} entries",
            matches.len()
        ));
        return;
    }

    let items = matches
        .into_iter()
        .map(|source| TransferItem {
            dest: dest_path.join(source.file_name().unwrap_or_default()),
            source,
        })
        .collect();
    transfer::start(state, kind, verb, items);
}

/// Where `dd` sends an entry, from `[general] delete_mode`.
///
/// An unparseable value cannot normally reach here -- it is rejected at load and
/// by `:set` -- so the fallback is the recoverable option: a config Trail cannot
/// read must not be the reason a file cannot be got back.
/// Applies `change` to the active tab's sort settings, re-orders the listing,
/// and says what the new order is.
///
/// Every sort binding and `:sort` go through here so that the notice, the
/// re-sort and the per-tab bookkeeping cannot be done by one caller and
/// forgotten by the next. The settings are read back from `state.sort` rather
/// than from the config, because they are per tab: the config only ever seeded
/// the first one.
fn apply_sort(state: &mut AppState, change: impl FnOnce(&mut crate::app::sort::SortSettings)) {
    let mut settings = state.sort;
    change(&mut settings);
    state.set_sort(settings);
    state.notify(settings.describe());
}

fn configured_delete_mode(state: &AppState) -> fs_ops::DeleteMode {
    fs_ops::DeleteMode::parse(&state.config.general.delete_mode)
        .unwrap_or(fs_ops::DeleteMode::Trash)
}

/// The pause policy for a command whose output the user is expecting to read.
///
/// An unparseable value cannot normally reach here -- `TrailConfig::validate`
/// rejects one at load and `:set` rejects one at runtime -- so falling back to
/// waiting is the conservative answer for a config that somehow got past both:
/// output kept is recoverable, output erased is not.
fn configured_pause(state: &AppState) -> shell_exec::ShellPause {
    shell_exec::ShellPause::parse(&state.config.general.shell_pause)
        .unwrap_or(shell_exec::ShellPause::Always)
}

/// Shown when a tab switch is asked for and there is nothing to switch to.
///
/// Switching used to be a silent no-op here, which is indistinguishable from a
/// binding that is not working — and was reported as exactly that.
const SINGLE_TAB_HINT: &str = "only one tab open — Ctrl-t opens another";

/// Shown when the last remaining tab is asked to close.
const LAST_TAB_HINT: &str = "the last tab stays open — q quits Trail";

/// Records the outcome of a yank operation in `state`.
///
/// `text` is the already-computed string to yank — a path from one of the
/// `*_text` functions in [`clipboard`], or an entry's content from
/// `clipboard::content_text`. On success the string is stored in
/// `AppState::last_yank` **whether or not** the OS clipboard accepted it: the
/// text was computed correctly, and only the hand-off to the OS can fail. That
/// keeps the status bar honest on machines with no reachable clipboard
/// (headless servers, bare TTYs) while still reporting the failure, rather
/// than silently doing nothing.
fn record_yank(state: &mut AppState, text: Result<String, clipboard::ClipboardError>) {
    match text {
        Ok(s) => {
            if let Err(e) = clipboard::set_clipboard(&s) {
                // Non-fatal: the yank still happened as far as Trail is
                // concerned, the OS just would not take it.
                state.set_error(format!("clipboard unavailable: {e}"));
            } else {
                // Summarized here rather than in the status bar: a `yc` of a
                // whole file must not put the file in a one-line widget, and
                // the notice is the thing that gets logged.
                state.notify(format!("yanked: {}", clipboard::summarize(&s)));
            }
            state.last_yank = Some(s);
        }
        Err(e) => {
            state.set_error(format!("yank: {e}"));
        }
    }
    state.dirty = true;
}

/// Executes a [`ParsedCommand`] against `state`, performing the corresponding
/// filesystem mutation (or surfacing a stub message for Phase-4+ commands).
fn execute_parsed_command(cmd: ParsedCommand, state: &mut AppState) -> Result<(), StateError> {
    let cwd = state.cwd.clone();

    match cmd {
        ParsedCommand::Mkdir(name) => match fs_ops::mkdir(&cwd, &name) {
            Ok(_) => {
                state.clear_notice();
                state.refresh()?;
            }
            Err(e) => {
                state.set_error(format!("mkdir: {e}"));
            }
        },

        ParsedCommand::Touch(name) => match fs_ops::touch(&cwd, &name) {
            Ok(_) => {
                state.clear_notice();
                state.refresh()?;
            }
            Err(e) => {
                state.set_error(format!("touch: {e}"));
            }
        },

        ParsedCommand::Rename(new_name) => {
            if let Some(entry) = state.selected_entry().cloned() {
                match fs_ops::rename(&entry.path, &new_name) {
                    Ok(_) => {
                        state.clear_notice();
                        state.refresh()?;
                    }
                    Err(e) => {
                        state.set_error(format!("rename: {e}"));
                    }
                }
            }
        }

        ParsedCommand::Mv(dest) => transfer_selection(state, TransferKind::Move, &dest),

        ParsedCommand::Cp(dest) => transfer_selection(state, TransferKind::Copy, &dest),

        ParsedCommand::MvMatching { pattern, dest } => {
            transfer_matches(state, TransferKind::Move, &pattern, &dest);
        }

        ParsedCommand::CpMatching { pattern, dest } => {
            transfer_matches(state, TransferKind::Copy, &pattern, &dest);
        }

        ParsedCommand::Git(subcmd) => {
            state.pending_external = Some(Action::RunExternal {
                argv: shell_exec::shell_argv(&state.config.general.shell, &format!("git {subcmd}")),
                cwd: state.cwd.clone(),
                pause: configured_pause(state),
            });
            state.dirty = true;
        }

        ParsedCommand::Sort { by, reverse } => {
            apply_sort(state, |s| {
                if let Some(by) = by {
                    s.by = by;
                }
                if reverse {
                    s.reverse = !s.reverse;
                }
            });
        }

        ParsedCommand::Set { key, value } => match state.config.set_value(&key, &value) {
            Ok(()) => {
                state.settings.record(&key, &value);
                // `preview.*` keys feed process-wide graphics state rather than
                // being read out of the config at render time, so it has to be
                // rebuilt here. Cheap, and applies to the next preview.
                if key.contains("image_") {
                    crate::preview::graphics::configure(
                        &state.config.preview.image_protocol,
                        (
                            state.config.preview.image_cell_width,
                            state.config.preview.image_cell_height,
                        ),
                    );
                }
                // The sort keys decide the order the listing was *built* in, so
                // setting one has no effect until something re-sorts — without
                // this, `:set sort_by size` would appear to do nothing until the
                // next navigation. `entry_details` needs no such rebuild: the
                // renderer reads it every frame.
                if matches!(
                    key.as_str(),
                    "navigation.sort_by"
                        | "sort_by"
                        | "navigation.sort_reverse"
                        | "sort_reverse"
                        | "navigation.dirs_first"
                        | "dirs_first"
                ) {
                    state.set_sort(state.config.navigation.sort_settings());
                }
                state.clear_notice();
                state.dirty = true;
            }
            Err(e) => {
                state.set_error(format!("set: {e}"));
            }
        },

        ParsedCommand::Shell(cmd_str) => {
            // Phase 6: run via shell_exec::run_external through the event loop.
            // Route through a shell interpreter so that builtins, pipelines,
            // quoted arguments, and variable expansions all work correctly.
            // Direct argv-split would fail for any non-trivial shell command.
            // Which interpreter is `[general] shell`'s call, not ours.
            if cmd_str.trim().is_empty() {
                state.set_error("!: empty command".to_owned());
            } else {
                state.pending_external = Some(Action::RunExternal {
                    argv: shell_exec::shell_argv(&state.config.general.shell, &cmd_str),
                    cwd: state.cwd.clone(),
                    pause: configured_pause(state),
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
                    state.notify(format!("bookmark added: {bookmark_name}"));
                }
                Err(e) => {
                    state.set_error(format!("bookmark: {e}"));
                }
            }
        }

        ParsedCommand::Jump(name) => match state.bookmark_jump(&name) {
            Ok(true) => {
                state.clear_notice();
            }
            Ok(false) => {
                state.set_error(format!("bookmark '{name}' not found"));
            }
            Err(e) => {
                state.set_error(format!("jump: {e}"));
            }
        },

        ParsedCommand::Term(command) => crate::terminal::run_command(state, command),

        ParsedCommand::ConfigWrite => write_config(state),

        ParsedCommand::Unmark => {
            if state.marks.is_empty() {
                state.notify(NOTHING_MARKED_HINT);
            } else {
                clear_marks(state);
            }
        }

        ParsedCommand::Plugin { name, arg } => {
            if let Some(engine) = &state.plugin_engine {
                if engine.fire_action(&name, &arg) {
                    state.clear_notice();
                } else {
                    state.set_error(format!("plugin action '{name}' not found"));
                }
            } else {
                state.set_error(format!("plugin action '{name}' failed: no plugins loaded"));
            }
        }
    }

    Ok(())
}
