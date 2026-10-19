//! Marks, and pasting them somewhere else (§2.20 of `upcoming_features.md`).
//!
//! These drive [`AppState`] through `actions::apply` with no worker channel
//! attached, so every copy and move runs inline and its outcome is visible as
//! soon as the action returns.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use trail::actions::transfer::TransferKind;
use trail::actions::{apply, Action};
use trail::app::state::AppState;
use trail::input::command_parser::ParsedCommand;

/// ```text
/// <tmp>/
///   dst/
///   src/
///     a.txt   "a"
///     b.txt   "b"
///     c.txt   "c"
/// ```
fn layout() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::create_dir(dir.path().join("dst")).unwrap();
    for name in ["a", "b", "c"] {
        fs::write(src.join(format!("{name}.txt")), name).unwrap();
    }
    dir
}

/// A state in `<tmp>/src` with `a.txt` and `b.txt` marked.
fn marked_in_src(dir: &Path) -> AppState {
    let mut state = AppState::new(dir.join("src")).unwrap();
    apply(Action::ToggleMark, &mut state).unwrap();
    apply(Action::ToggleMark, &mut state).unwrap();
    state
}

fn go(state: &mut AppState, path: impl AsRef<Path>) {
    state.enter_dir(path.as_ref().to_owned()).unwrap();
}

#[test]
fn space_marks_the_selection_and_moves_down() {
    let dir = layout();
    let state = marked_in_src(dir.path());

    assert_eq!(state.marks.len(), 2);
    assert!(state.marks.contains(&state.entries[0].path));
    assert!(state.marks.contains(&state.entries[1].path));
    assert_eq!(state.selected, 2, "each mark moves the selection on");
}

#[test]
fn space_on_a_marked_entry_unmarks_it() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    apply(Action::JumpTop, &mut state).unwrap();
    apply(Action::ToggleMark, &mut state).unwrap();
    assert_eq!(state.marks.len(), 1);
}

#[test]
fn marks_survive_navigation_and_tab_switches() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());

    go(&mut state, dir.path().join("dst"));
    assert_eq!(state.marks.len(), 2);

    apply(Action::NewTab, &mut state).unwrap();
    assert_eq!(state.marks.len(), 2, "one set for every tab");
    apply(Action::SwitchTabNext, &mut state).unwrap();
    assert_eq!(state.marks.len(), 2);
}

#[test]
fn esc_clears_the_marks() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    apply(Action::ClearMarks, &mut state).unwrap();
    assert!(state.marks.is_empty());
}

#[test]
fn copy_here_copies_and_keeps_the_marks() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    go(&mut state, dir.path().join("dst"));

    apply(Action::PasteMarks(TransferKind::Copy), &mut state).unwrap();

    let dst = dir.path().join("dst");
    assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"a");
    assert_eq!(fs::read(dst.join("b.txt")).unwrap(), b"b");
    assert!(dir.path().join("src/a.txt").exists());
    assert_eq!(
        state.marks.len(),
        2,
        "a copy keeps the set for another paste"
    );
    assert_eq!(state.error_text(), None);
    assert!(
        state.entries.iter().any(|e| e.file_name == "a.txt"),
        "the listing shows what was pasted"
    );
}

#[test]
fn move_here_moves_and_clears_the_moved_marks() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    go(&mut state, dir.path().join("dst"));

    apply(Action::PasteMarks(TransferKind::Move), &mut state).unwrap();

    assert!(dir.path().join("dst/a.txt").exists());
    assert!(!dir.path().join("src/a.txt").exists());
    assert!(state.marks.is_empty());
}

#[test]
fn a_taken_name_is_skipped_and_reported() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    let dst = dir.path().join("dst");
    fs::write(dst.join("a.txt"), b"keep me").unwrap();
    go(&mut state, &dst);

    apply(Action::PasteMarks(TransferKind::Move), &mut state).unwrap();

    assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"keep me");
    assert!(
        dir.path().join("src/a.txt").exists(),
        "the skipped one stays put"
    );
    let left: Vec<_> = state.marks.paths().collect();
    assert_eq!(left.len(), 1, "only the moved entry leaves the set");
    assert!(left[0].ends_with("a.txt"), "{left:?}");
    let notice = state.notice.as_ref().expect("a report").text.clone();
    assert!(notice.contains("1 moved, 1 skipped: exists"), "{notice}");
}

#[test]
fn a_mark_whose_file_vanished_is_dropped_and_counted() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    fs::remove_file(dir.path().join("src/b.txt")).unwrap();
    go(&mut state, dir.path().join("dst"));

    apply(Action::PasteMarks(TransferKind::Copy), &mut state).unwrap();

    assert_eq!(state.marks.len(), 1, "the vanished mark is gone");
    let notice = state.notice.as_ref().expect("a report").text.clone();
    assert!(notice.contains("1 copied, 1 gone"), "{notice}");
}

#[test]
fn pasting_with_nothing_marked_says_how_to_mark() {
    let dir = layout();
    let mut state = AppState::new(dir.path().join("dst")).unwrap();
    apply(Action::PasteMarks(TransferKind::Copy), &mut state).unwrap();
    let notice = state.notice.as_ref().expect("a hint").text.clone();
    assert!(notice.contains("Space"), "{notice}");
}

// ── :mv, :cp and dd act on the marks ─────────────────────────────────────────

fn run(state: &mut AppState, cmd: ParsedCommand) {
    apply(Action::ExecuteCommand(cmd), state).unwrap();
}

#[test]
fn mv_with_marks_moves_the_marks_not_the_selection() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    // The selection is on c.txt, which is not marked.
    let dst = dir.path().join("dst");
    run(&mut state, ParsedCommand::Mv(dst.display().to_string()));

    assert!(dst.join("a.txt").exists());
    assert!(dst.join("b.txt").exists());
    assert!(dir.path().join("src/c.txt").exists(), "the selection stays");
    assert!(state.marks.is_empty());
}

#[test]
fn cp_with_marks_keeps_them() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    run(&mut state, ParsedCommand::Cp("../dst".to_owned()));

    assert!(dir.path().join("dst/a.txt").exists());
    assert_eq!(state.marks.len(), 2);
}

#[test]
fn several_marks_need_a_directory_to_go_into() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    run(&mut state, ParsedCommand::Mv("one-name.txt".to_owned()));

    let err = state.error_text().expect("refused").to_owned();
    assert!(err.contains("2 entries are marked"), "{err}");
    assert!(dir.path().join("src/a.txt").exists());
}

#[test]
fn dd_with_marks_deletes_them_after_one_confirmation() {
    let dir = layout();
    let mut state = marked_in_src(dir.path());
    // Permanent, so the test does not fill the machine's recycle bin.
    state.config.general.delete_mode = "permanent".to_owned();

    apply(Action::BeginDelete, &mut state).unwrap();
    assert!(state.pending_delete);
    apply(Action::ConfirmDelete, &mut state).unwrap();

    assert!(!dir.path().join("src/a.txt").exists());
    assert!(!dir.path().join("src/b.txt").exists());
    assert!(
        dir.path().join("src/c.txt").exists(),
        "the selection is spared"
    );
    assert!(state.marks.is_empty());
    let notice = state.notice.as_ref().expect("a report").text.clone();
    assert_eq!(notice, "2 deleted");
}
