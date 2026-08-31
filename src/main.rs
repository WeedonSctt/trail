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
mod paths;
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

use crate::actions::shell_exec;
use crate::actions::Action;
use crate::app::state::AppState;
use crate::cli::Cli;
use crate::input::InputCtx;
use crate::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewRegistry};
use crate::workers::fswatch::FsWatchHandle;
use crate::workers::{GitCache, WorkerMsg};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // `--paths` is a report, not a session. Answer it and leave before the
    // panic hook, the logger or the terminal are touched, so the output is
    // ordinary stdout that can be piped, redirected or read by a script.
    if cli.paths {
        println!("{}", paths::report());
        return Ok(());
    }

    // Capture cwd_file path before consuming `cli` below.
    let cwd_file = cli.cwd_file.clone();

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
        let file_appender = tracing_appender::rolling::never(&log_dir, paths::LOG_FILE);
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        let _ = tracing_subscriber::fmt()
            .with_writer(non_blocking)
            .with_env_filter(
                tracing_subscriber::EnvFilter::builder()
                    .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                    .from_env_lossy(),
            )
            .try_init();
        // _guard is intentionally leaked: it lives for the process lifetime.
        std::mem::forget(_guard);
    }

    // The data directory holds everything Trail persists between runs
    // (bookmarks, recent directories, the remembered --config path). Resolve
    // it before loading config, since the remembered path lives there.
    let data_dir = paths::data_dir();

    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        tracing::debug!("failed to create data dir: {e}");
    }

    let config_state_file = data_dir.join(config::last_used::STATE_FILE_NAME);

    // Decide which config to load: an explicit --config, else the path
    // remembered from a previous run, else the built-in defaults.
    let config_source = config::last_used::resolve(
        cli.config.clone(),
        config::last_used::remembered(&config_state_file),
        cli.no_config,
    );

    // Build initial app state from the CLI start path before entering raw
    // mode so that a bad start path produces a normal error message rather
    // than a broken terminal.
    let (config, config_warning) = load_configured(&config_source)?;

    // Only record a path the user named explicitly, and only once it has
    // actually loaded — a file that fails to parse must never become the one
    // Trail reaches for on the next run.
    if let config::ConfigSource::Explicit(ref path) = config_source {
        if let Err(e) = config::last_used::remember(&config_state_file, path) {
            // Non-fatal: this session is fine, it just won't be recalled.
            tracing::debug!("failed to remember --config path: {e}");
        }
    }

    let mut state = AppState::with_config(cli.start_path, config)
        .map_err(|e| anyhow::anyhow!("failed to open start directory: {e}"))?;

    // Surface a degraded remembered config in the status bar. Trail owns the
    // alternate screen, so this is the only channel available.
    state.error_message = config_warning;

    state.bookmark_store =
        match plugin::bookmarks::BookmarkStore::open(data_dir.join(paths::BOOKMARKS_FILE)) {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::debug!("failed to load bookmarks: {e}");
                None
            }
        };

    state.recent_dirs = session::RecentDirs::load(&data_dir.join(paths::RECENT_DIRS_FILE));
    state.recent_dirs.visit(state.cwd.clone());

    let mut engine = plugin::PluginEngine::new()
        .map_err(|e| anyhow::anyhow!("failed to init plugin engine: {e}"))?;

    plugin::load_enabled_plugins(&mut engine, &state.config.plugins.enabled);
    state.plugin_engine = Some(engine);

    // Resolve the terminal's inline-image protocol before the first preview is
    // requested, so image decodes never race the detection.
    preview::graphics::configure(
        &state.config.preview.image_protocol,
        (
            state.config.preview.image_cell_width,
            state.config.preview.image_cell_height,
        ),
    );

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
    if state.config.general.git_status_enabled {
        workers::git::spawn_git_status(initial_cwd.clone(), worker_tx.clone(), git_cache.clone());
    }

    // Start the filesystem watcher for the initial directory.
    let mut fs_watch_handle: Option<FsWatchHandle> = workers::fswatch::spawn_fswatch(
        initial_cwd,
        worker_tx.clone(),
        state.config.general.fs_watch_debounce_ms,
    );

    // Enter raw mode and the alternate screen. The suspend/resume sequence in
    // shell_exec::run_external reuses the same crossterm operations, so
    // establishing the enter/exit pair correctly here avoids rework later.
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

    // Phase 6: On normal exit, write the current directory to `--cwd-file`
    // so the shell wrapper can `cd` into it. On cancellation or error the
    // file is not written — the shell wrapper then does nothing.
    let cancelled = matches!(&run_result, Err(ref e) if e.to_string() == "cancelled");
    if run_result.is_ok() {
        if let Some(ref path) = cwd_file {
            if let Err(e) = session::write_cwd_file(&state.cwd, path) {
                // Non-fatal: log and continue. The user still exits cleanly;
                // they just won't be `cd`-ed to the right directory this time.
                tracing::debug!("failed to write cwd-file: {e}");
            }
        }

        state
            .recent_dirs
            .save(&data_dir.join(paths::RECENT_DIRS_FILE));
    } else if cancelled {
        // Cancelled (Ctrl+C): save recent dirs but do NOT write cwd-file.
        state
            .recent_dirs
            .save(&data_dir.join(paths::RECENT_DIRS_FILE));
    }

    // Re-surface real errors; treat "cancelled" as a clean exit.
    if cancelled {
        Ok(())
    } else {
        run_result
    }
}

/// Loads the configuration named by `source`, applying the failure policy that
/// the source implies.
///
/// Returns the config plus an optional warning to show in the status bar.
///
/// # Errors
///
/// Returns an error only for [`config::ConfigSource::Explicit`]: the user named
/// that file on the command line, so a typo or a syntax error must fail loudly
/// rather than silently starting with different settings. A
/// [`config::ConfigSource::Remembered`] path that no longer loads is reported
/// as a warning and falls back to the built-in defaults instead — it was
/// recorded by an earlier run and may since have been moved, deleted or edited,
/// and refusing to start would leave the user with no obvious way back.
fn load_configured(source: &config::ConfigSource) -> Result<(config::TrailConfig, Option<String>)> {
    match source {
        config::ConfigSource::Defaults => {
            tracing::info!("using built-in default configuration");
            let config =
                config::load(None).map_err(|e| anyhow::anyhow!("failed to load config: {e}"))?;
            Ok((config, None))
        }

        config::ConfigSource::Explicit(path) => {
            tracing::info!(?path, "loading configuration from --config");
            let config = config::load(Some(path))
                .map_err(|e| anyhow::anyhow!("failed to load config: {e}"))?;
            Ok((config, None))
        }

        config::ConfigSource::Remembered(path) => match config::load(Some(path)) {
            Ok(config) => {
                tracing::info!(?path, "loading remembered configuration");
                Ok((config, None))
            }
            Err(e) => {
                tracing::warn!(
                    ?path,
                    "remembered config failed to load; falling back to defaults: {e}"
                );
                let config = config::load(None)
                    .map_err(|e| anyhow::anyhow!("failed to load config: {e}"))?;
                let warning = format!(
                    "remembered config {} failed to load ({e}); using defaults",
                    path.display()
                );
                Ok((config, Some(warning)))
            }
        },
    }
}

/// Updates `state.preview` synchronously for the currently selected entry.
///
/// Called after every action that might change the selection or current
/// directory. Increments `state.preview.generation` on every call so that
/// Phase 4/5 worker results for a since-abandoned selection can be discarded.
///
/// The `tx` sender is passed through to `PreviewCtx` so providers can spawn
/// async worker tasks (highlight worker, image decode worker).
fn refresh_preview(state: &mut AppState, registry: &PreviewRegistry, tx: &mpsc::Sender<WorkerMsg>) {
    state.preview.generation = state.preview.generation.wrapping_add(1);

    if let Some(entry) = state.selected_entry().cloned() {
        if let Some(engine) = &state.plugin_engine {
            engine.fire_on_select(&entry.path);
        }
        state.preview.for_path = entry.path.clone();
        let ctx = PreviewCtx {
            show_hidden: state.show_hidden,
            worker_tx: tx.clone(),
            generation: state.preview.generation,
            text_sync_threshold_bytes: state.config.general.text_sync_threshold_kb * 1024,
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
    debounce_ms: u64,
) {
    // Drop the current handle, which cancels the old watch task.
    *handle = None;
    *handle = workers::fswatch::spawn_fswatch(new_cwd, tx.clone(), debounce_ms);
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
    let mut cancelled = false;

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
        // Track whether a non-preview worker message (Git, FsChanged) made the
        // state dirty. Preview/ImageMeta results are applied directly by
        // merge() and must NOT trigger another refresh_preview call — doing so
        // would overwrite the just-merged content with `Loading` again.
        let mut needs_preview_refresh = false;
        while let Ok(msg) = worker_rx.try_recv() {
            let is_listing_change =
                matches!(&msg, WorkerMsg::Git { .. } | WorkerMsg::FsChanged { .. });
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
            if is_listing_change && state.dirty {
                needs_preview_refresh = true;
            }
        }

        // Refresh preview only when a listing-level change (FsChanged / Git)
        // dirtied the state — e.g. FsChanged triggered a directory refresh and
        // the selected entry may have changed. Preview/ImageMeta results are
        // already applied by merge() and must not be re-requested here.
        if needs_preview_refresh {
            refresh_preview(state, registry, &worker_tx);
        }

        if !worker_drained {
            // Block until either a terminal event or worker message arrives.
            tokio::select! {
                // Terminal input.
                maybe_event = event_stream.next() => {
                    match maybe_event {
                        Some(Ok(event)) => {
                            match event {
                                Event::Key(key) => {
                                    handle_key_event(
                                        key,
                                        terminal,
                                        state,
                                        registry,
                                        &worker_tx,
                                        &git_cache,
                                        fs_watch_handle,
                                        &mut ctx,
                                        &mut should_quit,
                                        &mut cancelled,
                                    );
                                }
                                Event::Resize(_, _) => {
                                    // A terminal resize desynchronises ratatui's
                                    // internal double-buffer from the physical
                                    // terminal. Call terminal.clear() to reset
                                    // ratatui's previous buffer so the next
                                    // draw() performs a complete redraw of every
                                    // cell, eliminating ghost content from the
                                    // old frame.
                                    if let Err(e) = terminal.clear() {
                                        tracing::debug!("terminal clear after resize failed: {e}");
                                    }
                                    state.dirty = true;
                                }
                                _ => {}
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

    if cancelled {
        Err(anyhow::anyhow!("cancelled"))
    } else {
        Ok(())
    }
}

/// Handles a single terminal key event.
///
/// Updates selection/directory as needed, refreshes the preview, re-subscribes
/// the fs watcher and spawns a git worker if the directory changed.
/// Phase 6: also accepts `terminal` so it can call `terminal.clear()` after
/// returning from `shell_exec::run_external`.
#[allow(clippy::too_many_arguments)]
fn handle_key_event(
    key: event::KeyEvent,
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    registry: &PreviewRegistry,
    worker_tx: &mpsc::Sender<WorkerMsg>,
    git_cache: &GitCache,
    fs_watch_handle: &mut Option<FsWatchHandle>,
    ctx: &mut InputCtx,
    should_quit: &mut bool,
    cancelled: &mut bool,
) {
    let old_selected = state.selected;
    let old_cwd = state.cwd.clone();

    if let Some(action) = input::dispatch(key, state, ctx) {
        if action == Action::Quit {
            *should_quit = true;
            return;
        }
        if action == Action::Cancel {
            *should_quit = true;
            *cancelled = true;
            return;
        }

        // Track whether this action sets the pending prefix so we can
        // conditionally clear it on the next non-prefix key.
        let is_prefix = matches!(action, Action::SetPendingNavKey(_));
        // Track whether the action may change the listing content without
        // changing the selected index or cwd (e.g. Refresh, ToggleHidden).
        let forces_preview_refresh = matches!(action, Action::Refresh | Action::ToggleHidden);

        // Log navigation errors at debug level and continue rather than
        // crashing — a bad directory is inconvenient, not fatal.
        if let Err(e) = actions::apply(action, state) {
            tracing::debug!("action error: {e}");
        }

        if !is_prefix && state.pending_nav_key.is_some() {
            state.pending_nav_key = None;
            state.dirty = true;
        }

        // Refresh preview whenever the selection or directory changed, or the
        // action explicitly requires it (Refresh, ToggleHidden).
        if state.selected != old_selected || state.cwd != old_cwd || forces_preview_refresh {
            refresh_preview(state, registry, worker_tx);
        }
    } else if key.kind == crossterm::event::KeyEventKind::Press && state.pending_nav_key.is_some() {
        // A keypress while a prefix was pending but produced no action —
        // cancel the sequence. Release/Repeat events are intentionally
        // excluded: crossterm fires both Press *and* Release on every
        // keystroke (especially on Windows), so allowing Release to clear
        // the pending key would prevent any two-character sequence from
        // ever completing.
        state.pending_nav_key = None;
        state.dirty = true;
    }

    // If the directory changed, re-subscribe the watcher and spawn git.
    if state.cwd != old_cwd {
        resubscribe_fswatch(
            state.cwd.clone(),
            worker_tx,
            fs_watch_handle,
            state.config.general.fs_watch_debounce_ms,
        );
        if state.config.general.git_status_enabled {
            workers::git::spawn_git_status(state.cwd.clone(), worker_tx.clone(), git_cache.clone());
        }
        // Clear stale git state immediately so the old branch doesn't linger.
        state.git = None;
    }

    // Phase 6: drain any pending external action (editor-open or !shell).
    // run_external is synchronous (blocks until the child exits) but we must
    // call it here on the UI thread because it manipulates terminal state.
    if let Some(Action::RunExternal { argv, cwd }) = state.pending_external.take() {
        let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        if let Err(e) = shell_exec::run_external(&argv_refs, &cwd) {
            tracing::debug!("run_external error: {e}");
            state.error_message = Some(format!("exec: {e}"));
        }
        // Force a full redraw: the child process may have overwritten the screen.
        // terminal.clear() resets ratatui's internal buffer so the next render
        // redraws every cell from scratch, preventing ghost characters.
        if let Err(e) = terminal.clear() {
            tracing::debug!("terminal clear after run_external failed: {e}");
        }
        // Refresh the listing and preview — the subprocess may have changed
        // the filesystem (e.g. saving a file in the editor).
        let _ = state.refresh();
        refresh_preview(state, registry, worker_tx);
        state.dirty = true;
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

            // Determine whether this change is relevant to what we're showing:
            // (a) the watched directory itself changed, or
            // (b) a .git/HEAD file inside the current repo changed (branch switch).
            let is_cwd_change = changed_path == state.cwd;
            let is_git_head_change = changed_path
                .file_name()
                .map(|n| n == "HEAD")
                .unwrap_or(false)
                && changed_path
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n == ".git")
                    .unwrap_or(false);

            if is_cwd_change {
                // Invalidate the git cache so the next git worker gets fresh data.
                workers::git::invalidate(git_cache, &changed_path);

                // Refresh the directory listing.
                if let Err(e) = state.refresh() {
                    tracing::debug!("refresh after FsChanged failed: {e}");
                }

                // Re-spawn the git worker for the now-refreshed directory.
                if state.config.general.git_status_enabled {
                    workers::git::spawn_git_status(
                        changed_path.clone(),
                        worker_tx.clone(),
                        git_cache.clone(),
                    );
                }

                tracing::debug!(?changed_path, "refreshed after FsChanged");
            } else if is_git_head_change {
                // A branch switch occurred — invalidate cache and re-run git worker.
                workers::git::invalidate(git_cache, &state.cwd);
                if state.config.general.git_status_enabled {
                    workers::git::spawn_git_status(
                        state.cwd.clone(),
                        worker_tx.clone(),
                        git_cache.clone(),
                    );
                }
                state.dirty = true;
                tracing::debug!(?changed_path, "git branch change detected via .git/HEAD");
            }

            // Regardless, resubscribe (the OS may have replaced the watched inode).
            resubscribe_fswatch(
                state.cwd.clone(),
                worker_tx,
                fs_watch_handle,
                state.config.general.fs_watch_debounce_ms,
            );
        }
        _ => {
            // All other messages go through the standard merge path.
            workers::merge(msg, state);
        }
    }
}
