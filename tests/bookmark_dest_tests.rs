//! Bookmarks as `:mv`/`:cp` destinations: `@name` (§2.22 of
//! `upcoming_features.md`).

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use trail::actions::{apply, Action};
use trail::app::state::AppState;
use trail::input::command_parser::{completions_with_sources, CompletionSources, ParsedCommand};
use trail::plugin::bookmarks::BookmarkStore;

/// ```text
/// <tmp>/
///   here/
///     a.txt
///     @folder/        a real folder whose name starts with `@`
///   far/
///     sub/
///   bookmarks.json    `far` is bookmarked as `docs`
/// ```
fn setup() -> (TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let here = dir.path().join("here");
    fs::create_dir_all(here.join("@folder")).unwrap();
    fs::create_dir_all(dir.path().join("far/sub")).unwrap();
    fs::write(here.join("a.txt"), b"a").unwrap();

    let mut store = BookmarkStore::open(dir.path().join("bookmarks.json")).unwrap();
    store
        .add("docs".to_owned(), dir.path().join("far"))
        .unwrap();

    let mut state = AppState::new(here).unwrap();
    state.bookmark_store = Some(store);
    // Select a.txt: the listing is `@folder/` then `a.txt`.
    while state.selected_entry().map(|e| e.file_name.as_str()) != Some("a.txt") {
        state.move_down();
    }
    (dir, state)
}

fn mv(state: &mut AppState, dest: &str) {
    apply(
        Action::ExecuteCommand(ParsedCommand::Mv(dest.to_owned())),
        state,
    )
    .unwrap();
}

#[test]
fn at_name_is_the_bookmarked_directory() {
    let (dir, mut state) = setup();
    mv(&mut state, "@docs");
    assert!(dir.path().join("far/a.txt").exists(), "{:?}", state.notice);
}

#[test]
fn a_route_after_the_name_goes_inside_it() {
    let (dir, mut state) = setup();
    mv(&mut state, "@docs/sub/");
    assert!(
        dir.path().join("far/sub/a.txt").exists(),
        "{:?}",
        state.notice
    );
}

#[test]
fn an_unknown_name_is_an_error_not_a_folder() {
    let (dir, mut state) = setup();
    mv(&mut state, "@nope");

    let err = state.error_text().expect("refused").to_owned();
    assert!(err.contains("no bookmark named 'nope'"), "{err}");
    assert!(dir.path().join("here/a.txt").exists(), "nothing moved");
}

#[test]
fn a_real_at_folder_is_reached_with_dot_slash() {
    let (dir, mut state) = setup();
    mv(&mut state, "./@folder");
    assert!(dir.path().join("here/@folder/a.txt").exists());
}

fn bookmarks(far: &Path) -> Vec<(String, PathBuf)> {
    vec![
        ("docs".to_owned(), far.to_owned()),
        ("downloads".to_owned(), far.to_owned()),
    ]
}

#[test]
fn tab_after_at_offers_bookmark_names() {
    let dir = tempfile::tempdir().unwrap();
    let marks = bookmarks(dir.path());
    let sources = CompletionSources {
        plugin_actions: &[],
        bookmarks: &marks,
    };
    let sep = std::path::MAIN_SEPARATOR;

    let got = completions_with_sources("mv @do", dir.path(), false, &sources);
    assert_eq!(got, vec![format!("@docs{sep}"), format!("@downloads{sep}")]);
    let got = completions_with_sources("mv @doc", dir.path(), false, &sources);
    assert_eq!(got, vec![format!("@docs{sep}")]);
}

#[test]
fn tab_after_a_bookmark_walks_inside_it() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("next")).unwrap();
    let marks = bookmarks(dir.path());
    let sources = CompletionSources {
        plugin_actions: &[],
        bookmarks: &marks,
    };

    let got = completions_with_sources("cp @docs/ne", Path::new("."), false, &sources);
    assert_eq!(got, vec!["@docs/next/".to_owned()]);
}
