//! Trail — a terminal file manager.
//!
//! Entry point: parses CLI arguments, initializes the terminal in raw mode
//! with an alternate screen, installs a panic hook that restores the terminal,
//! runs the event loop, and tears down cleanly on exit.
//!
//! Phase 4 extends the event loop with `select!` across terminal input, the
//! worker channel (`WorkerMsg`), and filesystem watch signals.

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
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, Event, EventStream};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::actions::Action;
use crate::app::state::AppState;
use crate::cli::Cli;
use crate::input::InputCtx;
use crate::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewRegistry};
use crate::workers::fswatch::{FsWatchHandle, DEFAULT_DEBOUNCE_MS};
use crate::workers::{GitCache, WorkerMsg};

#[tokio::main]
async fn main() -> Result<()> {
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

    // Set up structured logging to a file (never stdout/stderr — we own the
    // alternate screen). Log files go to the system's tmp directory; Phase 9
    // can route them to a proper log dir.
    //
    // Failures here are non-fatal: the app works fine without logs.
    if let Ok(log_dir) = std::env::temp_dir().canonicalize() {
        let file_appender = tracing_appender::rolling::never(&log_dir, "trail.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        let _ = tracing_subscriber::fmt()
            .with_writer(non_blocking)
            .try_init();
        // _guard is intentionally leaked: it lives for the process lifetime.
        std::mem::forget(_guard);
    }

    // Build initial app state from the CLI start path before entering raw
    // mode so that a bad start path produces a normal error message rather
    // than a broken terminal.
    let mut state = AppState::new(cli.start_path)
        .map_err(|e| anyhow::anyhow!("failed to open start directory: {e}"))?;

    // Build the preview registry once at startup. All providers are registered
    // here; the main loop calls registry.preview_for on every selection change.
    let mut registry = PreviewRegistry::new();
    preview::register_defaults(&mut registry);

    // Set up the worker channel (single mpsc, drained once per UI tick).
    let (worker_tx, worker_rx) = workers::channel();

    // Set up the git cache (shared between the UI thread and worker tasks).
    let git_cache = workers::git::new_cache();

    // Compute the initial preview for whatever is selected at startup.
    refresh_preview(&mut state, &registry, &worker_tx);

    // Spawn the initial git status worker for the starting directory.
    let initial_cwd = state.cwd.clone();
    workers::git::spawn_git_status(initial_cwd.clone(), worker_tx.clone(), git_cache.clone());

    // Start the filesystem watcher for the initial directory.
    let mut fs_watch_handle: Option<FsWatchHandle> =
        workers::fswatch::spawn_fswatch(initial_cwd, worker_tx.clone(), DEFAULT_DEBOUNCE_MS);

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

    let run_result = run_event_loop(
        &mut terminal,
        &mut state,
        &registry,
        worker_rx,
        worker_tx.clone(),
        git_cache,
        &mut fs_watch_handle,
    )
    .await;

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
///
/// The `tx` sender is passed through to `PreviewCtx` so providers can spawn
/// async worker tasks (used by Phase 5's highlight and image workers).
fn refresh_preview(
    state: &mut AppState,
    registry: &PreviewRegistry,
    _tx: &mpsc::Sender<WorkerMsg>,
) {
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
                // A worker was spawned. Show loading placeholder until the
                // channel message arrives and merge() applies the content.
                state.preview.content = PreviewContent::Loading;
            }
        }
    } else {
        state.preview.content = PreviewContent::Empty;
        state.preview.for_path = PathBuf::new();
    }
    state.dirty = true;
}

/// Re-subscribes the filesystem watcher to `new_cwd`, replacing the previous
/// handle. The old handle is dropped, which cancels the old watch task.
fn resubscribe_fswatch(
    new_cwd: PathBuf,
    tx: &mpsc::Sender<WorkerMsg>,
    handle: &mut Option<FsWatchHandle>,
) {
    // Drop the current handle, which cancels the old watch task.
    *handle = None;
    *handle = workers::fswatch::spawn_fswatch(new_cwd, tx.clone(), DEFAULT_DEBOUNCE_MS);
}

/// Runs the main event loop until the user quits.
///
/// Phase 4: `select!`s across three event sources:
///  1. Terminal key events (via `crossterm::event::EventStream`).
///  2. `WorkerMsg` messages from the async worker pool.
///  3. (No dedicated fs-signal channel: the watcher sends `FsChanged` over the
///     same `worker_rx` channel, matching the single-enum decision in the plan.)
///
/// On each iteration, after processing events, re-renders only when
/// `state.dirty` is set.
#[allow(clippy::too_many_arguments)]
async fn run_event_loop(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    registry: &PreviewRegistry,
    mut worker_rx: mpsc::Receiver<WorkerMsg>,
    worker_tx: mpsc::Sender<WorkerMsg>,
    git_cache: GitCache,
    fs_watch_handle: &mut Option<FsWatchHandle>,
) -> Result<()> {
    let mut ctx = InputCtx::default();
    let mut should_quit = false;

    // Initial render.
    ui::render(terminal, state)?;
    state.dirty = false;

    // Use crossterm's async EventStream for non-blocking terminal input.
    let mut event_stream = EventStream::new();

    while !should_quit {
        // Drain any worker messages that are already queued, then wait for
        // either a terminal event or the next worker message.
        //
        // Priority: worker messages are drained first (they may be many in a
        // burst), then the select handles whichever arrives next.
        let mut worker_drained = false;
        while let Ok(msg) = worker_rx.try_recv() {
            let prev_cwd = state.cwd.clone();
            handle_worker_msg(
                msg,
                state,
                &worker_tx,
                &git_cache,
                fs_watch_handle,
                &prev_cwd,
            );
            worker_drained = true;
        }

        if !worker_drained {
            // Block until either a terminal event or worker message arrives.
            tokio::select! {
                // Terminal input.
                maybe_event = event_stream.next() => {
                    match maybe_event {
                        Some(Ok(event)) => {
                            if let Event::Key(key) = event {
                                handle_key_event(
                                    key,
                                    state,
                                    registry,
                                    &worker_tx,
                                    &git_cache,
                                    fs_watch_handle,
                                    &mut ctx,
                                    &mut should_quit,
                                );
                            }
                        }
                        Some(Err(e)) => {
                            tracing::debug!("terminal event error: {e}");
                        }
                        None => {
                            // EventStream exhausted — terminal was closed.
                            should_quit = true;
                        }
                    }
                }

                // Worker messages (git, FsChanged, Preview, ImageMeta).
                Some(msg) = worker_rx.recv() => {
                    let prev_cwd = state.cwd.clone();
                    handle_worker_msg(
                        msg,
                        state,
                        &worker_tx,
                        &git_cache,
                        fs_watch_handle,
                        &prev_cwd,
                    );
                }
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

/// Handles a single terminal key event.
///
/// Updates selection/directory as needed, refreshes the preview, re-subscribes
/// the fs watcher and spawns a git worker if the directory changed.
#[allow(clippy::too_many_arguments)]
fn handle_key_event(
    key: event::KeyEvent,
    state: &mut AppState,
    registry: &PreviewRegistry,
    worker_tx: &mpsc::Sender<WorkerMsg>,
    git_cache: &GitCache,
    fs_watch_handle: &mut Option<FsWatchHandle>,
    ctx: &mut InputCtx,
    should_quit: &mut bool,
) {
    let old_selected = state.selected;
    let old_cwd = state.cwd.clone();

    if let Some(action) = input::dispatch(key, state, ctx) {
        if action == Action::Quit {
            *should_quit = true;
            return;
        }

        // Track whether this action sets the pending prefix so we can
        // conditionally clear it on the next non-prefix key.
        let is_prefix = matches!(action, Action::SetPendingNavKey(_));

        // Log navigation errors at debug level and continue rather than
        // crashing — a bad directory is inconvenient, not fatal.
        if let Err(e) = actions::apply(action, state) {
            tracing::debug!("action error: {e}");
        }

        if !is_prefix && state.pending_nav_key.is_some() {
            state.pending_nav_key = None;
            state.dirty = true;
        }
    } else if state.pending_nav_key.is_some() {
        // A key while a prefix was pending but it didn't produce an action.
        state.pending_nav_key = None;
        state.dirty = true;
    }

    // Refresh preview whenever the selection or directory changed.
    if state.selected != old_selected || state.cwd != old_cwd {
        refresh_preview(state, registry, worker_tx);
    }

    // If the directory changed, re-subscribe the watcher and spawn git.
    if state.cwd != old_cwd {
        resubscribe_fswatch(state.cwd.clone(), worker_tx, fs_watch_handle);
        workers::git::spawn_git_status(state.cwd.clone(), worker_tx.clone(), git_cache.clone());
        // Clear stale git state immediately so the old branch doesn't linger.
        state.git = None;
    }
}

/// Handles a single `WorkerMsg` received from the worker channel.
///
/// - `Git` and `Preview`/`ImageMeta` are dispatched to `workers::merge()`.
/// - `FsChanged` additionally triggers a directory refresh and re-spawns the
///   git worker (after invalidating the cache for the changed path).
fn handle_worker_msg(
    msg: WorkerMsg,
    state: &mut AppState,
    worker_tx: &mpsc::Sender<WorkerMsg>,
    git_cache: &GitCache,
    fs_watch_handle: &mut Option<FsWatchHandle>,
    _prev_cwd: &std::path::Path,
) {
    match &msg {
        WorkerMsg::FsChanged { path } => {
            let changed_path = path.clone();
            // Only refresh if the changed directory is the one we're viewing.
            if changed_path == state.cwd {
                // Invalidate the git cache so the next git worker gets fresh data.
                workers::git::invalidate(git_cache, &changed_path);

                // Refresh the directory listing.
                if let Err(e) = state.refresh() {
                    tracing::debug!("refresh after FsChanged failed: {e}");
                }

                // Re-spawn the git worker for the now-refreshed directory.
                workers::git::spawn_git_status(
                    changed_path.clone(),
                    worker_tx.clone(),
                    git_cache.clone(),
                );

                tracing::debug!(?changed_path, "refreshed after FsChanged");
            }
            // Regardless, resubscribe (the OS may have replaced the watched inode).
            resubscribe_fswatch(state.cwd.clone(), worker_tx, fs_watch_handle);
        }
        _ => {
            // All other messages go through the standard merge path.
            workers::merge(msg, state);
        }
    }
}
