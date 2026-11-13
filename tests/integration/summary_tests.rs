//! The exit summary's counters (§2.26 of `upcoming_features.md`): each moves
//! when the thing it counts happens, and only then.
//!
//! What these cannot show is how the summary looks printed in a real shell,
//! its colours, and the wait after quitting during a long copy.

use std::fs;
use std::path::Path;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use trail::actions::run_report::{RunReport, RunResult};
use trail::actions::transfer::TransferKind;
use trail::actions::{apply, report_run, Action};
use trail::app::state::AppState;
use trail::summary::{summary, ExitSummary, SessionReport};

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

/// `<dir>/a.txt`, `<dir>/b.txt` and an empty `<dir>/dst/`.
fn scratch() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), b"a").unwrap();
    fs::write(dir.path().join("b.txt"), b"b").unwrap();
    fs::create_dir(dir.path().join("dst")).unwrap();
    dir
}

/// Puts the selection on the entry called `name`.
fn select(state: &mut AppState, name: &str) {
    let index = state
        .entries
        .iter()
        .position(|e| e.file_name == name)
        .unwrap_or_else(|| panic!("{name} is not listed"));
    state.selected = index;
}

fn report(state: &AppState) -> SessionReport {
    SessionReport::from_state(state, false, false)
}

#[test]
fn creating_and_renaming_are_counted() {
    let dir = scratch();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    type_command(&mut state, "mkdir new_dir");
    type_command(&mut state, "touch new.txt");
    select(&mut state, "a.txt");
    type_command(&mut state, "rename c.txt");

    let report = report(&state);
    assert_eq!((report.created, report.renamed), (2, 1));
    assert_eq!((report.commands, report.failed_commands), (3, 0));
}

#[test]
fn copies_and_moves_count_the_entries_transferred() {
    let dir = scratch();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    select(&mut state, "a.txt");
    type_command(&mut state, "cp dst");
    select(&mut state, "b.txt");
    type_command(&mut state, "mv dst");

    let report = report(&state);
    assert_eq!((report.copied, report.moved), (1, 1));
    assert!(dir.path().join("dst/a.txt").exists());
    assert!(dir.path().join("dst/b.txt").exists());
}

#[test]
fn deletes_are_counted_by_where_they_went() {
    let dir = scratch();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.config.general.delete_mode = "permanent".to_owned();

    select(&mut state, "a.txt");
    apply(Action::BeginDelete, &mut state).unwrap();
    apply(Action::ConfirmDelete, &mut state).unwrap();

    let report = report(&state);
    assert_eq!((report.deleted, report.trashed), (1, 0));
}

#[test]
fn a_command_that_fails_is_counted_as_failed() {
    let dir = scratch();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    type_command(&mut state, "mkdir dst");
    type_command(&mut state, "frobnicate");
    type_command(&mut state, "cp @no_such_bookmark");
    type_command(&mut state, "mkdir fine");

    let report = report(&state);
    assert_eq!((report.commands, report.failed_commands), (4, 3));
    assert_eq!(report.created, 1, "a failed mkdir creates nothing");
}

/// A `:cp` that copied nothing failed as a command, only after it started.
#[test]
fn a_copy_that_transfers_nothing_is_a_failed_command() {
    let dir = scratch();
    fs::write(dir.path().join("dst/a.txt"), b"taken").unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    select(&mut state, "a.txt");
    type_command(&mut state, "cp dst");

    let report = report(&state);
    assert_eq!((report.copied, report.failed_commands), (0, 1));
}

/// A `!` command's result comes back after the event loop runs it.
#[test]
fn a_shell_command_that_failed_is_counted_when_it_reports() {
    let dir = scratch();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let run = |result| RunReport {
        label: Some("!cargo test".to_owned()),
        program: "cmd.exe".to_owned(),
        result,
        elapsed: Duration::from_secs(1),
    };

    report_run(&mut state, &run(RunResult::Success));
    report_run(&mut state, &run(RunResult::Failed { code: Some(101) }));
    // The editor reports too, but it is not a command.
    report_run(
        &mut state,
        &RunReport {
            label: None,
            ..run(RunResult::Failed { code: Some(1) })
        },
    );

    assert_eq!(report(&state).failed_commands, 1);
}

#[test]
fn a_mark_used_by_a_copy_is_used_though_it_stays_marked() {
    let dir = scratch();
    fs::write(dir.path().join("c.txt"), b"c").unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    for name in ["a.txt", "b.txt"] {
        select(&mut state, name);
        apply(Action::ToggleMark, &mut state).unwrap();
    }
    state.enter_dir(dir.path().join("dst")).unwrap();
    apply(Action::PasteMarks(TransferKind::Copy), &mut state).unwrap();
    assert_eq!(state.marks.len(), 2, "a copy keeps its marks");

    state.go_parent().unwrap();
    select(&mut state, "c.txt");
    apply(Action::ToggleMark, &mut state).unwrap();

    let report = report(&state);
    assert_eq!(report.unused_marks, 1);
    assert_eq!(report.copied, 2);
}

#[test]
fn a_new_shell_is_counted() {
    let dir = scratch();
    let mut config = trail::config::load(None).unwrap();
    let command = if cfg!(windows) {
        vec!["cmd.exe".to_owned(), "/Q".to_owned()]
    } else {
        vec!["/bin/sh".to_owned()]
    };
    config.terminal.profile = vec![trail::config::TerminalProfile {
        name: "sh".to_owned(),
        command,
    }];
    let mut state = AppState::with_config(dir.path().to_owned(), config).unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    // Leaked so the session threads' sends succeed.
    std::mem::forget(rx);
    state.terminal.attach(tx);

    let terminal_config = state.config.terminal.clone();
    state
        .terminal
        .new_shell(&terminal_config, None, dir.path())
        .unwrap();

    let report = report(&state);
    assert_eq!((report.shells_opened, report.shells_open), (1, 1));
}

#[test]
fn the_session_reads_back_as_a_summary() {
    let dir = scratch();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    select(&mut state, "a.txt");
    type_command(&mut state, "cp dst");

    let lines = summary(&report(&state), ExitSummary::Full, 200);

    assert_eq!(lines.len(), 3, "{lines:#?}");
    assert!(lines[0].starts_with("trail "), "{}", lines[0]);
    assert!(lines[0].contains(&trail::pathfmt::display(dir.path())));
    assert_eq!(lines[1], "1 command");
    assert_eq!(lines[2], "1 copied");
}

#[test]
fn the_setting_is_off_by_default_and_checked() {
    let mut config = trail::config::load(None).unwrap();
    assert_eq!(config.general.exit_summary, "off");

    for level in ["warnings", "brief", "full", "off"] {
        config.set_value("exit_summary", level).unwrap();
        assert_eq!(config.general.exit_summary, level);
    }
    let err = config
        .set_value("general.exit_summary", "loud")
        .unwrap_err();
    assert!(format!("{err}").contains("must be off, warnings, brief or full"));
}

#[test]
fn a_config_file_sets_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("trail.toml");
    fs::write(&file, "[general]\nexit_summary = \"brief\"\n").unwrap();
    assert_eq!(
        trail::config::load(Some(&file))
            .unwrap()
            .general
            .exit_summary,
        "brief"
    );

    fs::write(&file, "[general]\nexit_summary = \"loud\"\n").unwrap();
    assert!(trail::config::load(Some(Path::new(&file))).is_err());
}
