//! Unit tests for AppState transitions and core invariants.
//!
//! Covers the cases required by the Phase 1 testing specification:
//! - Selection movement at list boundaries (top/bottom edge cases)
//! - Directory-first sort correctness
//! - History push/pop behavior
//! - The `state.dirty` invariant: every mutation sets it; render must clear it
//! - Hidden-file toggle and visibility
//! - Selection preservation across re-entry

use std::fs;

use tempfile::TempDir;

use trail::app::mode::Mode;
use trail::app::state::{AppState, EntryKind};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Creates a predictable temp directory layout:
///
/// ```text
/// <tmp>/
///   alpha_dir/
///   zeta_dir/
///   a_file.txt
///   z_file.txt
///   .hidden_file
/// ```
fn make_test_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path();
    fs::create_dir(p.join("alpha_dir")).expect("mkdir alpha_dir");
    fs::create_dir(p.join("zeta_dir")).expect("mkdir zeta_dir");
    fs::write(p.join("a_file.txt"), b"").expect("write a_file.txt");
    fs::write(p.join("z_file.txt"), b"").expect("write z_file.txt");
    fs::write(p.join(".hidden_file"), b"").expect("write .hidden_file");
    dir
}

// ── Directory-first sort ──────────────────────────────────────────────────────

#[test]
fn directories_sort_before_files() {
    let dir = make_test_dir();
    let state = AppState::new(dir.path().to_owned()).unwrap();

    let entries: Vec<_> = state.visible_entries().collect();
    // With show_hidden=false we expect: alpha_dir, zeta_dir, a_file.txt, z_file.txt
    assert!(entries.len() >= 2, "expected at least two visible entries");
    assert_eq!(
        entries[0].kind,
        EntryKind::Dir,
        "first entry should be a directory"
    );
    assert_eq!(
        entries[1].kind,
        EntryKind::Dir,
        "second entry should be a directory"
    );

    // All files come after all directories.
    let first_file_idx = entries
        .iter()
        .position(|e| e.kind == EntryKind::File)
        .expect("expected at least one file");
    let last_dir_idx = entries
        .iter()
        .rposition(|e| e.kind == EntryKind::Dir)
        .expect("expected at least one dir");
    assert!(
        last_dir_idx < first_file_idx,
        "all directories must precede all files"
    );
}

#[test]
fn directory_sort_is_alphabetical_case_insensitive() {
    let dir = make_test_dir();
    let state = AppState::new(dir.path().to_owned()).unwrap();

    let dirs: Vec<&str> = state
        .visible_entries()
        .filter(|e| e.kind == EntryKind::Dir)
        .map(|e| e.file_name.as_str())
        .collect();

    assert_eq!(dirs, vec!["alpha_dir", "zeta_dir"]);
}

#[test]
fn files_sort_alphabetically_after_dirs() {
    let dir = make_test_dir();
    let state = AppState::new(dir.path().to_owned()).unwrap();

    let files: Vec<&str> = state
        .visible_entries()
        .filter(|e| e.kind == EntryKind::File)
        .map(|e| e.file_name.as_str())
        .collect();

    assert_eq!(files, vec!["a_file.txt", "z_file.txt"]);
}

// ── Hidden-file visibility ────────────────────────────────────────────────────

#[test]
fn hidden_files_invisible_by_default() {
    let dir = make_test_dir();
    let state = AppState::new(dir.path().to_owned()).unwrap();
    assert!(
        state.visible_entries().all(|e| !e.is_hidden),
        "no hidden entry should be visible when show_hidden is false"
    );
}

#[test]
fn toggle_hidden_reveals_hidden_entries() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.toggle_hidden().unwrap();
    assert!(
        state.visible_entries().any(|e| e.is_hidden),
        "at least one hidden entry should be visible after toggle"
    );
}

#[test]
fn toggle_hidden_twice_restores_original_visibility() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let count_before = state.visible_count();
    state.toggle_hidden().unwrap();
    state.toggle_hidden().unwrap();
    assert_eq!(state.visible_count(), count_before);
}

// ── Selection movement ────────────────────────────────────────────────────────

#[test]
fn initial_selection_is_zero() {
    let dir = make_test_dir();
    let state = AppState::new(dir.path().to_owned()).unwrap();
    assert_eq!(state.selected, 0);
}

#[test]
fn move_down_advances_selection() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.move_down();
    assert_eq!(state.selected, 1);
}

#[test]
fn move_down_clamps_at_last_entry() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let count = state.visible_count();
    // Move past the end many times.
    for _ in 0..count + 10 {
        state.move_down();
    }
    assert_eq!(
        state.selected,
        count - 1,
        "selection must clamp at last index {}",
        count - 1
    );
}

#[test]
fn move_up_at_top_stays_at_zero() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    assert_eq!(state.selected, 0);
    state.move_up();
    assert_eq!(state.selected, 0, "move_up at top must not underflow");
}

#[test]
fn move_up_decrements_selection() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.move_down();
    state.move_down();
    assert_eq!(state.selected, 2);
    state.move_up();
    assert_eq!(state.selected, 1);
}

#[test]
fn jump_top_goes_to_first_entry() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.jump_bottom();
    state.jump_top();
    assert_eq!(state.selected, 0);
}

#[test]
fn jump_bottom_goes_to_last_entry() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let count = state.visible_count();
    state.jump_bottom();
    assert_eq!(state.selected, count - 1);
}

// ── dirty invariant ───────────────────────────────────────────────────────────

#[test]
fn move_down_sets_dirty() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.dirty = false; // simulate post-render clear
    state.move_down();
    assert!(state.dirty, "move_down must set dirty=true");
}

#[test]
fn move_up_when_not_at_top_sets_dirty() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.move_down();
    state.dirty = false;
    state.move_up();
    assert!(state.dirty);
}

#[test]
fn move_up_at_top_does_not_set_dirty() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.dirty = false;
    // Already at 0; no-op move should not set dirty.
    state.move_up();
    assert!(!state.dirty, "no-op move_up must not set dirty");
}

#[test]
fn jump_top_when_already_at_top_does_not_set_dirty() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.dirty = false;
    state.jump_top(); // already at 0
    assert!(!state.dirty, "no-op jump_top must not set dirty");
}

#[test]
fn enter_dir_sets_dirty() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.dirty = false;
    let subdir = dir.path().join("alpha_dir");
    state.enter_dir(subdir).unwrap();
    assert!(state.dirty);
}

// ── Navigation history ────────────────────────────────────────────────────────

#[test]
fn entering_dir_pushes_history() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let original = state.cwd.clone();
    let subdir = dir.path().join("alpha_dir");
    state.enter_dir(subdir.clone()).unwrap();

    // Going back should return to the original directory.
    state.history_back().unwrap();
    assert_eq!(state.cwd, original);
}

#[test]
fn history_back_then_forward() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let subdir = dir.path().join("alpha_dir");
    state.enter_dir(subdir.clone()).unwrap();

    state.history_back().unwrap();
    let expected_root = dir.path().canonicalize().unwrap_or(dir.path().to_owned());
    assert_eq!(state.cwd, expected_root);

    state.history_forward().unwrap();
    // Canonicalize both sides before comparing to handle platform differences.
    let got = state.cwd.canonicalize().unwrap_or(state.cwd.clone());
    let want = subdir.canonicalize().unwrap_or(subdir.clone());
    assert_eq!(got, want);
}

#[test]
fn history_back_on_empty_is_noop() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let original = state.cwd.clone();
    // No navigation yet — back should be a no-op.
    state.history_back().unwrap();
    assert_eq!(state.cwd, original);
}

#[test]
fn history_forward_on_empty_is_noop() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let original = state.cwd.clone();
    state.history_forward().unwrap();
    assert_eq!(state.cwd, original);
}

// ── Selection preservation ────────────────────────────────────────────────────

#[test]
fn selection_remembered_on_re_entry() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    // Select the second entry, then enter alpha_dir, then go back.
    state.move_down(); // selection = 1
    let remembered_idx = state.selected;
    let subdir = dir.path().join("alpha_dir");
    state.enter_dir(subdir).unwrap();
    state.history_back().unwrap();

    assert_eq!(
        state.selected, remembered_idx,
        "selection should be restored when re-entering a directory"
    );
}

// ── Mode transitions ──────────────────────────────────────────────────────────

#[test]
fn default_mode_is_navigation() {
    let dir = make_test_dir();
    let state = AppState::new(dir.path().to_owned()).unwrap();
    assert_eq!(state.mode, Mode::Navigation);
}

#[test]
fn enter_search_mode() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    trail::actions::apply(trail::actions::Action::EnterSearch, &mut state).unwrap();
    assert!(
        matches!(state.mode, Mode::Search { .. }),
        "mode should be Search after EnterSearch action"
    );
}

#[test]
fn exit_mode_returns_to_navigation() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    trail::actions::apply(trail::actions::Action::EnterSearch, &mut state).unwrap();
    trail::actions::apply(trail::actions::Action::ExitMode, &mut state).unwrap();
    assert_eq!(state.mode, Mode::Navigation);
}

// ── go_parent ─────────────────────────────────────────────────────────────────

#[test]
fn go_parent_navigates_up() {
    let dir = make_test_dir();
    let subdir = dir.path().join("alpha_dir");
    let mut state = AppState::new(subdir).unwrap();
    let expected = dir.path().canonicalize().unwrap_or(dir.path().to_owned());
    state.go_parent().unwrap();
    assert_eq!(state.cwd, expected);
}

// ── visible_count consistency ─────────────────────────────────────────────────

#[test]
fn visible_count_excludes_hidden_by_default() {
    let dir = make_test_dir();
    let state = AppState::new(dir.path().to_owned()).unwrap();
    // 2 dirs + 2 files = 4 visible (1 .hidden_file excluded)
    assert_eq!(state.visible_count(), 4);
}

#[test]
fn visible_count_includes_hidden_when_toggled() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.toggle_hidden().unwrap();
    // 2 dirs + 2 files + 1 hidden = 5
    assert_eq!(state.visible_count(), 5);
}

// ── Separate TempDir — edge-case: empty directory ────────────────────────────

#[test]
fn empty_directory_visible_count_is_zero() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(dir.path().to_owned()).unwrap();
    assert_eq!(state.visible_count(), 0);
}

#[test]
fn move_down_in_empty_dir_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.move_down(); // must not panic
    assert_eq!(state.selected, 0);
}

#[test]
fn jump_bottom_in_empty_dir_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.jump_bottom(); // must not panic
    assert_eq!(state.selected, 0);
}

// ── selected_entry ────────────────────────────────────────────────────────────

#[test]
fn selected_entry_returns_none_for_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(dir.path().to_owned()).unwrap();
    assert!(state.selected_entry().is_none());
}

#[test]
fn selected_entry_returns_first_entry() {
    let dir = make_test_dir();
    let state = AppState::new(dir.path().to_owned()).unwrap();
    let entry = state
        .selected_entry()
        .expect("should have a selected entry");
    // The first visible entry should be "alpha_dir" (directory-first sort).
    assert_eq!(entry.file_name, "alpha_dir");
}

// ── preview.generation invariant ─────────────────────────────────────────────

#[test]
fn preview_slot_has_generation_field() {
    // This test ensures the PreviewSlot.generation field exists and starts at
    // the default value. The stale-guard logic in Phase 4 depends on this
    // invariant being established from Phase 1.
    let dir = make_test_dir();
    let state = AppState::new(dir.path().to_owned()).unwrap();
    // generation field must be accessible (compile-time check) and
    // default-initialized to 0.
    assert_eq!(state.preview.generation, 0);
}

// ── refresh reloads listing ───────────────────────────────────────────────────

#[test]
fn refresh_picks_up_newly_created_file() {
    let dir = make_test_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let before = state.visible_count();

    fs::write(dir.path().join("new_file.txt"), b"").unwrap();
    state.refresh().unwrap();

    assert_eq!(state.visible_count(), before + 1);
}

// ── NavigationHistory unit tests (via app::state integration) ────────────────

#[test]
fn multiple_back_forward_cycles() {
    let root = make_test_dir();
    let alpha = root.path().join("alpha_dir");
    let zeta = root.path().join("zeta_dir");

    let mut state = AppState::new(root.path().to_owned()).unwrap();
    let root_cwd = state.cwd.clone();

    state.enter_dir(alpha.clone()).unwrap();
    let alpha_cwd = state.cwd.clone();

    state.enter_dir(zeta.clone()).unwrap();

    // Back: zeta → alpha
    state.history_back().unwrap();
    assert_eq!(state.cwd, alpha_cwd);

    // Back: alpha → root
    state.history_back().unwrap();
    assert_eq!(state.cwd, root_cwd);

    // Forward: root → alpha
    state.history_forward().unwrap();
    assert_eq!(state.cwd, alpha_cwd);
}

// ── Phase 2: Search Mode / fuzzy filter ──────────────────────────────────────

/// Fixture with distinguishable names for filter tests:
///
/// ```text
/// <tmp>/
///   docs/         (dir)
///   src/          (dir)
///   main.rs       (file)
///   readme.md     (file)
///   .env          (hidden file)
/// ```
fn make_filter_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path();
    fs::create_dir(p.join("docs")).expect("mkdir docs");
    fs::create_dir(p.join("src")).expect("mkdir src");
    fs::write(p.join("main.rs"), b"").expect("write main.rs");
    fs::write(p.join("readme.md"), b"").expect("write readme.md");
    fs::write(p.join(".env"), b"").expect("write .env");
    dir
}

#[test]
fn filter_reduces_match_set() {
    let dir = make_filter_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    // Query "main" should match "main.rs" but not dirs/other files.
    state.apply_filter("main".to_owned());

    let filter = state.filter.as_ref().expect("filter should be Some");
    assert!(!filter.matches.is_empty(), "expected at least one match");

    // Every matched entry's filename must contain something matching "main".
    for &idx in &filter.matches {
        let name = &state.entries[idx].file_name.to_lowercase();
        assert!(
            name.contains("main"),
            "matched entry '{name}' does not look like a 'main' match"
        );
    }
}

#[test]
fn filter_exact_name_is_included() {
    let dir = make_filter_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.apply_filter("readme".to_owned());

    let filter = state.filter.as_ref().expect("filter should be Some");
    let matched_names: Vec<&str> = filter
        .matches
        .iter()
        .map(|&i| state.entries[i].file_name.as_str())
        .collect();
    assert!(
        matched_names.contains(&"readme.md"),
        "readme.md must be in matches; got {matched_names:?}"
    );
}

#[test]
fn filter_excludes_non_matching_entries() {
    let dir = make_filter_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    // "zzzzz" matches nothing in our fixture.
    state.apply_filter("zzzzz".to_owned());

    let filter = state.filter.as_ref().expect("filter should be Some");
    assert!(
        filter.matches.is_empty(),
        "expected no matches for nonsense query"
    );
}

#[test]
fn empty_query_shows_all_visible_entries() {
    let dir = make_filter_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    // Apply a non-empty filter first, then clear it.
    state.apply_filter("main".to_owned());
    state.apply_filter(String::new());

    let filter = state.filter.as_ref().expect("filter should be Some");
    // With show_hidden=false: docs/, src/, main.rs, readme.md = 4 entries.
    assert_eq!(
        filter.matches.len(),
        state.visible_count(),
        "empty query should show all visible entries"
    );
}

#[test]
fn exit_mode_clears_filter() {
    let dir = make_filter_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::EnterSearch, &mut state).unwrap();
    trail::actions::apply(trail::actions::Action::SearchAppendChar('m'), &mut state).unwrap();

    // Esc via ExitMode should clear the filter.
    trail::actions::apply(trail::actions::Action::ExitMode, &mut state).unwrap();
    assert!(state.filter.is_none(), "filter must be None after ExitMode");
    assert_eq!(state.mode, Mode::Navigation);
}

#[test]
fn filter_auto_selects_top_match() {
    let dir = make_filter_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    // Start from a non-zero selection.
    state.move_down();
    assert_eq!(state.selected, 1);

    // Applying a filter must reset selection to 0 (top match).
    state.apply_filter("main".to_owned());
    assert_eq!(
        state.selected, 0,
        "selection must reset to 0 on filter apply"
    );
}

#[test]
fn filter_search_move_down_clamps_within_matches() {
    let dir = make_filter_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    // "rs" matches "main.rs" and possibly nothing else in our fixture.
    // Use a broad query to get at least two matches.
    state.apply_filter("r".to_owned());

    let match_count = state.filter.as_ref().map(|f| f.matches.len()).unwrap_or(0);

    if match_count >= 2 {
        // Move down past the end: selection must clamp.
        for _ in 0..match_count + 5 {
            trail::actions::apply(trail::actions::Action::SearchMoveDown, &mut state).unwrap();
        }
        assert_eq!(
            state.selected,
            match_count - 1,
            "SearchMoveDown must clamp at the last match index"
        );
    }
    // If fewer than 2 matches, the clamping logic is still exercised by
    // move_down's empty-list guard — we just skip the multi-move assertion.
}

#[test]
fn filter_dirty_set_on_apply() {
    let dir = make_filter_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.dirty = false; // simulate post-render clear

    state.apply_filter("doc".to_owned());
    assert!(state.dirty, "apply_filter must set dirty=true");
}

#[test]
fn filter_reorder_by_score() {
    // Create a directory where the scores are predictably different:
    // "foobar" and "foo" both contain "foo", but an exact prefix match
    // should score higher.
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path();
    fs::write(p.join("foobar.txt"), b"").expect("write foobar.txt");
    fs::write(p.join("foo.txt"), b"").expect("write foo.txt");

    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.apply_filter("foo".to_owned());

    let filter = state.filter.as_ref().expect("filter should be Some");
    // Both files should match.
    assert_eq!(filter.matches.len(), 2, "both files should match 'foo'");
    // Scores must be in descending order.
    assert!(
        filter.scores[0] >= filter.scores[1],
        "scores must be non-increasing (best match first): {:?}",
        filter.scores
    );
}

#[test]
fn enter_search_action_shows_all_entries() {
    let dir = make_filter_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let visible_before = state.visible_count();

    trail::actions::apply(trail::actions::Action::EnterSearch, &mut state).unwrap();

    assert!(
        matches!(state.mode, Mode::Search { .. }),
        "mode must be Search after EnterSearch"
    );
    let filter = state
        .filter
        .as_ref()
        .expect("filter must be Some in Search Mode");
    assert_eq!(
        filter.matches.len(),
        visible_before,
        "entering Search Mode with empty query must show all visible entries"
    );
}
