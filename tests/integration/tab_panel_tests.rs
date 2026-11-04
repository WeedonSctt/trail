//! The tab panel, driven the way a user drives it: keys through the dispatcher,
//! a rendered frame out.
//!
//! What these cannot show is how it looks in a real terminal, and how an image
//! preview behaves as it opens and closes where the terminal draws real images.

use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;
use ratatui::Terminal;
use tempfile::TempDir;

use trail::actions::{apply, Action};
use trail::app::mode::Mode;
use trail::app::state::AppState;
use trail::input::{self, InputCtx};

const WIDTH: u16 = 120;
const HEIGHT: u16 = 30;

fn key_with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn ch(c: char) -> KeyEvent {
    key_with(KeyCode::Char(c), KeyModifiers::NONE)
}

fn key(code: KeyCode) -> KeyEvent {
    key_with(code, KeyModifiers::NONE)
}

/// Sends `key` through the dispatcher as the event loop does, including its
/// clearing of a prefix once the next key has been handled.
fn press(state: &mut AppState, key: KeyEvent) {
    let mut ctx = InputCtx::default();
    if let Some(action) = input::dispatch(key, state, &mut ctx) {
        let is_prefix = matches!(action, Action::SetPendingNavKey(_));
        apply(action, state).unwrap();
        if !is_prefix {
            state.pending_nav_key = None;
        }
    } else {
        state.pending_nav_key = None;
    }
}

/// A scratch directory holding `count` folders, `d00` onward, with a file in each
/// so they list something.
fn folders(count: usize) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..count {
        let folder = dir.path().join(format!("d{n:02}"));
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("file.txt"), b"hello\n").unwrap();
    }
    dir
}

fn folder(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("d{n:02}"))
}

/// A state with one tab on each of `d00` to `d(count-1)`, the first one active.
fn tabbed(dir: &Path, count: usize) -> AppState {
    let mut state = AppState::new(folder(dir, 0)).unwrap();
    for n in 1..count {
        state.open_tab(Some(folder(dir, n))).unwrap();
    }
    state.switch_tab_to(0).unwrap();
    state
}

fn selected(state: &AppState) -> usize {
    match state.mode {
        Mode::Tabs { selected, .. } => selected,
        ref other => panic!("the panel is not open: {other:?}"),
    }
}

fn is_open(state: &AppState) -> bool {
    matches!(state.mode, Mode::Tabs { .. })
}

fn draw(state: &mut AppState, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    trail::ui::render(&mut terminal, state).unwrap();
    terminal.backend().buffer().clone()
}

/// The frame as one string per row.
fn rows_of(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf.cell((x, y)).map_or(" ", |c| c.symbol()))
                .collect()
        })
        .collect()
}

fn frame(state: &mut AppState) -> Vec<String> {
    rows_of(&draw(state, WIDTH, HEIGHT))
}

/// The row the tab panel's top border is on, if it is drawn.
fn panel_top(rows: &[String]) -> Option<usize> {
    rows.iter().position(|row| row.contains("╭ Tabs "))
}

/// The rows of the panel, border included, without the status bar under it.
fn panel(rows: &[String]) -> &[String] {
    let top = panel_top(rows).expect("the panel is drawn");
    &rows[top..rows.len() - 1]
}

/// The row listing `file.txt`, which the preview's title also names.
fn listing_row(rows: &[String]) -> &String {
    rows.iter()
        .find(|row| row.starts_with('│') && row.contains("file.txt"))
        .expect("the listing is drawn")
}

fn column_of(row: &str, text: &str) -> Option<usize> {
    row.find(text).map(|at| row[..at].chars().count())
}

// ── Keys ──────────────────────────────────────────────────────────────────────

#[test]
fn t_opens_on_the_current_tab() {
    let dir = folders(3);
    let mut state = tabbed(dir.path(), 3);
    state.switch_tab_to(1).unwrap();

    press(&mut state, ch('t'));

    assert_eq!(selected(&state), 1);
}

#[test]
fn it_opens_with_one_tab_too() {
    let dir = folders(1);
    let mut state = tabbed(dir.path(), 1);
    press(&mut state, ch('t'));
    assert_eq!(selected(&state), 0);
}

#[test]
fn j_and_k_stop_at_either_end() {
    let dir = folders(3);
    let mut state = tabbed(dir.path(), 3);
    press(&mut state, ch('t'));

    press(&mut state, ch('k'));
    assert_eq!(selected(&state), 0, "no wrapping off the top");

    for expected in [1, 2, 2] {
        press(&mut state, ch('j'));
        assert_eq!(selected(&state), expected);
    }

    press(&mut state, key(KeyCode::Up));
    assert_eq!(selected(&state), 1, "the arrows move it too");
    press(&mut state, key(KeyCode::Down));
    assert_eq!(selected(&state), 2);
}

#[test]
fn gg_and_capital_g_go_to_the_ends() {
    let dir = folders(4);
    let mut state = tabbed(dir.path(), 4);
    press(&mut state, ch('t'));

    press(&mut state, ch('G'));
    assert_eq!(selected(&state), 3);
    press(&mut state, ch('g'));
    press(&mut state, ch('g'));
    assert_eq!(selected(&state), 0);
    assert_eq!(state.pending_nav_key, None);
}

#[test]
fn moving_the_selector_does_not_switch_tabs() {
    let dir = folders(3);
    let mut state = tabbed(dir.path(), 3);
    press(&mut state, ch('t'));

    press(&mut state, ch('j'));
    press(&mut state, ch('j'));

    assert_eq!(state.cwd, folder(dir.path(), 0));
    assert_eq!(state.tab_manager.active, 0);
}

#[test]
fn enter_and_l_switch_to_the_tab_and_close_the_panel() {
    let dir = folders(3);
    for enter in [key(KeyCode::Enter), ch('l')] {
        let mut state = tabbed(dir.path(), 3);
        press(&mut state, ch('t'));
        press(&mut state, ch('j'));
        press(&mut state, ch('j'));

        press(&mut state, enter);

        assert_eq!(state.mode, Mode::Navigation);
        assert_eq!(state.tab_manager.active, 2);
        assert_eq!(state.cwd, folder(dir.path(), 2));
    }
}

#[test]
fn enter_on_the_current_tab_just_closes_the_panel() {
    let dir = folders(2);
    let mut state = tabbed(dir.path(), 2);
    press(&mut state, ch('t'));
    press(&mut state, key(KeyCode::Enter));

    assert_eq!(state.mode, Mode::Navigation);
    assert_eq!(state.tab_manager.active, 0);
}

/// `Esc` clears the marks in Navigation Mode, and must not here.
#[test]
fn esc_and_t_close_without_switching_and_keep_the_marks() {
    let dir = folders(2);
    for close in [key(KeyCode::Esc), ch('t')] {
        let mut state = tabbed(dir.path(), 2);
        apply(Action::ToggleMark, &mut state).unwrap();
        assert_eq!(state.marks.len(), 1);
        press(&mut state, ch('t'));
        press(&mut state, ch('j'));

        press(&mut state, close);

        assert_eq!(state.mode, Mode::Navigation);
        assert_eq!(state.tab_manager.active, 0);
        assert_eq!(state.marks.len(), 1);
    }
}

#[test]
fn every_other_key_does_nothing() {
    let dir = folders(2);
    let mut state = tabbed(dir.path(), 2);
    press(&mut state, ch('t'));
    let before = state.mode.clone();

    let ignored = [
        ch('q'),
        ch('x'),
        ch('h'),
        ch(':'),
        ch('/'),
        ch('@'),
        ch(' '),
        ch('v'),
        ch('d'),
        key(KeyCode::Tab),
        key(KeyCode::Left),
        key(KeyCode::Backspace),
        key_with(KeyCode::Char('t'), KeyModifiers::CONTROL),
        key_with(KeyCode::Char('w'), KeyModifiers::CONTROL),
        key_with(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ];
    let mut ctx = InputCtx::default();
    for key in ignored {
        assert_eq!(input::dispatch(key, &state, &mut ctx), None, "{key:?}");
    }

    press(
        &mut state,
        key_with(KeyCode::Char('t'), KeyModifiers::CONTROL),
    );
    assert_eq!(state.mode, before);
    assert_eq!(state.tab_manager.len(), 2);
}

#[test]
fn it_opens_only_from_navigation_mode() {
    let dir = folders(1);
    let mut state = tabbed(dir.path(), 1);
    press(&mut state, ch('v'));
    assert!(matches!(state.mode, Mode::Visual { .. }));

    press(&mut state, ch('t'));

    assert!(matches!(state.mode, Mode::Visual { .. }));
}

#[test]
fn movement_follows_the_users_bindings() {
    let dir = folders(2);
    let mut state = tabbed(dir.path(), 2);
    state
        .config
        .keymap
        .navigation
        .insert("move_down".to_owned(), "n".to_owned());
    press(&mut state, ch('t'));

    press(&mut state, ch('n'));

    assert_eq!(selected(&state), 1);
}

// ── A config that already uses `t` ────────────────────────────────────────────

fn config_with(navigation: &str) -> trail::config::TrailConfig {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("trail.toml");
    fs::write(&file, format!("[keymap.navigation]\n{navigation}\n")).unwrap();
    trail::config::load(Some(&file)).unwrap()
}

#[test]
fn a_config_that_binds_t_keeps_its_binding() {
    for binding in ["refresh = \"t\"", "refresh = \"tr\""] {
        let config = config_with(binding);
        assert_eq!(
            config.keymap.navigation.get("toggle_tab_panel"),
            None,
            "{binding}"
        );
    }
}

#[test]
fn a_config_that_names_the_panel_key_gets_it() {
    let config = config_with("refresh = \"t\"\ntoggle_tab_panel = \"T\"");
    assert_eq!(
        config
            .keymap
            .navigation
            .get("toggle_tab_panel")
            .map(String::as_str),
        Some("T")
    );
}

/// `tab` is one key, not a sequence starting with the letter.
#[test]
fn a_named_key_starting_with_t_does_not_count() {
    let config = config_with("switch_tab_next = \"tab\"");
    assert_eq!(
        config
            .keymap
            .navigation
            .get("toggle_tab_panel")
            .map(String::as_str),
        Some("t")
    );
}

// ── Drawing ───────────────────────────────────────────────────────────────────

#[test]
fn the_panel_is_absent_until_opened_and_gone_after() {
    let dir = folders(2);
    let mut state = tabbed(dir.path(), 2);

    let closed = frame(&mut state);
    assert_eq!(panel_top(&closed), None);
    assert!(closed[HEIGHT as usize - 1].contains("NORMAL"));

    press(&mut state, ch('t'));
    let open = frame(&mut state);
    assert!(panel_top(&open).is_some());
    assert!(
        open[HEIGHT as usize - 1].contains("TABS"),
        "{}",
        open[HEIGHT as usize - 1]
    );

    press(&mut state, ch('t'));
    assert_eq!(panel_top(&frame(&mut state)), None);
}

#[test]
fn its_height_follows_the_tab_count_directly_above_the_status_bar() {
    let dir = folders(5);
    for tabs in [1_usize, 3, 5] {
        let mut state = tabbed(dir.path(), tabs);
        press(&mut state, ch('t'));

        let rows = frame(&mut state);

        let status = HEIGHT as usize - 1;
        assert_eq!(panel_top(&rows), Some(status - (tabs + 2)), "{tabs} tabs");
        assert!(rows[status - 1].contains('╰'), "bottom border");
        // The listing is still there, in what is left.
        assert!(rows.iter().any(|row| row.contains("file.txt")));
    }
}

#[test]
fn the_labels_are_the_directories_and_the_selector_starts_on_the_current_tab() {
    let dir = folders(3);
    let mut state = tabbed(dir.path(), 3);
    state.switch_tab_to(1).unwrap();
    press(&mut state, ch('t'));

    let rows = frame(&mut state);
    let panel = panel(&rows);

    for n in 0..3 {
        let name = format!("d{n:02}");
        let row = panel.iter().find(|row| row.contains(&name)).unwrap();
        assert_eq!(
            row.contains("> "),
            n == 1,
            "the selector marks only the current tab: {row}"
        );
    }
}

#[test]
fn the_active_label_follows_navigation_done_before_opening() {
    let dir = folders(2);
    let inner = folder(dir.path(), 0).join("inner");
    fs::create_dir(&inner).unwrap();
    let mut state = tabbed(dir.path(), 2);
    state.enter_dir(inner).unwrap();

    press(&mut state, ch('t'));
    let rows = frame(&mut state);

    assert!(
        panel(&rows).iter().any(|row| row.contains("inner")),
        "{:#?}",
        panel(&rows)
    );
}

#[test]
fn a_long_path_loses_its_start_and_keeps_the_folders_name() {
    let dir = folders(2);
    let mut state = tabbed(dir.path(), 2);
    press(&mut state, ch('t'));

    let rows = rows_of(&draw(&mut state, 14, HEIGHT));

    let panel = panel(&rows);
    assert!(
        panel
            .iter()
            .any(|row| row.contains("…") && row.contains("d01")),
        "{panel:#?}"
    );
}

#[test]
fn the_tab_you_are_in_stays_bold_once_the_selector_moves_off_it() {
    let dir = folders(2);
    let mut state = tabbed(dir.path(), 2);
    press(&mut state, ch('t'));
    press(&mut state, ch('j'));

    let buf = draw(&mut state, WIDTH, HEIGHT);
    let rows = rows_of(&buf);
    let is_bold = |name: &str| {
        let y = rows
            .iter()
            .rposition(|row| row.contains(name))
            .expect("label drawn");
        let x = column_of(&rows[y], name).unwrap();
        buf.cell((x as u16, y as u16))
            .unwrap()
            .modifier
            .contains(Modifier::BOLD)
    };

    assert!(is_bold("d00"));
    assert!(!is_bold("d01"));
}

/// The selection's row is unmarked and the names stay where they were, so only
/// one selector is on screen and nothing shifts sideways.
#[test]
fn the_navigation_highlight_is_off_and_its_names_do_not_move() {
    let dir = folders(2);
    let mut state = tabbed(dir.path(), 2);

    let closed = frame(&mut state);
    let row = listing_row(&closed);
    assert!(row.contains("> "), "the highlight before opening: {row}");
    let column = column_of(row, "file.txt");

    press(&mut state, ch('t'));
    let open = frame(&mut state);
    let row = listing_row(&open);

    assert!(!row.contains("> "), "the highlight while open: {row}");
    assert_eq!(column_of(row, "file.txt"), column);
    let selectors = open.iter().filter(|row| row.contains("> ")).count();
    assert_eq!(selectors, 1, "{open:#?}");
}

#[test]
fn at_the_top_it_scrolls_with_the_selector_in_view_and_hides_the_listing() {
    let dir = folders(40);
    let mut state = tabbed(dir.path(), 40);
    press(&mut state, ch('t'));

    press(&mut state, ch('G'));
    let rows = frame(&mut state);

    assert_eq!(panel_top(&rows), Some(0), "it stops growing at the top");
    assert!(!rows.iter().any(|row| row.contains("file.txt")));
    let selected_row = rows.iter().find(|row| row.contains("> ")).unwrap();
    assert!(selected_row.contains("d39"), "{selected_row}");

    press(&mut state, ch('g'));
    press(&mut state, ch('g'));
    let rows = frame(&mut state);
    let selected_row = rows.iter().find(|row| row.contains("> ")).unwrap();
    assert!(selected_row.contains("d00"), "{selected_row}");
}

/// The bug of §2.13: a position rebuilt each frame pins the selector to the last
/// row, so moving up would scroll the list instead of moving the selector.
#[test]
fn moving_up_moves_the_selector_rather_than_scrolling_under_it() {
    let dir = folders(40);
    let mut state = tabbed(dir.path(), 40);
    state.config.navigation.scroll_margin = 0;
    press(&mut state, ch('t'));
    press(&mut state, ch('G'));
    let row_of_selector = |rows: &[String]| rows.iter().position(|row| row.contains("> "));
    let bottom = row_of_selector(&frame(&mut state)).unwrap();

    press(&mut state, ch('k'));
    let rows = frame(&mut state);

    assert_eq!(row_of_selector(&rows), Some(bottom - 1));
    assert!(rows[bottom].contains("d39"), "the view held still");
}

#[test]
fn switching_from_the_panel_lands_with_the_panel_gone() {
    let dir = folders(2);
    let mut state = tabbed(dir.path(), 2);
    press(&mut state, ch('t'));
    press(&mut state, ch('j'));
    press(&mut state, key(KeyCode::Enter));

    let rows = frame(&mut state);

    assert_eq!(panel_top(&rows), None);
    assert!(rows[HEIGHT as usize - 1].contains("NORMAL"));
}

// ── Too little room ───────────────────────────────────────────────────────────

#[test]
fn too_few_rows_refuses_with_a_notice() {
    let dir = folders(1);
    let mut state = tabbed(dir.path(), 1);
    // Three rows less the status bar leaves two for the file area.
    draw(&mut state, WIDTH, 3);

    press(&mut state, ch('t'));

    assert!(!is_open(&state));
    let rows = rows_of(&draw(&mut state, WIDTH, 3));
    assert!(rows[2].contains("no room"), "{rows:#?}");
}

#[test]
fn just_enough_rows_opens() {
    let dir = folders(1);
    let mut state = tabbed(dir.path(), 1);
    draw(&mut state, WIDTH, 4);

    press(&mut state, ch('t'));

    assert!(is_open(&state));
    assert_eq!(panel_top(&rows_of(&draw(&mut state, WIDTH, 4))), Some(0));
}

#[test]
fn a_window_shrunk_too_far_closes_the_panel() {
    let dir = folders(2);
    let mut state = tabbed(dir.path(), 2);
    press(&mut state, ch('t'));
    assert!(is_open(&state));

    draw(&mut state, WIDTH, 2);

    assert_eq!(state.mode, Mode::Navigation);
}

// ── With the terminal panel ───────────────────────────────────────────────────

/// A state with the terminal panel open running a real shell, and the keyboard
/// back on the file list.
fn state_with_shell(dir: &Path) -> AppState {
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
    let mut state = AppState::with_config(folder(dir, 0), config).unwrap();
    state.open_tab(Some(folder(dir, 1))).unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    // The receiver is leaked so the session threads' sends succeed.
    std::mem::forget(rx);
    state.terminal.attach(tx);
    let terminal_config = state.config.terminal.clone();
    let cwd = state.cwd.clone();
    state.terminal.toggle(&terminal_config, &cwd).unwrap();
    state.terminal.switch_focus(&terminal_config, &cwd).unwrap();
    assert!(state.terminal.is_visible());
    assert!(!state.terminal.shell_focused());
    state
}

#[test]
fn an_open_terminal_panel_moves_up_and_keeps_its_size() {
    let dir = folders(2);
    let mut state = state_with_shell(dir.path());
    let status = HEIGHT as usize - 1;
    let shell_top = |rows: &[String]| rows.iter().position(|row| row.contains("1:sh")).unwrap();

    let closed = frame(&mut state);
    let rows_before = status - shell_top(&closed);

    press(&mut state, ch('t'));
    let open = frame(&mut state);

    let panel_top = panel_top(&open).expect("the panel is drawn");
    assert_eq!(panel_top, status - 4, "two tabs and the border");
    assert_eq!(panel_top - shell_top(&open), rows_before);
}

#[test]
fn a_maximized_terminal_panel_refuses_with_a_notice() {
    let dir = folders(2);
    let mut state = state_with_shell(dir.path());
    state.terminal.toggle_maximized();
    frame(&mut state);

    press(&mut state, ch('t'));

    assert!(!is_open(&state));
    assert!(frame(&mut state)[HEIGHT as usize - 1].contains("no room"));
}
