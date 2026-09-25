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
pub fn navigation(key: KeyEvent, _ctx: &mut InputCtx, state: &AppState) -> Option<Action> {
    if state.pending_delete {
        return match key.code {
            KeyCode::Enter | KeyCode::Char('y') => Some(Action::ConfirmDelete),
            KeyCode::Esc | KeyCode::Char('n') => Some(Action::CancelDelete),
            _ => None,
        };
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
        // Shift+arrow scrolls the preview — checked before the bare arrows,
        // which match on the key code alone and would otherwise swallow it and
        // move the selection instead.
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
            Some(Action::PreviewScrollDown)
        }
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => Some(Action::PreviewScrollUp),
        KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Enter | KeyCode::Right => Some(Action::EnterOrOpen),
        KeyCode::Backspace | KeyCode::Left => Some(Action::GoParent),
        // Ctrl+C cancels Trail without writing --cwd-file.
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Cancel),
        // Phase 8: tab management built-in fallbacks.
        // These fire when not overridden by a configured keymap binding.
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::NewTab),
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::CloseTab)
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => Some(Action::SwitchTabPrev),
        KeyCode::Tab => Some(Action::SwitchTabNext),
        KeyCode::Esc => None,
        _ => None,
    }
}

/// Translates a `KeyEvent` in Search Mode into an `Action`.
///
/// Search Mode is a typing mode, so an unmodified printable character is
/// **always** appended to the query — before the configured keymap is even
/// consulted. Binding a bare character to a movement action here would make
/// that character impossible to search for, which is exactly what the shipped
/// defaults used to do with `j` and `k`: the list moved and the letter never
/// reached the query.
///
/// Everything that is not text therefore drives the mode: arrows and
/// `Ctrl-n`/`Ctrl-p` move, Enter confirms, Esc leaves, Backspace deletes.
/// Those are what `[keymap.search]` may rebind, and
/// `TrailConfig::validate` rejects a single-character binding so the
/// shadowing cannot be reintroduced through config.
pub fn search(key: KeyEvent, keymap: &KeymapConfig) -> Option<Action> {
    if let KeyCode::Char(ch) = key.code {
        // SHIFT is deliberately not excluded: capital letters are text too.
        if !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return Some(Action::SearchAppendChar(ch));
        }
    }

    if let Some(action) = configured_search_action(key, keymap) {
        return Some(action);
    }

    match key.code {
        KeyCode::Esc => Some(Action::ExitMode),
        KeyCode::Enter | KeyCode::Right => Some(Action::SearchConfirm),
        KeyCode::Down => Some(Action::SearchMoveDown),
        KeyCode::Up => Some(Action::SearchMoveUp),
        KeyCode::Backspace => Some(Action::SearchDeleteChar),
        // Readline-style chords, so the hands never have to leave the home row
        // now that j/k are text. Built-in aliases, like Ctrl-h for backspace.
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::SearchMoveDown)
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::SearchMoveUp)
        }
        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::SearchDeleteChar)
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
        "copy_content" => Some(Action::CopyContent),
        "delete" => Some(Action::BeginDelete),
        "enter_search" => Some(Action::EnterSearch),
        "enter_command" => Some(Action::EnterCommand),
        "quit" => Some(Action::Quit),
        "open_with_os" => Some(Action::OpenWithOs),
        // Phase 8: tab management.
        "new_tab" => Some(Action::NewTab),
        "close_tab" => Some(Action::CloseTab),
        "switch_tab_next" => Some(Action::SwitchTabNext),
        "switch_tab_prev" => Some(Action::SwitchTabPrev),
        // Preview pane scrolling.
        "preview_scroll_down" => Some(Action::PreviewScrollDown),
        "preview_scroll_up" => Some(Action::PreviewScrollUp),
        "preview_page_down" => Some(Action::PreviewPageDown),
        "preview_page_up" => Some(Action::PreviewPageUp),
        "preview_scroll_top" => Some(Action::PreviewScrollTop),
        "preview_scroll_bottom" => Some(Action::PreviewScrollBottom),
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

/// Renders `key` in the form `[keymap]` bindings are written in, or `None` for
/// a key with no config spelling.
///
/// A character carries its own shift: crossterm delivers `Shift`+`j` as
/// `Char('J')`, which is why `jump_bottom = "G"` works and why a `shift-`
/// prefix is added only to *named* keys — `shift-down`, `shift-end`. Without
/// that prefix every shifted arrow would be indistinguishable from the bare
/// one, and `Shift`+`↓` could not scroll the preview while `↓` moves the
/// selection.
fn key_to_config_string(key: KeyEvent) -> Option<String> {
    let named = match key.code {
        KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return Some(format!("ctrl-{ch}").to_ascii_lowercase());
        }
        KeyCode::Char(ch) => return Some(ch.to_string()),
        KeyCode::Enter => "enter",
        KeyCode::Esc => "esc",
        KeyCode::Backspace => "backspace",
        KeyCode::Tab => "tab",
        // Terminals split on how they report Shift+Tab: some send Tab with the
        // shift modifier, others a distinct BackTab with no modifier. Both must
        // reach the same binding.
        KeyCode::BackTab => return Some("shift-tab".to_owned()),
        KeyCode::Left => "left",
        KeyCode::Right => "right",
        KeyCode::Up => "up",
        KeyCode::Down => "down",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        _ => return None,
    };

    if key.modifiers.contains(KeyModifiers::SHIFT) {
        Some(format!("shift-{named}"))
    } else {
        Some(named.to_owned())
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

    /// The bug this guards: `j` and `k` moved the selection instead of being
    /// typed, so neither letter could appear in a search query. A configured
    /// binding must not be able to reintroduce that.
    #[test]
    fn search_types_every_bare_character_even_when_bound() {
        let mut keymap = crate::config::load(None).unwrap().keymap;
        keymap.search.insert("move_down".to_owned(), "j".to_owned());
        keymap.search.insert("move_up".to_owned(), "k".to_owned());

        for ch in ['j', 'k', 'q', ':', '/', 'Z'] {
            assert_eq!(
                search(key(KeyCode::Char(ch)), &keymap),
                Some(Action::SearchAppendChar(ch)),
                "{ch} must reach the query"
            );
        }
    }

    #[test]
    fn search_moves_with_keys_that_are_not_text() {
        let keymap = crate::config::load(None).unwrap().keymap;
        let ctrl = |ch| KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        assert_eq!(
            search(key(KeyCode::Down), &keymap),
            Some(Action::SearchMoveDown)
        );
        assert_eq!(
            search(key(KeyCode::Up), &keymap),
            Some(Action::SearchMoveUp)
        );
        assert_eq!(search(ctrl('n'), &keymap), Some(Action::SearchMoveDown));
        assert_eq!(search(ctrl('p'), &keymap), Some(Action::SearchMoveUp));
        assert_eq!(search(ctrl('h'), &keymap), Some(Action::SearchDeleteChar));
        assert_eq!(
            search(key(KeyCode::Enter), &keymap),
            Some(Action::SearchConfirm)
        );
        assert_eq!(search(key(KeyCode::Esc), &keymap), Some(Action::ExitMode));
    }

    #[test]
    fn shifted_characters_are_still_text() {
        let keymap = crate::config::load(None).unwrap().keymap;
        let shifted = KeyEvent {
            code: KeyCode::Char('K'),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            search(shifted, &keymap),
            Some(Action::SearchAppendChar('K'))
        );
    }

    /// A shifted character carries its own capital, so `J`/`K` reach the
    /// preview-scroll actions through the ordinary configured path — the same
    /// one that already serves `G`.
    #[test]
    fn preview_scroll_bindings_resolve_from_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(dir.path().to_owned()).unwrap();
        let mut ctx = InputCtx::default();
        let ctrl = |ch| KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let shifted = |code| KeyEvent {
            code,
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        for (event, expected) in [
            (key(KeyCode::Char('J')), Action::PreviewScrollDown),
            (key(KeyCode::Char('K')), Action::PreviewScrollUp),
            (ctrl('f'), Action::PreviewPageDown),
            (ctrl('b'), Action::PreviewPageUp),
            (shifted(KeyCode::Home), Action::PreviewScrollTop),
            (shifted(KeyCode::End), Action::PreviewScrollBottom),
        ] {
            assert_eq!(
                navigation(event, &mut ctx, &state),
                Some(expected.clone()),
                "{event:?} must map to {expected:?}"
            );
        }
    }

    /// The bug this guards: the built-in arrow fallbacks match on the key code
    /// alone, so `Shift+↓` used to move the selection. It must scroll the
    /// preview while the bare arrow still moves, and `j`/`k` must be untouched.
    #[test]
    fn shift_arrows_scroll_the_preview_and_bare_arrows_still_move() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(dir.path().to_owned()).unwrap();
        let mut ctx = InputCtx::default();
        let shifted = |code| KeyEvent {
            code,
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        assert_eq!(
            navigation(shifted(KeyCode::Down), &mut ctx, &state),
            Some(Action::PreviewScrollDown)
        );
        assert_eq!(
            navigation(shifted(KeyCode::Up), &mut ctx, &state),
            Some(Action::PreviewScrollUp)
        );
        assert_eq!(
            navigation(key(KeyCode::Down), &mut ctx, &state),
            Some(Action::MoveDown)
        );
        assert_eq!(
            navigation(key(KeyCode::Up), &mut ctx, &state),
            Some(Action::MoveUp)
        );
        assert_eq!(
            navigation(key(KeyCode::Char('j')), &mut ctx, &state),
            Some(Action::MoveDown)
        );
        assert_eq!(
            navigation(key(KeyCode::Char('k')), &mut ctx, &state),
            Some(Action::MoveUp)
        );
    }

    #[test]
    fn shift_is_spelled_out_for_named_keys_only() {
        let shifted = |code| KeyEvent {
            code,
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        assert_eq!(
            key_to_config_string(shifted(KeyCode::Down)),
            Some("shift-down".to_owned())
        );
        assert_eq!(
            key_to_config_string(shifted(KeyCode::End)),
            Some("shift-end".to_owned())
        );
        assert_eq!(
            key_to_config_string(key(KeyCode::Home)),
            Some("home".to_owned())
        );
        // A capital is already the shifted form of the character.
        assert_eq!(
            key_to_config_string(shifted(KeyCode::Char('J'))),
            Some("J".to_owned())
        );
        // Terminals that send BackTab instead of Shift+Tab reach the same binding.
        assert_eq!(
            key_to_config_string(key(KeyCode::BackTab)),
            Some("shift-tab".to_owned())
        );
        assert_eq!(
            key_to_config_string(shifted(KeyCode::Tab)),
            Some("shift-tab".to_owned())
        );
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
