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
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::actions::Action;
use crate::app::state::AppState;
use crate::cli::Cli;
use crate::input::InputCtx;
use crate::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewRegistry};

fn main() -> Result<()> {
    let cli = Cli::parse();

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

    // Build initial app state from the CLI start path before entering raw
    // mode so that a bad start path produces a normal error message rather
    // than a broken terminal.
    let mut state = AppState::new(cli.start_path)
        .map_err(|e| anyhow::anyhow!("failed to open start directory: {e}"))?;

    // Build the preview registry once at startup. All providers are registered
    // here; the main loop calls registry.preview_for on every selection change.
    let mut registry = PreviewRegistry::new();
    preview::register_defaults(&mut registry);

    // Compute the initial preview for whatever is selected at startup.
    refresh_preview(&mut state, &registry);

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

    let run_result = run_event_loop(&mut terminal, &mut state, &registry);

    // Teardown: leave alternate screen and restore cooked mode regardless of
    // whether the event loop exited cleanly or with an error.
    execute!(stdout(), LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    run_result
}

/// Updates `state.preview` synchronously for the currently selected entry.
///
/// Called after every action that might change the selection or current
/// directory. Increments `state.preview.generation` on every call so that
/// Phase 4/5 worker results for a since-abandoned selection can be discarded.
fn refresh_preview(state: &mut AppState, registry: &PreviewRegistry) {
    state.preview.generation = state.preview.generation.wrapping_add(1);

    if let Some(entry) = state.selected_entry().cloned() {
        state.preview.for_path = entry.path.clone();
        let ctx = PreviewCtx {
            show_hidden: state.show_hidden,
        };
        match registry.preview_for(&entry, &ctx) {
            PreviewOutcome::Ready(content) => {
                state.preview.content = content;
            }
            PreviewOutcome::Deferred => {
                // A worker was spawned. Show loading placeholder until Phase 4
                // wires the channel and merge path.
                state.preview.content = PreviewContent::Loading;
            }
        }
    } else {
        state.preview.content = PreviewContent::Empty;
        state.preview.for_path = std::path::PathBuf::new();
    }
    state.dirty = true;
}

/// Runs the main event loop until the user quits.
///
/// Phase 1: polls for terminal key events, dispatches them through
/// `input::dispatch`, applies the resulting `Action` via `actions::apply`,
/// updates the preview on any navigation change, and re-renders only when
/// `state.dirty` is set.
///
/// Phase 4 will extend this with `select!` across terminal input, the worker
/// channel, and the filesystem watch signal.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    registry: &PreviewRegistry,
) -> Result<()> {
    let mut ctx = InputCtx::default();
    let mut should_quit = false;

    // Initial render.
    ui::render(terminal, state)?;
    state.dirty = false;

    while !should_quit {
        // Block until a terminal event arrives.
        let event = event::read()?;

        if let Event::Key(key) = event {
            // Track whether the selection or directory changed so we know
            // whether to refresh the preview.
            let old_selected = state.selected;
            let old_cwd = state.cwd.clone();

            if let Some(action) = input::dispatch(key, state, &mut ctx) {
                if action == Action::Quit {
                    should_quit = true;
                } else {
                    // Clear any pending multi-key nav prefix unless this action
                    // *is* the prefix setter itself. This ensures `y` sets the
                    // prefix, and the very next key resolves it (or clears it).
                    let is_prefix = matches!(action, Action::SetPendingNavKey(_));
                    // Log navigation errors at debug level and continue rather
                    // than crashing — a bad directory is inconvenient, not fatal.
                    if let Err(e) = actions::apply(action, state) {
                        tracing::debug!("action error: {e}");
                    }
                    if !is_prefix {
                        // The second key of a multi-key sequence was consumed
                        // (or a non-prefix key cleared the pending state).
                        if state.pending_nav_key.is_some() {
                            state.pending_nav_key = None;
                            state.dirty = true;
                        }
                    }
                }
            } else if state.pending_nav_key.is_some() {
                // A key was pressed that did not produce an action while a
                // prefix was pending. The prefix was already consumed by the
                // keymap (returned None), so clear it now.
                state.pending_nav_key = None;
                state.dirty = true;
            }

            // Refresh preview whenever the selection or directory changed.
            if state.selected != old_selected || state.cwd != old_cwd {
                refresh_preview(state, registry);
            }
        }

        // Re-render only when something changed.
        if state.dirty {
            ui::render(terminal, state)?;
            state.dirty = false;
        }
    }

    Ok(())
}
