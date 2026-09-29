//! What a plugin asks Trail to do.
//!
//! A plugin never mutates `AppState`: the engine lives inside it, so a hook
//! runs while the state is borrowed. Instead every write function in the Lua
//! API pushes a [`PluginRequest`], and [`crate::plugin::host::drain`] applies
//! the queue once the hook has returned and the borrow has ended. See
//! `docs/plugin_api_plan.md` §3.2.

use std::path::PathBuf;

/// How a command a plugin runs is spelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSpec {
    /// A program and its arguments, run directly with no shell in between.
    Argv(Vec<String>),
    /// A command line, run through `[general] shell` exactly as `!` runs one.
    Shell(String),
}

/// One thing a plugin asked for, in the order it asked.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginRequest {
    /// Enter a directory. Relative paths resolve against the current one.
    Navigate(PathBuf),
    /// Move the selection to the entry with this path or file name.
    Select(String),
    /// Move the selection by this many rows; negative moves up.
    Move(i64),
    /// Go to the parent directory.
    GoParent,
    /// Step back through the navigation history.
    Back,
    /// Step forward through the navigation history.
    Forward,
    /// Re-read the listing, keeping the selection.
    Refresh,
    /// Change the active tab's order. `None` keeps the current value.
    SetSort {
        /// Sort key, spelled as `[navigation] sort_by` takes it.
        by: Option<String>,
        /// Whether the order is flipped.
        reverse: Option<bool>,
        /// Whether directories are grouped first.
        dirs_first: Option<bool>,
    },
    /// Show or hide hidden entries.
    SetHidden(bool),
    /// Put text on the clipboard.
    Yank(String),
    /// Run a Trail command line, as if typed after `:`.
    Command(String),
    /// Open a tab, at this path or at the current directory.
    OpenTab(Option<PathBuf>),
    /// Close the active tab.
    CloseTab,
    /// Suspend Trail and run a command in the terminal.
    Run {
        /// What to run.
        cmd: CommandSpec,
        /// A `[general] shell_pause` value overriding the configured one.
        pause: Option<String>,
    },
    /// Run a command on the worker pool and report back to job `id`.
    Spawn {
        /// The job id handed back to Lua, which keys the `on_exit` callback.
        id: u64,
        /// What to run.
        cmd: CommandSpec,
        /// Working directory; the current directory when `None`.
        cwd: Option<PathBuf>,
    },
    /// Show a notice.
    Notify(String),
    /// Show an error.
    Error(String),
    /// Set or clear the plugin segment of the status bar.
    SetStatus(Option<String>),
}

/// What a finished job reports back to its `on_exit` callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobResult {
    /// Whether the command ran and exited with status zero.
    pub ok: bool,
    /// The exit code, or `None` when the process was killed by a signal or
    /// could not be started at all.
    pub code: Option<i32>,
    /// Everything the command wrote to stdout, lossily decoded as UTF-8.
    pub stdout: String,
    /// Everything it wrote to stderr — or, when it could not be started, why.
    pub stderr: String,
}

impl JobResult {
    /// Runs `cmd` to completion in `cwd`, blocking the calling thread.
    ///
    /// For the worker pool only: this waits for the process however long it
    /// takes, which is exactly why it must never run on the UI thread.
    pub fn run_blocking(cmd: &CommandSpec, shell: &str, cwd: &std::path::Path) -> Self {
        let argv = match cmd {
            CommandSpec::Argv(argv) => argv.clone(),
            CommandSpec::Shell(line) => crate::actions::shell_exec::shell_argv(shell, line),
        };
        let Some((program, args)) = argv.split_first() else {
            return Self::failed("empty command".to_owned());
        };
        match std::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(out) => Self {
                ok: out.status.success(),
                code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            },
            Err(e) => Self::failed(format!("could not start `{program}`: {e}")),
        }
    }

    /// A result for a command that never ran.
    fn failed(reason: String) -> Self {
        Self {
            ok: false,
            code: None,
            stdout: String::new(),
            stderr: reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_is_a_failed_result_not_a_panic() {
        let result = JobResult::run_blocking(
            &CommandSpec::Argv(vec!["trail-no-such-program-exists".to_owned()]),
            "",
            &std::env::temp_dir(),
        );
        assert!(!result.ok);
        assert_eq!(result.code, None);
        assert!(result.stderr.contains("could not start"));
    }

    #[test]
    fn an_empty_argv_is_refused() {
        let result = JobResult::run_blocking(&CommandSpec::Argv(vec![]), "", &std::env::temp_dir());
        assert!(!result.ok);
        assert_eq!(result.stderr, "empty command");
    }

    #[test]
    fn a_shell_command_captures_stdout() {
        let result = JobResult::run_blocking(
            &CommandSpec::Shell("echo trail-job".to_owned()),
            "",
            &std::env::temp_dir(),
        );
        assert!(result.ok, "{result:?}");
        assert_eq!(result.code, Some(0));
        assert!(result.stdout.contains("trail-job"));
    }
}
