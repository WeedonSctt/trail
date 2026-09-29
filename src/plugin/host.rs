//! Applying what plugins asked for.
//!
//! The engine queues [`PluginRequest`]s while a hook runs, because the hook
//! runs with `AppState` borrowed. This module is the other half: once the
//! borrow has ended, [`settle`] drains the queue and applies each request —
//! through [`crate::actions::apply`] wherever an `Action` already does the job,
//! so a plugin gets exactly the behaviour a keystroke gets.
//!
//! Applying a request can fire more hooks (a navigation fires `on_enter_dir`,
//! a new selection fires `on_select`), which queue more requests. `settle`
//! drains in rounds and stops after [`MAX_ROUNDS`], so two plugins that answer
//! each other's events cannot freeze the UI thread. See
//! `docs/plugin_api_plan.md` §3.2.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use crate::actions::{self, shell_exec, Action};
use crate::app::state::AppState;
use crate::plugin::request::{CommandSpec, JobResult, PluginRequest};
use crate::workers::WorkerMsg;

/// How many rounds of requests one input event may cause.
///
/// One round is what the plugins asked for directly; each further round is a
/// reaction to the round before. Four covers any sensible chain — a jump that
/// triggers an `on_enter_dir` that selects something that triggers `on_select`
/// — while a genuine loop is cut off before anyone notices a delay.
pub const MAX_ROUNDS: usize = 4;

/// Applies every queued plugin request, in rounds, until the queue is empty or
/// [`MAX_ROUNDS`] is reached.
///
/// `refresh` is called after any round that may have changed what is on
/// screen, so the caller can re-run the preview — which is also what fires
/// `on_select` for the next round. Requests still queued after the last round
/// are dropped and reported, rather than carried into the next input event
/// where they would look like something the user did.
pub fn settle(
    state: &mut AppState,
    worker_tx: &mpsc::Sender<WorkerMsg>,
    refresh: &mut dyn FnMut(&mut AppState),
) {
    for _ in 0..MAX_ROUNDS {
        announce_dir(state);
        let requests = take(state);
        if requests.is_empty() {
            return;
        }
        let visible = requests.iter().any(changes_view);
        let before = (state.selected, state.cwd.clone());
        for request in requests {
            apply(request, state, worker_tx);
        }
        if visible || before != (state.selected, state.cwd.clone()) {
            refresh(state);
        }
    }
    announce_dir(state);
    let dropped = take(state);
    if !dropped.is_empty() {
        state.set_error(format!(
            "plugins: stopped after {MAX_ROUNDS} rounds of requests causing more requests \
             ({} dropped) — two hooks are probably answering each other",
            dropped.len()
        ));
    }
}

/// Fires `on_enter_dir` if the directory changed since plugins were last told.
fn announce_dir(state: &mut AppState) {
    if state.plugin_announced_dir.as_ref() == Some(&state.cwd) {
        return;
    }
    state.plugin_announced_dir = Some(state.cwd.clone());
    if let Some(engine) = &state.plugin_engine {
        engine.fire_on_enter_dir(state, &state.cwd);
    }
}

fn take(state: &AppState) -> Vec<PluginRequest> {
    state
        .plugin_engine
        .as_ref()
        .map(|e| e.take_requests())
        .unwrap_or_default()
}

/// Whether a request can change the listing or the preview, as opposed to
/// only the status bar or the clipboard.
fn changes_view(request: &PluginRequest) -> bool {
    !matches!(
        request,
        PluginRequest::Notify(_)
            | PluginRequest::Error(_)
            | PluginRequest::SetStatus(_)
            | PluginRequest::Yank(_)
            | PluginRequest::Spawn { .. }
    )
}

/// Applies one request to `state`.
///
/// Failures are reported through [`AppState::set_error`], never returned: a
/// plugin asking for something impossible is the plugin's problem, and the
/// requests queued after it still run.
pub fn apply(request: PluginRequest, state: &mut AppState, worker_tx: &mpsc::Sender<WorkerMsg>) {
    match request {
        PluginRequest::Navigate(path) => {
            let target = resolve(&state.cwd, &path);
            if !target.is_dir() {
                state.set_error(format!(
                    "plugin navigate: {} is not a directory",
                    crate::pathfmt::display(&target)
                ));
            } else if let Err(e) = state.enter_dir(target) {
                state.set_error(format!("plugin navigate: {e}"));
            }
        }
        PluginRequest::Select(target) => select(state, &target),
        PluginRequest::Move(n) => {
            let step = if n < 0 {
                Action::MoveUp
            } else {
                Action::MoveDown
            };
            // Bounded by the listing: moving further than the list is long
            // changes nothing more, so there is no reason to loop past it.
            let times = n.unsigned_abs().min(state.filtered_count() as u64);
            for _ in 0..times {
                run(state, step.clone());
            }
        }
        PluginRequest::GoParent => run(state, Action::GoParent),
        PluginRequest::Back => run(state, Action::HistoryBack),
        PluginRequest::Forward => run(state, Action::HistoryForward),
        PluginRequest::Refresh => run(state, Action::Refresh),
        PluginRequest::CloseTab => run(state, Action::CloseTab),
        PluginRequest::OpenTab(path) => {
            let path = path.map(|p| resolve(&state.cwd, &p));
            if let Err(e) = state.open_tab(path) {
                state.set_error(format!("plugin open_tab: {e}"));
            }
        }
        PluginRequest::SetSort {
            by,
            reverse,
            dirs_first,
        } => {
            let mut settings = state.sort;
            if let Some(by) = by.as_deref().and_then(crate::app::sort::SortBy::parse) {
                settings.by = by;
            }
            if let Some(reverse) = reverse {
                settings.reverse = reverse;
            }
            if let Some(dirs_first) = dirs_first {
                settings.dirs_first = dirs_first;
            }
            state.set_sort(settings);
        }
        PluginRequest::SetHidden(show) => {
            if state.show_hidden != show {
                run(state, Action::ToggleHidden);
            }
        }
        PluginRequest::Yank(text) => actions::record_yank(state, Ok(text)),
        PluginRequest::Command(line) => match crate::input::command_parser::parse(&line, false) {
            Ok(command) => run(state, Action::ExecuteCommand(command)),
            Err(e) => state.set_error(format!("plugin command `{line}`: {e}")),
        },
        PluginRequest::Run { cmd, pause } => {
            let argv = argv_for(&cmd, &state.config.general.shell);
            if argv.is_empty() {
                state.set_error("plugin run: empty command".to_owned());
                return;
            }
            let pause = pause
                .as_deref()
                .and_then(shell_exec::ShellPause::parse)
                .unwrap_or_else(|| actions::configured_pause(state));
            state.pending_external = Some(Action::RunExternal {
                argv,
                cwd: state.cwd.clone(),
                pause,
            });
            state.dirty = true;
        }
        PluginRequest::Spawn { id, cmd, cwd } => spawn(state, worker_tx, id, cmd, cwd),
        PluginRequest::Notify(text) => state.notify(text),
        PluginRequest::Error(text) => state.set_error(text),
        PluginRequest::SetStatus(text) => {
            state.plugin_status = text;
            state.dirty = true;
        }
    }
}

/// Runs `action` the way a keystroke would, reporting a failure.
fn run(state: &mut AppState, action: Action) {
    if let Err(e) = actions::apply(action, state) {
        state.set_error(format!("plugin: {e}"));
    }
}

/// A path a plugin gave, made absolute against `cwd` and with `.` and `..`
/// resolved lexically.
///
/// Lexically rather than through the filesystem: a plugin's `navigate("..")`
/// should land where the user would by pressing `h`, not wherever a symlink
/// in between points.
fn resolve(cwd: &Path, path: &Path) -> PathBuf {
    use std::path::Component;
    let joined = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Moves the selection to the entry `target` names: a file name in the
/// current listing, or a path.
fn select(state: &mut AppState, target: &str) {
    let as_path = resolve(&state.cwd, Path::new(target));
    let found = state.filtered_entries().position(|(_, e)| {
        e.file_name == target || e.path == as_path || crate::pathfmt::display(&e.path) == target
    });
    match found {
        Some(index) => {
            if state.selected != index {
                state.selected = index;
                state.dirty = true;
            }
        }
        None => state.set_error(format!("plugin select: no '{target}' in this listing")),
    }
}

fn argv_for(cmd: &CommandSpec, shell: &str) -> Vec<String> {
    match cmd {
        CommandSpec::Argv(argv) => argv.clone(),
        CommandSpec::Shell(line) => shell_exec::shell_argv(shell, line),
    }
}

/// Starts job `id` on the blocking pool and posts its result back to the UI
/// thread as `WorkerMsg::PluginJob`.
///
/// `spawn_blocking` rather than an async process: a job may run for minutes,
/// and a blocking-pool thread waiting on it costs nothing the async workers
/// need. It must run inside the runtime; outside one (a unit test that never
/// started one) the job is reported as failed instead of panicking.
fn spawn(
    state: &mut AppState,
    worker_tx: &mpsc::Sender<WorkerMsg>,
    id: u64,
    cmd: CommandSpec,
    cwd: Option<PathBuf>,
) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        state.set_error("plugin spawn: no async runtime to run the job on".to_owned());
        return;
    };
    let cwd = cwd
        .map(|c| resolve(&state.cwd, &c))
        .unwrap_or_else(|| state.cwd.clone());
    let shell = state.config.general.shell.clone();
    let tx = worker_tx.clone();
    runtime.spawn(async move {
        let result =
            tokio::task::spawn_blocking(move || JobResult::run_blocking(&cmd, &shell, &cwd))
                .await
                .unwrap_or_else(|e| JobResult {
                    ok: false,
                    code: None,
                    stdout: String::new(),
                    stderr: format!("job did not finish: {e}"),
                });
        // If the channel is closed the UI thread has exited; nobody is waiting.
        let _ = tx.send(WorkerMsg::PluginJob { id, result }).await;
    });
}
