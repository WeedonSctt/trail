//! `@`: do the last file operation again (§2.21 of `upcoming_features.md`) —
//! a `:`/`!` command, or one of the keys that change files: `dd`, `pp`, `pm`.

use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use trail::actions::transfer::TransferKind;
use trail::actions::{apply, Action, Repeatable};
use trail::app::state::AppState;

/// Types `line` into Command Mode and presses Enter, as a user would.
fn type_command(state: &mut AppState, line: &str) {
    apply(Action::EnterCommand, state).unwrap();
    for ch in line.chars() {
        let key = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
        apply(Action::CommandKey(key), state).unwrap();
    }
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    apply(Action::CommandKey(enter), state).unwrap();
}

#[test]
fn at_runs_the_last_typed_command_again() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("archive")).unwrap();
    fs::write(dir.path().join("a.txt"), b"").unwrap();
    fs::write(dir.path().join("b.txt"), b"").unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    // Listing: archive/, a.txt, b.txt. `:mv archive` on a.txt, then `@`: the
    // selection has fallen to b.txt, so it follows — Vim's `.`.
    apply(Action::MoveDown, &mut state).unwrap();
    type_command(&mut state, "mv archive");
    apply(Action::RepeatLastCommand, &mut state).unwrap();

    assert!(dir.path().join("archive/a.txt").exists());
    assert!(dir.path().join("archive/b.txt").exists());
}

#[test]
fn at_repeats_dd_and_still_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), b"").unwrap();
    fs::write(dir.path().join("b.txt"), b"").unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.config.general.delete_mode = "permanent".to_owned();

    apply(Action::BeginDelete, &mut state).unwrap();
    apply(Action::ConfirmDelete, &mut state).unwrap();
    assert!(!dir.path().join("a.txt").exists());

    apply(Action::RepeatLastCommand, &mut state).unwrap();
    assert!(state.pending_delete, "a repeated delete asks again");
    assert!(dir.path().join("b.txt").exists(), "nothing gone until `y`");
    apply(Action::ConfirmDelete, &mut state).unwrap();
    assert!(!dir.path().join("b.txt").exists());
}

#[test]
fn at_repeats_a_paste_into_another_directory() {
    let dir = tempfile::tempdir().unwrap();
    for sub in ["src", "one", "two"] {
        fs::create_dir(dir.path().join(sub)).unwrap();
    }
    fs::write(dir.path().join("src/a.txt"), b"a").unwrap();
    let mut state = AppState::new(dir.path().join("src")).unwrap();
    apply(Action::ToggleMark, &mut state).unwrap();

    state.enter_dir(dir.path().join("one")).unwrap();
    apply(Action::PasteMarks(TransferKind::Copy), &mut state).unwrap();
    state.enter_dir(dir.path().join("two")).unwrap();
    apply(Action::RepeatLastCommand, &mut state).unwrap();

    assert!(dir.path().join("one/a.txt").exists());
    assert!(dir.path().join("two/a.txt").exists());
}

/// Whichever operation came last is the one repeated, command or key.
#[test]
fn the_newest_operation_wins_and_moving_does_not_count() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    type_command(&mut state, "mkdir x");
    apply(Action::BeginDelete, &mut state).unwrap();
    apply(Action::CancelDelete, &mut state).unwrap();
    apply(Action::MoveDown, &mut state).unwrap();

    assert_eq!(
        state.last_repeatable,
        Some(Repeatable::Key(Action::BeginDelete)),
        "the later `dd` replaced the command; `j` replaced nothing"
    );
}

#[test]
fn with_nothing_done_yet_it_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    apply(Action::RepeatLastCommand, &mut state).unwrap();

    let notice = state.notice.as_ref().expect("a hint").text.clone();
    assert!(notice.contains("nothing to repeat"), "{notice}");
    assert_eq!(state.error_text(), None, "it is not an error");
}

#[test]
fn a_repeated_shell_command_keeps_its_bang() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.last_repeatable = Some(Repeatable::CommandLine("!echo hi".to_owned()));

    apply(Action::RepeatLastCommand, &mut state).unwrap();

    assert!(
        matches!(state.pending_external, Some(Action::RunExternal { .. })),
        "it goes to the shell, not to the `:` parser"
    );
}
