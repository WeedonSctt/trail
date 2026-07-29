//! Default and user-configured key bindings.
//!
//! Resolves `[keymap]` TOML tables to `Action` values for each mode. Arrow
//! keys and Enter remain built-in aliases for ergonomic terminal navigation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::actions::Action;
use crate::app::state::AppState;
use crate::config::KeymapConfig;
use crate::input::InputCtx;

/// Translates a `KeyEvent` in Navigation Mode into an `Action`.
///
/// Configured bindings are resolved first from `state.config.keymap`. Built-in
/// non-text fallbacks such as arrows, Enter, and Backspace are then checked.
/// Multi-key sequences are represented by compact strings such as `gg`, `ya`,
/// and `dd`.
pub fn navigation(key: KeyEvent, ctx: &mut InputCtx, state: &AppState) -> Option<Action> {
    if state.pending_delete {
        return match key.code {
            KeyCode::Enter | KeyCode::Char('y') => Some(Action::ConfirmDelete),
            KeyCode::Esc | KeyCode::Char('n') => Some(Action::CancelDelete),
            _ => None,
        };
    }

    if ctx.pending_g {
        ctx.pending_g = false;
        if let KeyCode::Char('g') = key.code {
            return Some(Action::JumpTop);
        }
        return None;
    }

    if let Some(pending) = state.pending_nav_key {
        if let Some(sequence) = append_to_sequence(pending, key) {
            return nav_action_for_sequence(&state.config.keymap, &sequence);
        }
        return None;
    }

    if let Some(action) = configured_nav_action(key, &state.config.keymap) {
        return Some(action);
    }

    if let Some(prefix) = configured_nav_prefix(key, &state.config.keymap) {
        return Some(Action::SetPendingNavKey(prefix));
    }

    match key.code {
        KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Char('g') => {
            ctx.pending_g = true;
            None
        }
        KeyCode::Enter | KeyCode::Right => Some(Action::EnterOrOpen),
        KeyCode::Backspace | KeyCode::Left => Some(Action::GoParent),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Esc => None,
        _ => None,
    }
}

/// Translates a `KeyEvent` in Search Mode into an `Action`.
///
/// Printable characters not claimed by the configured search keymap are
/// appended to the active query.
pub fn search(key: KeyEvent, keymap: &KeymapConfig) -> Option<Action> {
    if let Some(action) = configured_search_action(key, keymap) {
        return Some(action);
    }

    match key.code {
        KeyCode::Esc => Some(Action::ExitMode),
        KeyCode::Enter | KeyCode::Right => Some(Action::SearchConfirm),
        KeyCode::Down => Some(Action::SearchMoveDown),
        KeyCode::Up => Some(Action::SearchMoveUp),
        KeyCode::Backspace => Some(Action::SearchDeleteChar),
        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::SearchDeleteChar)
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::SearchAppendChar(ch))
        }
        _ => None,
    }
}

fn configured_nav_action(key: KeyEvent, keymap: &KeymapConfig) -> Option<Action> {
    let key_text = key_to_config_string(key)?;
    nav_action_for_sequence(keymap, &key_text)
}

fn configured_search_action(key: KeyEvent, keymap: &KeymapConfig) -> Option<Action> {
    let key_text = key_to_config_string(key)?;
    keymap.search.iter().find_map(|(name, binding)| {
        if binding == &key_text {
            search_action_from_name(name)
        } else {
            None
        }
    })
}

fn configured_nav_prefix(key: KeyEvent, keymap: &KeymapConfig) -> Option<char> {
    let KeyCode::Char(ch) = key.code else {
        return None;
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }
    keymap
        .navigation
        .values()
        .any(|binding| binding.len() > ch.len_utf8() && binding.starts_with(ch))
        .then_some(ch)
}

fn nav_action_for_sequence(keymap: &KeymapConfig, sequence: &str) -> Option<Action> {
    keymap.navigation.iter().find_map(|(name, binding)| {
        if binding == sequence {
            nav_action_from_name(name)
        } else {
            None
        }
    })
}

fn nav_action_from_name(name: &str) -> Option<Action> {
    match name {
        "move_down" => Some(Action::MoveDown),
        "move_up" => Some(Action::MoveUp),
        "jump_top" => Some(Action::JumpTop),
        "jump_bottom" => Some(Action::JumpBottom),
        "enter_or_open" => Some(Action::EnterOrOpen),
        "go_parent" => Some(Action::GoParent),
        "history_back" => Some(Action::HistoryBack),
        "history_forward" => Some(Action::HistoryForward),
        "refresh" => Some(Action::Refresh),
        "toggle_hidden" => Some(Action::ToggleHidden),
        "copy_absolute_path" => Some(Action::CopyAbsPath),
        "copy_relative_path" => Some(Action::CopyRelPath),
        "copy_filename" => Some(Action::CopyFilename),
        "delete" => Some(Action::BeginDelete),
        "enter_search" => Some(Action::EnterSearch),
        "enter_command" => Some(Action::EnterCommand),
        "quit" => Some(Action::Quit),
        _ => None,
    }
}

fn search_action_from_name(name: &str) -> Option<Action> {
    match name {
        "exit" => Some(Action::ExitMode),
        "confirm" => Some(Action::SearchConfirm),
        "move_down" => Some(Action::SearchMoveDown),
        "move_up" => Some(Action::SearchMoveUp),
        "delete_char" => Some(Action::SearchDeleteChar),
        _ => None,
    }
}

fn append_to_sequence(prefix: char, key: KeyEvent) -> Option<String> {
    let KeyCode::Char(ch) = key.code else {
        return None;
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }
    Some(format!("{prefix}{ch}"))
}

fn key_to_config_string(key: KeyEvent) -> Option<String> {
    match key.code {
        KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(format!("ctrl-{ch}").to_ascii_lowercase())
        }
        KeyCode::Char(ch) => Some(ch.to_string()),
        KeyCode::Enter => Some("enter".to_owned()),
        KeyCode::Esc => Some("esc".to_owned()),
        KeyCode::Backspace => Some("backspace".to_owned()),
        KeyCode::Tab => Some("tab".to_owned()),
        KeyCode::Left => Some("left".to_owned()),
        KeyCode::Right => Some("right".to_owned()),
        KeyCode::Up => Some("up".to_owned()),
        KeyCode::Down => Some("down".to_owned()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn parses_ctrl_key_binding() {
        let event = KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(key_to_config_string(event), Some("ctrl-r".to_owned()));
    }

    #[test]
    fn configured_binding_overrides_default_char() {
        let mut cfg = crate::config::load(None).unwrap();
        cfg.keymap
            .navigation
            .insert("move_down".to_owned(), "n".to_owned());
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::with_config(dir.path().to_owned(), cfg).unwrap();
        let mut ctx = InputCtx::default();
        assert_eq!(
            navigation(key(KeyCode::Char('n')), &mut ctx, &state),
            Some(Action::MoveDown)
        );
    }
}
