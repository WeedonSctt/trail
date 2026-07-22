//! Trail — a terminal file manager.
//!
//! Entry point: parses CLI arguments, initializes the terminal in raw mode
//! with an alternate screen, installs a panic hook that restores the terminal,
//! runs the event loop, and tears down cleanly on exit.

#![forbid(unsafe_code)]

mod actions;
mod app;
mod cli;
mod config;
mod input;
mod plugin;
mod preview;
mod session;
mod ui;
mod workers;

use std::io::{self, stdout};
use std::panic;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::cli::Cli;

fn main() -> Result<()> {
    let _cli = Cli::parse();

    // Install the panic hook BEFORE touching terminal state, so a panic at
    // any point during execution restores the terminal rather than leaving it
    // broken. This is a Phase 0 deliverable explicitly called out in the
    // implementation plan and coding standard (§5).
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Best-effort terminal restoration — if this itself fails, there's
        // nothing more we can do.
        let _ = execute!(stdout(), LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        default_hook(info);
    }));

    // Enter raw mode and the alternate screen. These are the same operations
    // Phase 6's suspend/resume will reuse, so establishing the enter/exit
    // pair correctly here avoids rework later.
    terminal::enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout());
    // expect: terminal init is a genuinely unrecoverable startup failure,
    // and the panic hook is already installed above, so the terminal will be
    // restored before the process exits.
    let mut terminal = Terminal::new(backend).expect("failed to initialize terminal");

    let run_result = run_event_loop(&mut terminal);

    // Teardown: leave alternate screen and restore cooked mode regardless of
    // whether the event loop exited cleanly or with an error.
    execute!(stdout(), LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    run_result
}

/// Runs the main event loop until the user quits.
///
/// Phase 0: polls for terminal key events and exits on `q`. Later phases
/// extend this with `select!` across terminal input, worker channels, and
/// the filesystem watch signal.
fn run_event_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    // Initial render of the empty three-panel layout.
    ui::render(terminal)?;

    loop {
        // Block until a terminal event arrives.
        if let Event::Key(key) = event::read()? {
            // crossterm on Windows fires both Press and Release events;
            // only handle Press to avoid double-processing.
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                _ => {
                    // Phase 0: no other keys are handled yet.
                    // TODO(phase-1): Dispatch to input::dispatch(key, state, ctx).
                }
            }
        }

        // Re-render after every event. Phase 1 introduces the `state.dirty`
        // guard so we only render when state actually changed.
        ui::render(terminal)?;
    }

    Ok(())
}
