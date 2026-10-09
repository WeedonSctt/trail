//! How keystrokes and `:term` commands reach the terminal panel.
//!
//! [`handle_key`] runs before Trail's own keymap, so the panel's toggle and
//! focus keys work from any mode, and a focused shell receives every key that
//! is not reserved. [`run_command`] carries out `:term`; [`allow_quit`] is the
//! check a quit passes through before it ends the panel's shells.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use super::keys::{self, KeyChord};
use super::{Confirm, ConfirmPolicy, KeyOutcome, TermCommand};
use crate::app::mode::Mode;
use crate::app::state::AppState;

/// Offers `key` to the panel before Trail's keymap sees it.
///
/// In order: an open question takes `y`/`n`/Enter/Esc and nothing else; the
/// toggle and focus keys work from anywhere; with the shell focused, the
/// reserved keys act and every other key is sent to the shell.
pub fn handle_key(state: &mut AppState, key: &KeyEvent) -> KeyOutcome {
    if key.kind == KeyEventKind::Release {
        // crossterm reports releases on Windows; a shell only wants presses.
        return if state.terminal.shell_focused() {
            KeyOutcome::Handled
        } else {
            KeyOutcome::NotMine
        };
    }

    if let Some(confirm) = state.terminal.pending.clone() {
        state.dirty = true;
        return match key.code {
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                state.terminal.pending = None;
                match confirm {
                    Confirm::Quit { cancel, .. } => KeyOutcome::Quit { cancel },
                    Confirm::Close { .. } => {
                        state.terminal.close_active();
                        KeyOutcome::Handled
                    }
                }
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                state.terminal.pending = None;
                KeyOutcome::Handled
            }
            _ => KeyOutcome::Handled,
        };
    }

    let bindings = &state.config.keymap.terminal;
    let is = |action: &str| {
        bindings
            .get(action)
            .and_then(|b| KeyChord::parse(b))
            .is_some_and(|chord| chord.matches(key))
    };

    if is("toggle") {
        let result = state.terminal.toggle(&state.config.terminal, &state.cwd);
        after_focus_change(state, result);
        return KeyOutcome::Handled;
    }
    if is("focus") {
        let result = state
            .terminal
            .switch_focus(&state.config.terminal, &state.cwd);
        after_focus_change(state, result);
        return KeyOutcome::Handled;
    }
    if !state.terminal.shell_focused() {
        return KeyOutcome::NotMine;
    }

    state.dirty = true;
    let panel = &mut state.terminal;
    if is("next_shell") {
        panel.cycle(1);
    } else if is("prev_shell") {
        panel.cycle(-1);
    } else if is("scroll_up") || is("scroll_down") {
        if let Some(session) = panel.sessions.get(panel.active) {
            let page = session.screen().screen().size().0.max(2) as isize / 2;
            session.scroll(if is("scroll_up") { page } else { -page });
        }
    } else if let Some(session) = panel.sessions.get(panel.active) {
        let application_cursor = session.screen().screen().application_cursor();
        if let Some(bytes) = keys::encode(key, application_cursor) {
            session.scroll_to_bottom();
            session.write(bytes);
        }
    }
    KeyOutcome::Handled
}

/// Common tail of the toggle and focus keys: report a failure to start a
/// shell, and leave any half-typed command or search when the shell takes the
/// keyboard, since the keys that would finish it now go to the shell.
fn after_focus_change(state: &mut AppState, result: Result<(), String>) {
    if let Err(e) = result {
        state.set_error(format!("terminal: {e}"));
    }
    if state.terminal.shell_focused() && !matches!(state.mode, Mode::Navigation) {
        state.mode = Mode::Navigation;
        state.filter = None;
    }
    state.pending_nav_key = None;
    state.dirty = true;
}

/// Runs a `:term` command.
pub fn run_command(state: &mut AppState, command: TermCommand) {
    let result = match command {
        TermCommand::Toggle => state.terminal.toggle(&state.config.terminal, &state.cwd),
        TermCommand::New(profile) => {
            state
                .terminal
                .new_shell(&state.config.terminal, profile.as_deref(), &state.cwd)
        }
        TermCommand::Close => {
            if state.terminal.sessions.is_empty() {
                Err("there is no shell to close".to_owned())
            } else {
                let policy = ConfirmPolicy::parse(&state.config.terminal.confirm_close)
                    .unwrap_or(ConfirmPolicy::WhenBusy);
                state.terminal.request_close(policy);
                Ok(())
            }
        }
        TermCommand::Select(n) => state.terminal.select(n),
        TermCommand::Max => {
            if state.terminal.is_visible() {
                state.terminal.toggle_maximized();
                Ok(())
            } else {
                Err("the terminal panel is not open".to_owned())
            }
        }
    };
    if let Err(e) = result {
        state.set_error(format!("term: {e}"));
    }
    state.dirty = true;
}

/// Decides whether a quit (`q`, or `Ctrl+C` when `cancel`) may go ahead now.
/// When it may not, the question is put up and `false` returned; the answer
/// arrives through [`handle_key`] as [`KeyOutcome::Quit`].
pub fn allow_quit(state: &mut AppState, cancel: bool) -> bool {
    let policy = ConfirmPolicy::parse(&state.config.terminal.confirm_quit)
        .unwrap_or(ConfirmPolicy::WhenBusy);
    match state.terminal.quit_confirmation(policy, cancel) {
        Some(confirm) => {
            state.terminal.ask(confirm);
            state.dirty = true;
            false
        }
        None => true,
    }
}
