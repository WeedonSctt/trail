//! `:config write`: the session's `:set` changes saved into `trail.toml`
//! (§5.1 of `upcoming_features.md`).

use std::fs;

use trail::actions::{apply, Action};
use trail::app::state::AppState;
use trail::input::command_parser::ParsedCommand;

fn run(state: &mut AppState, cmd: ParsedCommand) {
    apply(Action::ExecuteCommand(cmd), state).unwrap();
}

fn set(state: &mut AppState, key: &str, value: &str) {
    run(
        state,
        ParsedCommand::Set {
            key: key.to_owned(),
            value: value.to_owned(),
        },
    );
}

#[test]
fn set_then_write_saves_into_the_loaded_file_with_its_comments() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("trail.toml");
    fs::write(
        &file,
        "# keep this comment\n[navigation]\nsort_by = \"name\"\n",
    )
    .unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.settings.file = Some(file.clone());

    set(&mut state, "sort_by", "time");
    set(&mut state, "navigation.sort_by", "size"); // the same key, spelled out
    set(&mut state, "show_version", "on");
    run(&mut state, ParsedCommand::ConfigWrite);

    assert_eq!(state.error_text(), None, "{:?}", state.notice);
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("# keep this comment"), "{text}");
    assert!(
        text.contains("sort_by = \"size\""),
        "the last value wins: {text}"
    );
    assert!(!text.contains("time"), "{text}");
    assert!(text.contains("show_version = true"), "{text}");
    let notice = state.notice.as_ref().unwrap().text.clone();
    assert!(notice.starts_with("wrote 2 setting(s)"), "{notice}");

    let reloaded = trail::config::load(Some(&file)).unwrap();
    assert_eq!(reloaded.navigation.sort_by, "size");
}

#[test]
fn writing_twice_writes_only_what_changed_since() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("trail.toml");
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.settings.file = Some(file.clone());

    set(&mut state, "scroll_margin", "1");
    run(&mut state, ParsedCommand::ConfigWrite);
    run(&mut state, ParsedCommand::ConfigWrite);

    let notice = state.notice.as_ref().unwrap().text.clone();
    assert!(notice.contains("nothing to write"), "{notice}");
}

#[test]
fn a_rejected_set_is_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("trail.toml");
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.settings.file = Some(file.clone());

    set(&mut state, "sort_by", "colour");
    assert!(state.error_text().is_some());
    run(&mut state, ParsedCommand::ConfigWrite);

    assert!(
        !file.exists(),
        "nothing valid was set, so nothing is written"
    );
}
