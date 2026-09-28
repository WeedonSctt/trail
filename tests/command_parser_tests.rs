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
fn plugin_valid_action_name() {
    let cmd = parse("plugin my_action", false).unwrap();
    assert_eq!(
        cmd,
        ParsedCommand::Plugin {
            name: "my_action".to_owned(),
            arg: "".to_owned(),
        }
    );
}

#[test]
fn plugin_with_args() {
    let cmd = parse("plugin log_note hello world", false).unwrap();
    assert_eq!(
        cmd,
        ParsedCommand::Plugin {
            name: "log_note".to_owned(),
            arg: "hello world".to_owned(),
        }
    );
}

#[test]
fn plugin_without_action_returns_error() {
    let err = parse("plugin", false).unwrap_err();
    assert!(matches!(err, ParseError::MissingArgument(_)));
}

#[test]
fn verb_completion_all_verbs_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let candidates = completions("", dir.path(), false);
    assert_eq!(
        candidates.len(),
        10,
        "all 10 verbs should be returned for empty prefix; got {candidates:?}"
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

/// A destination that names a directory should be completable one step at a
/// time, which needs a trailing separator on the candidate — without it the
/// next `Tab` has nothing to walk into.
#[test]
fn path_completion_marks_a_directory_with_a_separator() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    fs::write(dir.path().join("nested.txt"), b"").unwrap();

    let candidates = completions("mv nested", dir.path(), false);
    let want_dir = format!("nested{}", std::path::MAIN_SEPARATOR);
    assert!(
        candidates.contains(&want_dir),
        "the directory should carry a trailing separator: {candidates:?}"
    );
    assert!(
        candidates.contains(&"nested.txt".to_owned()),
        "the file should not: {candidates:?}"
    );
}

/// Regression: completion used to match the whole destination against the
/// names in `cwd`, so the first separator typed killed it — no route into a
/// subdirectory could ever be completed.
#[test]
fn path_completion_walks_into_a_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    fs::write(dir.path().join("nested").join("inner.txt"), b"").unwrap();
    fs::write(dir.path().join("nested").join("other.txt"), b"").unwrap();

    let candidates = completions("mv nested/in", dir.path(), false);
    assert_eq!(
        candidates,
        vec!["nested/inner.txt".to_owned()],
        "the route must be completed against the directory it names, and the \
         candidate must carry the route back"
    );
}

/// The separator the user typed is the one the candidate comes back with, so a
/// route does not end up spelled two ways at once.
#[test]
#[cfg(windows)]
fn path_completion_accepts_a_backslash_route() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    fs::write(dir.path().join("nested").join("inner.txt"), b"").unwrap();

    let candidates = completions(r"mv nested\in", dir.path(), false);
    assert_eq!(candidates, vec![r"nested\inner.txt".to_owned()]);
}

/// An absolute destination — what you get by pasting a path in — completes
/// against the directory it names, not against `cwd`.
#[test]
fn path_completion_accepts_an_absolute_route() {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("target.txt"), b"").unwrap();
    let cwd = dir.path().join("cwd");
    fs::create_dir(&cwd).unwrap();

    let route = format!("{}{}", elsewhere.display(), std::path::MAIN_SEPARATOR);
    let candidates = completions(&format!("mv {route}"), &cwd, false);
    assert_eq!(candidates, vec![format!("{route}target.txt")]);
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
        state.error_text().is_none(),
        "no error expected; got {:?}",
        state.error_text()
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

    assert!(state.error_text().is_none());
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

    assert!(state.error_text().is_none());
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
        state.error_text().is_some(),
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
    // Permanent on purpose: the shipped default is the recycle bin, and a test
    // suite must not fill the machine's bin with its own fixtures.
    state.config.set_value("delete_mode", "permanent").unwrap();

    trail::actions::apply(trail::actions::Action::BeginDelete, &mut state).unwrap();
    trail::actions::apply(trail::actions::Action::ConfirmDelete, &mut state).unwrap();

    assert!(!state.pending_delete);
    assert!(
        !state
            .visible_entries()
            .any(|e| e.file_name == "to_delete.txt"),
        "to_delete.txt should be gone after ConfirmDelete"
    );
    let notice = state
        .notice
        .as_ref()
        .expect("a delete should report itself");
    assert_eq!(notice.level, trail::app::state::NoticeLevel::Info);
    assert!(
        notice.text.starts_with("deleted:"),
        "a permanent delete must not claim to be recoverable; got: {}",
        notice.text
    );
}

/// `dd` goes to the recycle bin unless the config says otherwise, and the
/// confirmation prompt says which of the two is about to happen — one can be
/// undone from the desktop and the other cannot.
#[test]
fn the_shipped_default_sends_a_delete_to_the_recycle_bin() {
    let config = trail::config::load(None).unwrap();
    assert_eq!(config.general.delete_mode, "trash");
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

/// Regression: `ya` yanked whatever `canonicalize` produced, which on Windows
/// is `\\?\C:\…`. A yank exists to be pasted into a shell or another program,
/// and the verbatim prefix is not something either wants to be handed.
#[test]
fn copy_abs_path_yanks_a_path_without_a_verbatim_prefix() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.txt"), b"").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::CopyAbsPath, &mut state).unwrap();
    let yank = state.last_yank.as_deref().expect("last_yank should be set");
    assert!(
        !yank.contains(r"\\?\"),
        "yank leaked an extended-length prefix: {yank}"
    );
    // Still a usable absolute path, not just a prefix-free string.
    assert!(
        std::path::Path::new(yank).exists(),
        "the yanked path must still resolve: {yank}"
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
fn copy_rel_path_is_relative_to_the_launch_dir() {
    let dir = tempfile::tempdir().unwrap();
    let launched_from = dir.path().join("launched");
    let browsing = dir.path().join("browsing");
    fs::create_dir(&launched_from).unwrap();
    fs::create_dir(&browsing).unwrap();
    fs::write(browsing.join("rel.txt"), b"").unwrap();

    let mut state = trail::app::state::AppState::new(browsing).unwrap();
    state.launch_dir = trail::pathfmt::canonicalize(&launched_from).unwrap();

    trail::actions::apply(trail::actions::Action::CopyRelPath, &mut state).unwrap();
    let yank = state.last_yank.as_deref().expect("last_yank should be set");
    // Relative to the launch directory, climbing out of it as needed — this is
    // what makes the yank pasteable in the shell Trail was started from.
    assert_eq!(yank.replace('\\', "/"), "../browsing/rel.txt");
}

#[test]
fn copy_rel_path_and_copy_filename_yank_different_strings() {
    // Regression: `yr` used to resolve against `cwd`, where every entry's
    // relative path is its own file name — making it a duplicate of `yn`.
    let dir = tempfile::tempdir().unwrap();
    let browsing = dir.path().join("browsing");
    fs::create_dir(&browsing).unwrap();
    fs::write(browsing.join("rel.txt"), b"").unwrap();

    let mut state = trail::app::state::AppState::new(browsing).unwrap();
    state.launch_dir = trail::pathfmt::canonicalize(dir.path()).unwrap();

    trail::actions::apply(trail::actions::Action::CopyRelPath, &mut state).unwrap();
    let relative = state.last_yank.clone().expect("last_yank should be set");

    trail::actions::apply(trail::actions::Action::CopyFilename, &mut state).unwrap();
    let filename = state.last_yank.clone().expect("last_yank should be set");

    assert_eq!(relative.replace('\\', "/"), "browsing/rel.txt");
    assert_eq!(filename, "rel.txt");
    assert_ne!(relative, filename);
}

#[test]
fn copy_content_yanks_file_text() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.txt"), b"first\nsecond\n").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::CopyContent, &mut state).unwrap();
    assert_eq!(state.last_yank.as_deref(), Some("first\nsecond\n"));
}

#[test]
fn copy_content_yanks_directory_listing() {
    let dir = tempfile::tempdir().unwrap();
    // Directories sort first, so `sub` is the selection at index 0.
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("inner.txt"), b"").unwrap();
    fs::create_dir(dir.path().join("sub").join("deeper")).unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::CopyContent, &mut state).unwrap();
    assert_eq!(state.last_yank.as_deref(), Some("deeper/\ninner.txt"));
}

#[test]
fn copy_content_of_a_binary_file_reports_an_error() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("blob.bin"), [0u8, 1, 0, 2]).unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::CopyContent, &mut state).unwrap();
    assert!(
        state.last_yank.is_none(),
        "nothing should be yanked for a binary file"
    );
    let err = state
        .error_text()
        .expect("a binary yank should surface an error");
    assert!(
        err.contains("binary"),
        "error should name the cause; got: {err}"
    );
}

// ── Notices ───────────────────────────────────────────────────────────────────

/// The bug this guards: the success branch of `:bookmark` set the error field,
/// so a bookmark that saved correctly was announced as
/// `Error: bookmark added: name`.
#[test]
fn a_saved_bookmark_is_reported_as_an_outcome_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(
        trail::actions::Action::ExecuteCommand(ParsedCommand::Bookmark("work".to_owned())),
        &mut state,
    )
    .unwrap();

    assert!(
        state.error_text().is_none(),
        "saving a bookmark is not a failure"
    );
    let notice = state.notice.as_ref().expect("the outcome should be shown");
    assert_eq!(notice.level, trail::app::state::NoticeLevel::Info);
    assert!(
        notice.text.contains("bookmark added: work"),
        "the notice should name the bookmark; got: {}",
        notice.text
    );
}

/// A yank reports itself through the notice channel, summarized: `yc` puts
/// whole files on the clipboard and the status bar holds one line.
#[test]
fn a_yank_posts_a_summarized_notice() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("long.txt"), "first line\nsecond line\n").unwrap();
    let mut state = trail::app::state::AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::CopyContent, &mut state).unwrap();

    // The clipboard itself may be unreachable (headless CI), which is reported
    // as an error rather than an outcome — but either way the notice must be
    // one line and must not carry the whole file.
    let notice = state.notice.as_ref().expect("a yank should report itself");
    assert!(
        !notice.text.contains('\n'),
        "a notice is one line; got: {:?}",
        notice.text
    );
    if notice.level == trail::app::state::NoticeLevel::Info {
        assert!(
            notice.text.starts_with("yanked: first line"),
            "the notice should summarize the yank; got: {}",
            notice.text
        );
        assert!(
            notice.text.ends_with('…'),
            "the elision should be marked; got: {}",
            notice.text
        );
    }
}
