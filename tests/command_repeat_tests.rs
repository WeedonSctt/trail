//! `@`: run the last command again (§2.21 of `upcoming_features.md`).

use std::fs;

use trail::actions::{apply, Action};
use trail::app::state::AppState;

#[test]
fn at_runs_the_newest_command_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    // History holds what was typed after the `:`.
    state.command_history.push("mkdir first".to_owned());
    state
        .command_history
        .push("touch made-by-repeat".to_owned());

    apply(Action::RepeatLastCommand, &mut state).unwrap();

    assert!(dir.path().join("made-by-repeat").is_file());
    assert!(!dir.path().join("first").exists(), "only the newest runs");
}

/// The Vim `.` behaviour the plan asks for: a repeated `:mv` acts on whatever
/// is selected now, so `:mv archive`, `j`, `@` files the next entry too.
#[test]
fn a_repeated_mv_follows_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("archive")).unwrap();
    fs::write(dir.path().join("a.txt"), b"").unwrap();
    fs::write(dir.path().join("b.txt"), b"").unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.command_history.push("mv archive".to_owned());

    // Listing: archive/, a.txt, b.txt. Select a.txt and repeat, twice.
    apply(Action::MoveDown, &mut state).unwrap();
    apply(Action::RepeatLastCommand, &mut state).unwrap();
    apply(Action::RepeatLastCommand, &mut state).unwrap();

    assert!(dir.path().join("archive/a.txt").exists());
    assert!(dir.path().join("archive/b.txt").exists());
}

#[test]
fn with_no_history_it_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    apply(Action::RepeatLastCommand, &mut state).unwrap();

    let notice = state.notice.as_ref().expect("a hint").text.clone();
    assert!(notice.contains("no command to repeat"), "{notice}");
    assert_eq!(state.error_text(), None, "an empty history is not an error");
}

#[test]
fn a_repeated_shell_command_keeps_its_bang() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.command_history.push("!echo hi".to_owned());

    apply(Action::RepeatLastCommand, &mut state).unwrap();

    assert!(
        matches!(state.pending_external, Some(Action::RunExternal { .. })),
        "it goes to the shell, not to the `:` parser"
    );
}
