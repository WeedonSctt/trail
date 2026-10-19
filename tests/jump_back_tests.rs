//! `U`: return from a jump (§2.23 of `upcoming_features.md`).
//!
//! The maintainer's own example: in `dir1`, jump to a bookmark (`dir2`), wander
//! on to `dir3` and `dir4`, and one key returns to `dir1` — because it returns
//! from jumps, not from every directory entered.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use trail::actions::{apply, Action};
use trail::app::state::AppState;
use trail::input::command_parser::ParsedCommand;
use trail::plugin::bookmarks::BookmarkStore;

/// `<tmp>/dir1`, and `<tmp>/dir2/dir3/dir4` with `dir2` bookmarked as `two`.
fn setup() -> (TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("dir1")).unwrap();
    fs::create_dir_all(dir.path().join("dir2/dir3/dir4")).unwrap();
    let mut store = BookmarkStore::open(dir.path().join("bookmarks.json")).unwrap();
    store
        .add("two".to_owned(), dir.path().join("dir2"))
        .unwrap();

    let mut state = AppState::new(dir.path().join("dir1")).unwrap();
    state.bookmark_store = Some(store);
    (dir, state)
}

fn jump(state: &mut AppState, name: &str) {
    apply(
        Action::ExecuteCommand(ParsedCommand::Jump(name.to_owned())),
        state,
    )
    .unwrap();
}

fn ends_in(state: &AppState, name: &str) -> bool {
    state.cwd.file_name() == Some(name.as_ref())
}

fn enter(state: &mut AppState, path: impl AsRef<Path>) {
    state.enter_dir(path.as_ref().to_owned()).unwrap();
}

#[test]
fn u_capital_returns_from_the_jump_past_everything_since() {
    let (dir, mut state) = setup();
    jump(&mut state, "two");
    enter(&mut state, dir.path().join("dir2/dir3"));
    enter(&mut state, dir.path().join("dir2/dir3/dir4"));

    apply(Action::ReturnFromJump, &mut state).unwrap();

    assert!(ends_in(&state, "dir1"), "{:?}", state.cwd);
}

#[test]
fn ordinary_navigation_records_no_jump() {
    let (dir, mut state) = setup();
    enter(&mut state, dir.path().join("dir2"));

    apply(Action::ReturnFromJump, &mut state).unwrap();

    assert!(ends_in(&state, "dir2"), "nothing to return from");
    let notice = state.notice.as_ref().expect("a hint").text.clone();
    assert!(notice.contains("no jump to return from"), "{notice}");
}

#[test]
fn u_returns_further_back_through_earlier_jumps() {
    let (dir, mut state) = setup();
    jump(&mut state, "two");
    enter(&mut state, dir.path().join("dir2/dir3"));
    // A second jump, from dir3 back to dir2.
    jump(&mut state, "two");

    apply(Action::ReturnFromJump, &mut state).unwrap();
    assert!(ends_in(&state, "dir3"));
    apply(Action::ReturnFromJump, &mut state).unwrap();
    assert!(ends_in(&state, "dir1"));
}

#[test]
fn a_jump_whose_origin_is_gone_is_skipped() {
    let (dir, mut state) = setup();
    jump(&mut state, "two");
    fs::remove_dir(dir.path().join("dir1")).unwrap();

    apply(Action::ReturnFromJump, &mut state).unwrap();

    assert!(ends_in(&state, "dir2"), "stays put");
    let notice = state.notice.as_ref().expect("a report").text.clone();
    assert!(notice.contains("gone"), "{notice}");
}

#[test]
fn each_tab_has_its_own_jumps() {
    let (_dir, mut state) = setup();
    jump(&mut state, "two");
    apply(Action::NewTab, &mut state).unwrap();

    apply(Action::ReturnFromJump, &mut state).unwrap();
    assert!(ends_in(&state, "dir2"), "the new tab made no jump");

    apply(Action::SwitchTabNext, &mut state).unwrap();
    apply(Action::ReturnFromJump, &mut state).unwrap();
    assert!(ends_in(&state, "dir1"), "the first tab still has its jump");
}
