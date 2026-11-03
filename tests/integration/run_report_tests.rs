//! The result of an external command, back in Trail (§2.25 of
//! `upcoming_features.md`). The block's wording and its keys are unit-tested in
//! `actions::run_report`, and the run-again loop in `actions::shell_exec`.

use std::time::Duration;

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use trail::actions::run_report::{RunReport, RunResult};
use trail::actions::{apply, report_run, Action};
use trail::app::state::{AppState, NoticeLevel};
use trail::input::command_parser::ParsedCommand;

fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(dir.path().to_owned()).unwrap();
    (dir, state)
}

fn report(label: Option<&str>, result: RunResult) -> RunReport {
    RunReport {
        label: label.map(str::to_owned),
        program: "nvim".to_owned(),
        result,
        elapsed: Duration::from_millis(12_400),
    }
}

/// The label a queued command will be reported under.
fn queued_label(state: &AppState) -> Option<String> {
    match &state.pending_external {
        Some(Action::RunExternal { label, .. }) => label.clone(),
        other => panic!("expected a queued RunExternal, got {other:?}"),
    }
}

#[test]
fn a_failure_is_reported_in_the_error_colour_and_reaches_the_screen() {
    let (_dir, mut state) = state();
    report_run(
        &mut state,
        &report(Some("!cargo test"), RunResult::Failed { code: Some(101) }),
    );

    assert_eq!(
        state.error_text(),
        Some("!cargo test failed: exit 101 (12.4s)")
    );

    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
    trail::ui::render(&mut terminal, &mut state).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(screen.contains("failed: exit 101"), "{screen}");
}

#[test]
fn a_success_is_an_ordinary_notice() {
    let (_dir, mut state) = state();
    report_run(&mut state, &report(Some("!ls"), RunResult::Success));

    let notice = state.notice.as_ref().expect("a report");
    assert_eq!(notice.level, NoticeLevel::Info);
    assert_eq!(notice.text, "!ls ok (12.4s)");
}

#[test]
fn the_editor_says_nothing_unless_it_failed() {
    let (_dir, mut state) = state();
    report_run(&mut state, &report(None, RunResult::Success));
    assert_eq!(state.notice, None);

    report_run(
        &mut state,
        &report(None, RunResult::Failed { code: Some(1) }),
    );
    assert_eq!(state.error_text(), Some("nvim failed: exit 1 (12.4s)"));
}

/// The block's header shows the command as it was typed, not the shell's argv.
#[test]
fn commands_are_labelled_as_typed() {
    let (_dir, mut state) = state();
    apply(
        Action::ExecuteCommand(ParsedCommand::Shell("cargo test".to_owned())),
        &mut state,
    )
    .unwrap();
    assert_eq!(queued_label(&state).as_deref(), Some("!cargo test"));

    apply(
        Action::ExecuteCommand(ParsedCommand::Git("status --short".to_owned())),
        &mut state,
    )
    .unwrap();
    assert_eq!(queued_label(&state).as_deref(), Some(":git status --short"));
}

#[test]
fn opening_a_file_carries_no_label() {
    let (dir, _) = state();
    std::fs::write(dir.path().join("a.txt"), b"").unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    apply(Action::EnterOrOpen, &mut state).unwrap();

    assert_eq!(queued_label(&state), None);
}
