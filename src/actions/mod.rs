//! Action system: the `Action` enum and `apply(action, state)`.
//!
//! Every user-initiated mutation flows through an `Action` value, keeping
//! the state machine testable independently of input handling.

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
            state.clear_notice();
            state.dirty = true;
        }

        Action::ExitMode => {
            use crate::app::mode::Mode;
            if state.mode != Mode::Navigation {
                state.mode = Mode::Navigation;
                state.filter = None;
                state.pending_delete = false;
                state.clear_notice();
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
                            pause: shell_exec::ShellPause::Never,
                        });
                        state.dirty = true;
                    }
                }
            }
        }

        // ── Command Mode ─────────────────────────────────────────────────────
        Action::CommandKey(key) => {
            use crate::app::mode::Mode;
            use crate::input::command_parser::{feed_with_plugins, FeedResult};

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

            let result = {
                // Temporarily move command_history and tab_state out of state.
                // They are put back below.
                let hist = std::mem::take(&mut state.command_history);
                let mut tab = std::mem::take(&mut state.tab_state);

                let res = feed_with_plugins(
                    key,
                    &mut buf_owned,
                    &mut cur_owned,
                    &mut hist_owned,
                    &mut tab,
                    &hist,
                    &cwd,
                    is_shell,
                    &plugin_action_refs,
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
                            state.clear_notice();
                            // Apply the parsed command.
                            apply(Action::ExecuteCommand(cmd), state)?;
                        }
                        Err(e) => {
                            // Surface validation error in the status bar.
                            state.set_error(e.to_string());
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
            if state.selected_entry().is_some() {
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
            if let Some(entry) = state.selected_entry().cloned() {
                match fs_ops::delete(&entry.path) {
                    Ok(()) => {
                        state.clear_notice();
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

        ParsedCommand::Mv(dest) => {
            if let Some(entry) = state.selected_entry().cloned() {
                match fs_ops::mv(&entry.path, &dest, &cwd) {
                    Ok(_) => {
                        state.clear_notice();
                        state.refresh()?;
                    }
                    Err(e) => {
                        state.set_error(format!("mv: {e}"));
                    }
                }
            }
        }

        ParsedCommand::Cp(dest) => {
            if let Some(entry) = state.selected_entry().cloned() {
                match fs_ops::cp(&entry.path, &dest, &cwd) {
                    Ok(_) => {
                        state.clear_notice();
                        state.refresh()?;
                    }
                    Err(e) => {
                        state.set_error(format!("cp: {e}"));
                    }
                }
            }
        }

        ParsedCommand::Git(subcmd) => {
            state.pending_external = Some(Action::RunExternal {
                argv: shell_exec::shell_argv(&state.config.general.shell, &format!("git {subcmd}")),
                cwd: state.cwd.clone(),
                pause: configured_pause(state),
            });
            state.dirty = true;
        }

        ParsedCommand::Set { key, value } => match state.config.set_value(&key, &value) {
            Ok(()) => {
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
