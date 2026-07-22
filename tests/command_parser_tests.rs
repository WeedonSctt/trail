//! Integration tests for the Command Mode grammar, validation, history, and
//! completion (Phase 3).
//!
//! These tests exercise the public API of `trail::input::command_parser`
//! from outside the crate, ensuring the grammar is correct and that error
//! messages are helpful rather than cryptic.

use std::fs;

use trail::input::command_parser::{completions, parse, CommandHistory, ParseError, ParsedCommand};

// ── Grammar: valid inputs ─────────────────────────────────────────────────────

#[test]
fn mkdir_valid_simple_name() {
    let cmd = parse("mkdir new_dir", false).unwrap();
    assert_eq!(cmd, ParsedCommand::Mkdir("new_dir".to_owned()));
}

#[test]
fn touch_valid_filename() {
    let cmd = parse("touch README.md", false).unwrap();
    assert_eq!(cmd, ParsedCommand::Touch("README.md".to_owned()));
}

#[test]
fn rename_valid() {
    let cmd = parse("rename new-name.txt", false).unwrap();
    assert_eq!(cmd, ParsedCommand::Rename("new-name.txt".to_owned()));
}

#[test]
fn rename_alias_ren_works() {
    let cmd = parse("ren new-name.txt", false).unwrap();
    assert_eq!(cmd, ParsedCommand::Rename("new-name.txt".to_owned()));
}

#[test]
fn mv_accepts_relative_path() {
    let cmd = parse("mv ../sibling", false).unwrap();
    assert_eq!(cmd, ParsedCommand::Mv("../sibling".to_owned()));
}

#[test]
fn cp_accepts_path_with_extension() {
    let cmd = parse("cp backup.tar.gz", false).unwrap();
    assert_eq!(cmd, ParsedCommand::Cp("backup.tar.gz".to_owned()));
}

#[test]
fn git_valid_subcommand() {
    let cmd = parse("git status --short", false).unwrap();
    assert_eq!(cmd, ParsedCommand::Git("status --short".to_owned()));
}

#[test]
fn set_valid_key_value() {
    let cmd = parse("set text_sync_threshold_kb 500", false).unwrap();
    assert_eq!(
        cmd,
        ParsedCommand::Set {
            key: "text_sync_threshold_kb".to_owned(),
            value: "500".to_owned(),
        }
    );
}

#[test]
fn shell_command_with_bang() {
    let cmd = parse("ls -la /tmp", true).unwrap();
    assert_eq!(cmd, ParsedCommand::Shell("ls -la /tmp".to_owned()));
}

// ── Grammar: invalid / error inputs ───────────────────────────────────────────

#[test]
fn empty_buffer_returns_error() {
    let err = parse("", false).unwrap_err();
    assert!(
        matches!(err, ParseError::Empty),
        "expected Empty, got {err:?}"
    );
}

#[test]
fn unknown_verb_returns_helpful_error() {
    let err = parse("zap foo", false).unwrap_err();
    assert!(matches!(err, ParseError::UnknownVerb(_)));
    // The error message should mention the verb.
    let msg = err.to_string();
    assert!(
        msg.contains("zap"),
        "error message should name the unknown verb; got: {msg}"
    );
}

#[test]
fn mkdir_empty_arg_returns_error() {
    let err = parse("mkdir", false).unwrap_err();
    assert!(matches!(err, ParseError::MissingArgument(_)));
}

#[test]
fn mkdir_with_slash_returns_invalid_arg() {
    let err = parse("mkdir foo/bar", false).unwrap_err();
    assert!(matches!(err, ParseError::InvalidArgument(_)));
}

#[test]
fn rename_with_backslash_is_invalid_on_windows_semantics() {
    let err = parse("rename foo\\bar", false).unwrap_err();
    assert!(matches!(err, ParseError::InvalidArgument(_)));
}

#[test]
fn git_without_subcommand_is_missing_argument() {
    let err = parse("git", false).unwrap_err();
    assert!(matches!(err, ParseError::MissingArgument(_)));
}

#[test]
fn set_without_value_is_missing_argument() {
    let err = parse("set only_key", false).unwrap_err();
    assert!(matches!(err, ParseError::MissingArgument(_)));
}

#[test]
fn shell_empty_string_is_missing_argument() {
    let err = parse("   ", true).unwrap_err();
    assert!(matches!(err, ParseError::MissingArgument(_)));
}

// ── Completion ────────────────────────────────────────────────────────────────

#[test]
fn verb_completion_prefix_t_returns_touch() {
    let dir = tempfile::tempdir().unwrap();
    let candidates = completions("t", dir.path(), false);
    assert!(
        candidates.contains(&"touch ".to_owned()),
        "expected 'touch ' in: {candidates:?}"
    );
}

#[test]
fn verb_completion_mk_returns_only_mkdir() {
    let dir = tempfile::tempdir().unwrap();
    let candidates = completions("mk", dir.path(), false);
    assert_eq!(candidates, vec!["mkdir "]);
}

#[test]
fn verb_completion_all_verbs_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let candidates = completions("", dir.path(), false);
    assert_eq!(
        candidates.len(),
        7,
        "all 7 verbs should be returned for empty prefix; got {candidates:?}"
    );
}

#[test]
fn path_completion_for_cp_matches_prefix() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("alpha.txt"), b"").unwrap();
    fs::write(dir.path().join("beta.txt"), b"").unwrap();

    let candidates = completions("cp a", dir.path(), false);
    assert!(
        candidates.contains(&"alpha.txt".to_owned()),
        "expected alpha.txt in: {candidates:?}"
    );
    assert!(
        !candidates.contains(&"beta.txt".to_owned()),
        "beta.txt should not match 'a' prefix"
    );
}

#[test]
fn no_completion_for_rename_arg() {
    // rename takes a simple name, not a path — no path completion.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("foo.txt"), b"").unwrap();
    let candidates = completions("rename f", dir.path(), false);
    assert!(
        candidates.is_empty(),
        "rename should not have path completion; got {candidates:?}"
    );
}

#[test]
fn shell_mode_returns_no_completions() {
    let dir = tempfile::tempdir().unwrap();
    let candidates = completions("ls", dir.path(), true);
    assert!(
        candidates.is_empty(),
        "shell mode should not complete verbs; got {candidates:?}"
    );
}

// ── History ───────────────────────────────────────────────────────────────────

#[test]
fn history_push_retains_commands_in_order() {
    let mut h = CommandHistory::new();
    h.push("mkdir alpha".to_owned());
    h.push("touch beta.txt".to_owned());
    h.push("rename gamma.txt".to_owned());
    assert_eq!(h.prev(0), Some("rename gamma.txt"));
    assert_eq!(h.prev(1), Some("touch beta.txt"));
    assert_eq!(h.prev(2), Some("mkdir alpha"));
}

#[test]
fn history_deduplicates_consecutive_entries() {
    let mut h = CommandHistory::new();
    h.push("mkdir foo".to_owned());
    h.push("mkdir foo".to_owned());
    assert_eq!(h.len(), 1);
}

#[test]
fn history_persists_to_file_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.txt");

    {
        let mut h = CommandHistory::with_path(path.clone());
        h.push("mkdir a".to_owned());
        h.push("touch b.txt".to_owned());
    }

    let h2 = CommandHistory::with_path(path);
    assert_eq!(h2.len(), 2);
    assert_eq!(h2.prev(0), Some("touch b.txt"));
    assert_eq!(h2.prev(1), Some("mkdir a"));
}

#[test]
fn history_prev_out_of_range_returns_none() {
    let mut h = CommandHistory::new();
    h.push("mkdir foo".to_owned());
    assert!(h.prev(999).is_none());
}

// ── Action integration: ExecuteCommand via AppState ───────────────────────────

#[test]
fn execute_mkdir_creates_dir_and_refreshes() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();
    let before = state.visible_count();

    trail::actions::apply(
        trail::actions::Action::ExecuteCommand(ParsedCommand::Mkdir("new_subdir".to_owned())),
        &mut state,
    )
    .unwrap();

    assert!(
        state.error_message.is_none(),
        "no error expected; got {:?}",
        state.error_message
    );
    assert_eq!(
        state.visible_count(),
        before + 1,
        "listing should contain the new directory"
    );
}

#[test]
fn execute_touch_creates_file_and_refreshes() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();
    let before = state.visible_count();

    trail::actions::apply(
        trail::actions::Action::ExecuteCommand(ParsedCommand::Touch("new_file.txt".to_owned())),
        &mut state,
    )
    .unwrap();

    assert!(state.error_message.is_none());
    assert_eq!(state.visible_count(), before + 1);
}

#[test]
fn execute_rename_renames_selected_entry() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("old.txt"), b"").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();
    // Select old.txt (it's the only file; selection index 0).
    assert_eq!(
        state.selected_entry().map(|e| e.file_name.as_str()),
        Some("old.txt")
    );

    trail::actions::apply(
        trail::actions::Action::ExecuteCommand(ParsedCommand::Rename("new.txt".to_owned())),
        &mut state,
    )
    .unwrap();

    assert!(state.error_message.is_none());
    assert!(
        state.visible_entries().any(|e| e.file_name == "new.txt"),
        "new.txt should appear in listing"
    );
}

#[test]
fn execute_mkdir_duplicate_surfaces_error_message() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("existing")).unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(
        trail::actions::Action::ExecuteCommand(ParsedCommand::Mkdir("existing".to_owned())),
        &mut state,
    )
    .unwrap();

    assert!(
        state.error_message.is_some(),
        "an error message should be set for duplicate mkdir"
    );
}

// ── Delete confirmation flow ──────────────────────────────────────────────────

#[test]
fn begin_delete_sets_pending_delete() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("to_delete.txt"), b"").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::BeginDelete, &mut state).unwrap();
    assert!(state.pending_delete);
}

#[test]
fn confirm_delete_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("to_delete.txt"), b"").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::BeginDelete, &mut state).unwrap();
    trail::actions::apply(trail::actions::Action::ConfirmDelete, &mut state).unwrap();

    assert!(!state.pending_delete);
    assert!(
        !state
            .visible_entries()
            .any(|e| e.file_name == "to_delete.txt"),
        "to_delete.txt should be gone after ConfirmDelete"
    );
}

#[test]
fn cancel_delete_clears_pending_without_deleting() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("safe.txt"), b"").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::BeginDelete, &mut state).unwrap();
    trail::actions::apply(trail::actions::Action::CancelDelete, &mut state).unwrap();

    assert!(!state.pending_delete);
    assert!(
        state.visible_entries().any(|e| e.file_name == "safe.txt"),
        "safe.txt must still exist after CancelDelete"
    );
}

// ── Clipboard actions ─────────────────────────────────────────────────────────

#[test]
fn copy_abs_path_stores_in_last_yank() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.txt"), b"").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    // Select file.txt (index 0 in a single-file dir).
    trail::actions::apply(trail::actions::Action::CopyAbsPath, &mut state).unwrap();
    let yank = state.last_yank.as_deref().expect("last_yank should be set");
    assert!(
        yank.contains("file.txt"),
        "yanked absolute path should contain the filename; got: {yank}"
    );
}

#[test]
fn copy_filename_stores_just_name() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("unique_name.rs"), b"").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::CopyFilename, &mut state).unwrap();
    assert_eq!(
        state.last_yank.as_deref(),
        Some("unique_name.rs"),
        "CopyFilename should store only the filename"
    );
}

#[test]
fn copy_rel_path_is_relative_to_cwd() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("rel.txt"), b"").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::CopyRelPath, &mut state).unwrap();
    let yank = state.last_yank.as_deref().expect("last_yank should be set");
    // The relative path from cwd to a file in cwd is just the filename.
    assert_eq!(yank, "rel.txt");
}
