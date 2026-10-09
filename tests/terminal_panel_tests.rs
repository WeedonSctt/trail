//! The terminal panel, driven the way a user drives it: keys in, a rendered
//! frame out, against a real shell.
//!
//! What these cannot show is how it *feels* in a real terminal — ConPTY's
//! resizing, Windows Terminal delivering `Ctrl+.` — which needs a manual pass.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use trail::app::state::AppState;
use trail::terminal::{self, KeyOutcome};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn toggle() -> KeyEvent {
    key(KeyCode::Char('.'), KeyModifiers::CONTROL)
}

fn focus() -> KeyEvent {
    key(KeyCode::F(12), KeyModifiers::NONE)
}

/// A state in a scratch directory, with every panel shell running `cmd.exe`
/// (Windows) or `/bin/sh`, so the test does not depend on the machine's
/// PowerShell setup.
fn state_with_shell_profile(dir: &std::path::Path) -> AppState {
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
    let mut state = AppState::with_config(dir.to_owned(), config).unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    // The receiver is leaked so the session threads' sends succeed; these
    // tests read the screen directly rather than waiting on messages.
    std::mem::forget(rx);
    state.terminal.attach(tx);
    state
}

fn render(state: &mut AppState) -> String {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
    trail::ui::render(&mut terminal, state).unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            out.push_str(buf.cell((x, y)).map_or(" ", |c| c.symbol()));
        }
        out.push('\n');
    }
    out
}

fn type_line(state: &mut AppState, line: &str) {
    for ch in line.chars() {
        let outcome = terminal::handle_key(state, &key(KeyCode::Char(ch), KeyModifiers::NONE));
        assert_eq!(outcome, KeyOutcome::Handled);
    }
    terminal::handle_key(state, &key(KeyCode::Enter, KeyModifiers::NONE));
}

/// Renders until `needle` appears, or fails after a generous timeout — a cold
/// shell on a loaded CI machine can take seconds to print its first prompt.
fn render_until(state: &mut AppState, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let frame = render(state);
        if frame.contains(needle) {
            return frame;
        }
        assert!(Instant::now() < deadline, "never saw {needle:?}:\n{frame}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_toggle_key_opens_a_shell_that_runs_commands() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_shell_profile(dir.path());

    assert_eq!(
        terminal::handle_key(&mut state, &toggle()),
        KeyOutcome::Handled
    );
    assert!(state.terminal.is_visible());
    assert!(state.terminal.shell_focused());

    let frame = render(&mut state);
    assert!(frame.contains("1:sh"), "tab strip:\n{frame}");
    assert!(frame.contains("TERMINAL"), "status badge:\n{frame}");

    type_line(&mut state, "echo trail-panel-marker");
    // The command line echoes the text once; its output is the second copy.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let frame = render(&mut state);
        if frame.matches("trail-panel-marker").count() >= 2 {
            break;
        }
        assert!(Instant::now() < deadline, "no command output:\n{frame}");
        std::thread::sleep(Duration::from_millis(50));
    }

    // The toggle key hides it again, and the shell keeps running.
    terminal::handle_key(&mut state, &toggle());
    assert!(!state.terminal.is_visible());
    assert_eq!(state.terminal.sessions().len(), 1);
    assert!(!render(&mut state).contains("1:sh"));
}

#[test]
fn the_focus_key_hands_the_keyboard_back_to_the_file_list() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let mut state = state_with_shell_profile(dir.path());

    // From hidden, the focus key opens the panel rather than doing nothing.
    terminal::handle_key(&mut state, &focus());
    assert!(state.terminal.shell_focused());
    // While the shell has the keyboard, `j` is the shell's.
    let j = key(KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(terminal::handle_key(&mut state, &j), KeyOutcome::Handled);

    terminal::handle_key(&mut state, &focus());
    assert!(state.terminal.is_visible(), "the panel stays open");
    assert!(!state.terminal.shell_focused());
    assert_eq!(terminal::handle_key(&mut state, &j), KeyOutcome::NotMine);
    assert!(!render(&mut state).contains("TERMINAL"));
}

#[test]
fn several_shells_switch_and_close() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_shell_profile(dir.path());

    terminal::run_command(&mut state, terminal::TermCommand::New(None));
    terminal::run_command(&mut state, terminal::TermCommand::New(Some("sh".into())));
    assert_eq!(state.terminal.sessions().len(), 2);
    assert_eq!(state.terminal.active(), 1);
    let frame = render(&mut state);
    assert!(frame.contains("1:sh") && frame.contains("2:sh"), "{frame}");

    let next = key(KeyCode::PageDown, KeyModifiers::CONTROL);
    terminal::handle_key(&mut state, &next);
    assert_eq!(state.terminal.active(), 0, "wraps around");

    terminal::run_command(&mut state, terminal::TermCommand::Select(2));
    assert_eq!(state.terminal.active(), 1);

    terminal::run_command(&mut state, terminal::TermCommand::New(Some("nope".into())));
    assert!(render(&mut state).contains("no terminal profile 'nope'"));

    state.config.terminal.confirm_close = "never".into();
    terminal::run_command(&mut state, terminal::TermCommand::Close);
    terminal::run_command(&mut state, terminal::TermCommand::Close);
    assert!(state.terminal.sessions().is_empty());
    assert!(
        !state.terminal.is_visible(),
        "the last close hides the panel"
    );
}

#[test]
fn maximize_takes_the_whole_screen_and_restores() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("visible-file.txt"), "x").unwrap();
    let mut state = state_with_shell_profile(dir.path());
    terminal::handle_key(&mut state, &toggle());

    assert!(render(&mut state).contains("visible-file.txt"));
    terminal::run_command(&mut state, terminal::TermCommand::Max);
    assert!(!render(&mut state).contains("visible-file.txt"));
    terminal::run_command(&mut state, terminal::TermCommand::Max);
    assert!(render(&mut state).contains("visible-file.txt"));
}

#[test]
fn quitting_with_shells_open_asks_when_told_to() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_shell_profile(dir.path());

    // No shells: nothing to ask about, whatever the setting.
    state.config.terminal.confirm_quit = "always".into();
    assert!(terminal::allow_quit(&mut state, false));

    terminal::handle_key(&mut state, &toggle());
    render_until(&mut state, "1:sh");
    assert!(!terminal::allow_quit(&mut state, false));
    assert!(render(&mut state).contains("Quit and close 1 shell?"));

    // `n` answers no, and every other key waits for an answer.
    let x = key(KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(terminal::handle_key(&mut state, &x), KeyOutcome::Handled);
    assert!(state.terminal.pending().is_some());
    let n = key(KeyCode::Char('n'), KeyModifiers::NONE);
    terminal::handle_key(&mut state, &n);
    assert!(state.terminal.pending().is_none());

    // `y` answers yes, and Ctrl+C's quit stays a cancel.
    assert!(!terminal::allow_quit(&mut state, true));
    let y = key(KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(
        terminal::handle_key(&mut state, &y),
        KeyOutcome::Quit { cancel: true }
    );

    state.config.terminal.confirm_quit = "never".into();
    assert!(terminal::allow_quit(&mut state, false));
}

#[test]
fn an_idle_shell_does_not_hold_up_a_quit_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_shell_profile(dir.path());
    terminal::handle_key(&mut state, &toggle());
    // Wait for the prompt, so the shell is idle rather than still starting.
    render_until(&mut state, &prompt_marker(dir.path()));
    assert_eq!(state.config.terminal.confirm_quit, "when_busy");
    assert!(terminal::allow_quit(&mut state, false));
}

/// Something every prompt of the test shell contains once it is ready.
fn prompt_marker(dir: &std::path::Path) -> String {
    if cfg!(windows) {
        // cmd's prompt is the directory followed by `>`; the end of the path is
        // enough, and avoids how a long temp path wraps.
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        format!("{name}>")
    } else {
        "$".to_owned()
    }
}
