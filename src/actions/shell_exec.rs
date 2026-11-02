//! Suspend/resume subprocess execution.
//!
//! Implements the suspend/resume sequence from the architecture doc §3:
//!   1. Leave alternate screen and restore cooked terminal mode.
//!   2. Spawn the subprocess with inherited stdio, relative to `cwd`.
//!   3. Wait for the subprocess to exit.
//!   4. Re-enter raw mode and the alternate screen.
//!   5. Signal the caller that a full redraw is required (the subprocess
//!      may have overwritten the terminal).
//!
//! The same path serves both "open in configured editor" and Command Mode's
//! `!<shell command>` — the only difference is the argv passed in.
//!
//! For the command forms that need an interpreter (`!<command>` and `:git`),
//! [`shell_argv`] builds that argv from the `[general] shell` config key, so
//! which shell runs them is the user's choice rather than a compile-time
//! constant.

use std::io::{stdout, IsTerminal, Write};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};
use thiserror::Error;

use super::run_report::{answer_for, BlockAnswer, ResultBlock, RunReport, RunResult};

/// Interpreter used for `!<command>` and `:git` when `[general] shell` is blank.
///
/// These are the values Trail hardcoded before the key existed, so a config
/// that does not mention `shell` keeps behaving exactly as it did. The final
/// element of the spawned argv is always the command string, which is why the
/// flag here is the one that means "read the next argument as a script".
#[cfg(windows)]
pub const DEFAULT_SHELL_ARGV: &[&str] = &["cmd.exe", "/C"];
/// Interpreter used for `!<command>` and `:git` when `[general] shell` is blank.
///
/// `sh` rather than `$SHELL`: the command string is POSIX shell syntax, and a
/// login shell such as fish would interpret parts of it differently.
#[cfg(not(windows))]
pub const DEFAULT_SHELL_ARGV: &[&str] = &["sh", "-c"];

/// When Trail prints the result block and waits for a key before reclaiming the
/// screen from a command.
///
/// Without a pause, `run_external` re-enters the alternate screen the instant the
/// child exits, so the output of `!ls` is painted and erased in the same breath.
/// The editor is the reason this is a choice rather than always on: nobody wants
/// to press Enter after closing their editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPause {
    /// Always wait — the default for `!<command>` and `:git`.
    Always,
    /// Wait only when the command exited non-zero, or could not be run at all.
    OnError,
    /// Never wait. Used for the editor and for OS-handler opens, whatever the
    /// configured policy is, since those draw their own screens.
    Never,
}

impl ShellPause {
    /// Parses a `[general] shell_pause` value.
    ///
    /// Returns `None` for an unrecognised name so the config layer can report it
    /// with the key that carried it.
    pub fn parse(value: &str) -> Option<ShellPause> {
        match value.trim().to_ascii_lowercase().as_str() {
            "always" => Some(ShellPause::Always),
            "on_error" | "on-error" => Some(ShellPause::OnError),
            "never" | "off" => Some(ShellPause::Never),
            _ => None,
        }
    }

    /// Whether a command that finished with `success` should hold the screen.
    fn applies(self, success: bool) -> bool {
        match self {
            ShellPause::Always => true,
            ShellPause::OnError => !success,
            ShellPause::Never => false,
        }
    }
}

/// Errors that can arise while running an external process.
#[derive(Debug, Error)]
pub enum ShellExecError {
    /// Terminal state manipulation failed.
    #[error("terminal error: {0}")]
    Terminal(#[source] std::io::Error),
}

/// Leaves the alternate screen, runs `argv` in `cwd`, then restores the TUI.
///
/// The suspend/resume cycle is:
///   1. `LeaveAlternateScreen` + `disable_raw_mode` — hand the terminal back
///      to the spawned process so it can draw its own UI (e.g., a text editor).
///   2. Spawn `argv[0]` with `argv[1..]` as arguments, with `cwd` as the
///      working directory and all three stdio streams inherited from the
///      current process.
///   3. Block until the child exits.
///   4. `enable_raw_mode` + `EnterAlternateScreen` — reclaim the terminal.
///
/// **Redraw**: callers must force a full redraw after this returns, since the
/// child process may have written to the terminal. See `main.rs`'s handling of
/// [`crate::actions::Action::RunExternal`].
///
/// **Pause**: `pause` decides whether step 4 waits for an answer first. A
/// command's output lives on the normal screen, which step 4 replaces, so without
/// a pause anything `!ls` printed is gone before it can be read. The wait prints
/// the result block ([`ResultBlock`]), and `r` at the block runs the command
/// again, so steps 2 and 3 may repeat. The alternate screen is inactive
/// throughout, which is the one time writing to stdout is legitimate.
///
/// `label` is the command as typed, for the block and the report; `None` for what
/// Trail starts on the user's behalf.
///
/// **Must be called from the key handler**, as it is: the block reads keys
/// through crossterm directly, and `EventStream` holds crossterm's input lock
/// from a background thread while it waits. Once the stream has just delivered a
/// key that thread has finished, so the lock is free until the stream is next
/// polled.
///
/// # Errors
///
/// Returns [`ShellExecError`] if the terminal could not be handed over. A command
/// that fails, or cannot be started, is not an error here: that is the report's
/// to say.
pub fn run_external(
    argv: &[&str],
    cwd: &Path,
    pause: ShellPause,
    label: Option<&str>,
) -> Result<RunReport, ShellExecError> {
    if argv.is_empty() {
        return Ok(RunReport {
            label: label.map(str::to_owned),
            program: String::new(),
            result: RunResult::CouldNotRun("nothing to run".to_owned()),
            elapsed: Duration::ZERO,
        });
    }

    // ── Step 1: leave alternate screen / cooked mode ──────────────────────────
    // Best-effort: if either of these fails we still attempt to restore later.
    execute!(stdout(), LeaveAlternateScreen).map_err(ShellExecError::Terminal)?;
    terminal::disable_raw_mode().map_err(ShellExecError::Terminal)?;

    // ── Steps 2 & 3: run it, for as long as the block is answered with `r` ────
    let report = run_until_answered(
        pause,
        || run_once(argv, cwd, label),
        |report| ask_at_the_block(report, cwd),
    );

    // ── Step 4: restore raw mode and alternate screen ─────────────────────────
    // Always restore regardless of how the command ended, so the TUI isn't left
    // in a broken state. Log restoration failures rather than losing the report.
    if let Err(e) = terminal::enable_raw_mode() {
        tracing::error!("failed to re-enable raw mode after subprocess: {e}");
    }
    if let Err(e) = execute!(stdout(), EnterAlternateScreen) {
        tracing::error!("failed to re-enter alternate screen after subprocess: {e}");
    }

    Ok(report)
}

/// Runs the command until something other than `r` ends the loop, returning the
/// report of the last run.
///
/// `shell_pause` decides whether there is a block to answer: with `on_error`, a
/// command that succeeds goes straight back to Trail with no chance to press `r`.
/// A command that could not be spawned counts as a failure, so `on_error` pauses
/// for it too.
///
/// Both halves are passed in so this decision can be tested without a terminal.
fn run_until_answered(
    pause: ShellPause,
    mut run: impl FnMut() -> RunReport,
    mut ask: impl FnMut(&RunReport) -> BlockAnswer,
) -> RunReport {
    loop {
        let report = run();
        if !pause.applies(report.succeeded()) || ask(&report) != BlockAnswer::RunAgain {
            return report;
        }
    }
}

/// Spawns `argv` in `cwd` with inherited stdio and waits for it to exit.
fn run_once(argv: &[&str], cwd: &Path, label: Option<&str>) -> RunReport {
    let started = Instant::now();
    let result = match Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .spawn()
        .and_then(|mut child| child.wait())
    {
        Ok(status) if status.success() => RunResult::Success,
        Ok(status) => RunResult::Failed {
            code: status.code(),
        },
        Err(e) => RunResult::CouldNotRun(e.to_string()),
    };
    RunReport {
        label: label.map(str::to_owned),
        program: argv[0].to_owned(),
        result,
        elapsed: started.elapsed(),
    }
}

/// Prints the result block and waits for it to be answered.
///
/// Called only between the child exiting and the alternate screen coming back,
/// so stdout is the user's normal screen — the one window where Trail may write
/// to it.
///
/// Reading one key needs raw mode, which is switched off again before this
/// returns, because `r` hands the terminal straight to the command. What was
/// typed while the command ran is thrown away first: otherwise an `r` typed
/// during a long command would be read the moment it finished and repeat
/// something like `!del old.log`, and an early Enter would dismiss the result
/// before it was seen.
///
/// A closed or redirected stdin means "do not wait", and so does any failure to
/// read: Trail must not refuse to come back because it could not ask a question.
fn ask_at_the_block(report: &RunReport, cwd: &Path) -> BlockAnswer {
    let interactive = std::io::stdin().is_terminal() && terminal::enable_raw_mode().is_ok();
    if interactive {
        discard_pending_input();
    }

    let columns = terminal::size().map_or(80, |(columns, _)| columns);
    let block = ResultBlock::new(report, cwd, columns);
    if let Err(e) = print_block(&block) {
        tracing::debug!("could not print the result block: {e}");
    }

    let answer = if interactive {
        wait_for_answer()
    } else {
        BlockAnswer::Return
    };
    if interactive {
        if let Err(e) = terminal::disable_raw_mode() {
            tracing::debug!("could not leave raw mode at the result block: {e}");
        }
    }
    // The blank line that keeps the next command's output off the closing rule.
    let mut out = stdout();
    let _ = out.write_all(b"\r\n");
    let _ = out.flush();
    answer
}

/// Drops every input event already waiting.
fn discard_pending_input() {
    while matches!(event::poll(Duration::ZERO), Ok(true)) {
        if event::read().is_err() {
            return;
        }
    }
}

/// Blocks until a key answers the block; a failure to read answers "go back".
fn wait_for_answer() -> BlockAnswer {
    loop {
        match event::read() {
            Ok(Event::Key(key)) => match answer_for(&key) {
                BlockAnswer::Ignore => {}
                answer => return answer,
            },
            Ok(_) => {}
            Err(e) => {
                tracing::debug!("could not read a key at the result block: {e}");
                return BlockAnswer::Return;
            }
        }
    }
}

/// Writes the block, with the cursor left on the line below it.
///
/// Lines end in `\r\n` because raw mode may be on, where `\n` alone does not
/// return to the first column. The leading one ends the command's output, whether
/// or not it ended in a newline of its own.
fn print_block(block: &ResultBlock) -> std::io::Result<()> {
    let colour = if block.succeeded {
        Color::Green
    } else {
        Color::Red
    };
    let mut out = stdout();
    queue!(
        out,
        Print("\r\n"),
        Print(&block.header_start),
        SetForegroundColor(colour),
        Print(&block.outcome),
        ResetColor,
        Print(&block.header_end),
        Print("\r\n"),
    )?;
    for line in block.lines().iter().skip(1) {
        queue!(out, Print(line), Print("\r\n"))?;
    }
    out.flush()
}

/// Splits a `[general] shell` spec into the argv prefix it stands for.
///
/// The spec is a program name followed by the flags that make that program read
/// a command string, for example `pwsh -NoProfile -Command`. Tokens are
/// separated by whitespace; a double-quoted run is one token, which is how a
/// program path containing spaces is written. Backslashes are literal, so a
/// Windows path needs no doubling here — though TOML itself still requires a
/// literal string (`'...'`) or doubled backslashes to deliver one.
///
/// Returns `None` when the spec cannot be used as an argv prefix: it is blank,
/// it leaves a quote unterminated, or its first token — the program to spawn —
/// is empty. [`shell_argv`] reads `None` as "fall back to
/// [`DEFAULT_SHELL_ARGV`]", and `TrailConfig::validate` rejects a non-blank
/// spec that returns `None` so the user hears about a typo at startup.
///
/// ```
/// use trail::actions::shell_exec::split_shell_spec;
///
/// assert_eq!(
///     split_shell_spec("pwsh -NoProfile -Command"),
///     Some(vec!["pwsh".to_owned(), "-NoProfile".to_owned(), "-Command".to_owned()])
/// );
/// assert_eq!(
///     split_shell_spec("\"C:/Program Files/PowerShell/7/pwsh.exe\" -Command"),
///     Some(vec![
///         "C:/Program Files/PowerShell/7/pwsh.exe".to_owned(),
///         "-Command".to_owned(),
///     ])
/// );
/// assert_eq!(split_shell_spec("   "), None);
/// assert_eq!(split_shell_spec("\"unterminated -c"), None);
/// ```
#[must_use]
pub fn split_shell_spec(spec: &str) -> Option<Vec<String>> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut in_quotes = false;

    for ch in spec.chars() {
        match ch {
            // A quote delimits a token without contributing a character, and
            // opening one starts a token even if it turns out to be empty.
            '"' => {
                in_quotes = !in_quotes;
                started = true;
            }
            ch if ch.is_whitespace() && !in_quotes => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            ch => {
                current.push(ch);
                started = true;
            }
        }
    }

    if in_quotes {
        return None;
    }
    if started {
        tokens.push(current);
    }

    match tokens.first() {
        Some(program) if !program.is_empty() => Some(tokens),
        _ => None,
    }
}

/// Builds the argv that runs `command` through the shell named by `spec`.
///
/// `spec` is the `[general] shell` value; a blank or unusable one falls back to
/// [`DEFAULT_SHELL_ARGV`], so this never fails. `command` is appended as a
/// single final argument — it is the shell's job to parse it, which is what
/// makes pipelines, quoting and builtins work.
///
/// ```
/// use trail::actions::shell_exec::shell_argv;
///
/// assert_eq!(
///     shell_argv("pwsh -Command", "./gradlew runClient"),
///     vec!["pwsh".to_owned(), "-Command".to_owned(), "./gradlew runClient".to_owned()]
/// );
/// ```
#[must_use]
pub fn shell_argv(spec: &str, command: &str) -> Vec<String> {
    let mut argv = split_shell_spec(spec).unwrap_or_else(|| {
        DEFAULT_SHELL_ARGV
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect()
    });
    argv.push(command.to_owned());
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_spec_falls_back_to_the_platform_default() {
        for blank in ["", "   ", "\t"] {
            let argv = shell_argv(blank, "echo hi");
            let expected: Vec<String> = DEFAULT_SHELL_ARGV
                .iter()
                .map(|arg| (*arg).to_owned())
                .chain(std::iter::once("echo hi".to_owned()))
                .collect();
            assert_eq!(argv, expected, "{blank:?} should use the default shell");
        }
    }

    #[test]
    fn the_command_stays_one_argument() {
        // The whole point of routing through a shell: the command keeps its
        // pipes and quotes instead of being split into argv here.
        let argv = shell_argv("sh -c", "ls -la | grep foo && echo \"done\"");
        assert_eq!(argv.len(), 3);
        assert_eq!(argv[2], "ls -la | grep foo && echo \"done\"");
    }

    #[test]
    fn a_quoted_program_path_keeps_its_spaces() {
        let argv = split_shell_spec("\"/usr/local/my shell/sh\" -c").unwrap();
        assert_eq!(argv, vec!["/usr/local/my shell/sh", "-c"]);
    }

    #[test]
    fn runs_of_whitespace_do_not_produce_empty_tokens() {
        let argv = split_shell_spec("  pwsh   -NoProfile  -Command  ").unwrap();
        assert_eq!(argv, vec!["pwsh", "-NoProfile", "-Command"]);
    }

    #[test]
    fn a_program_name_alone_is_a_valid_spec() {
        assert_eq!(split_shell_spec("bash"), Some(vec!["bash".to_owned()]));
    }

    #[test]
    fn unusable_specs_are_rejected() {
        // Unterminated quote, and an explicitly empty program name.
        for bad in ["\"pwsh -Command", "\"\" -c", "\"\""] {
            assert_eq!(split_shell_spec(bad), None, "{bad:?} should be rejected");
        }
    }

    #[test]
    fn the_argv_actually_spawns_and_the_shell_parses_the_command() {
        // The vector-shape tests above would all still pass if the argv were
        // ordered wrongly, so spawn one for real. The default shell is used
        // because it is the only one guaranteed to exist on every CI target,
        // and `echo` is a builtin in both cmd.exe and sh.
        let argv = shell_argv("", "echo trail-ok");
        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .expect("the default shell must be spawnable");

        assert!(
            output.status.success(),
            "shell exited with {:?}",
            output.status
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("trail-ok"),
            "the shell did not interpret the command; stdout was {stdout:?}"
        );
    }

    #[test]
    fn pause_policies_parse_from_what_the_config_can_say() {
        assert_eq!(ShellPause::parse("always"), Some(ShellPause::Always));
        assert_eq!(ShellPause::parse(" ALWAYS "), Some(ShellPause::Always));
        assert_eq!(ShellPause::parse("on_error"), Some(ShellPause::OnError));
        assert_eq!(ShellPause::parse("on-error"), Some(ShellPause::OnError));
        assert_eq!(ShellPause::parse("never"), Some(ShellPause::Never));
        assert_eq!(ShellPause::parse("off"), Some(ShellPause::Never));
        assert_eq!(ShellPause::parse("sometimes"), None);
        assert_eq!(ShellPause::parse(""), None);
    }

    #[test]
    fn on_error_waits_only_for_a_command_that_failed() {
        assert!(ShellPause::Always.applies(true));
        assert!(ShellPause::Always.applies(false));
        assert!(!ShellPause::OnError.applies(true));
        assert!(ShellPause::OnError.applies(false));
        assert!(!ShellPause::Never.applies(true));
        assert!(!ShellPause::Never.applies(false));
    }

    fn ran(result: RunResult) -> RunReport {
        RunReport {
            label: Some("!x".to_owned()),
            program: "sh".to_owned(),
            result,
            elapsed: Duration::ZERO,
        }
    }

    /// How many runs and how many questions a policy leads to, given the
    /// results the runs produce and the answers the block receives.
    fn drive(
        pause: ShellPause,
        results: Vec<RunResult>,
        answers: Vec<BlockAnswer>,
    ) -> (usize, usize, RunReport) {
        let (mut runs, mut asks) = (0, 0);
        let mut results = results.into_iter();
        let mut answers = answers.into_iter();
        let report = run_until_answered(
            pause,
            || {
                runs += 1;
                ran(results.next().expect("no more runs were expected"))
            },
            |_| {
                asks += 1;
                answers.next().expect("no more questions were expected")
            },
        );
        (runs, asks, report)
    }

    #[test]
    fn r_runs_again_and_the_last_run_is_the_one_reported() {
        let (runs, asks, report) = drive(
            ShellPause::Always,
            vec![RunResult::Failed { code: Some(1) }, RunResult::Success],
            vec![BlockAnswer::RunAgain, BlockAnswer::Return],
        );
        assert_eq!((runs, asks), (2, 2));
        assert_eq!(report.result, RunResult::Success);
    }

    #[test]
    fn on_error_never_asks_about_a_success() {
        let (runs, asks, _) = drive(ShellPause::OnError, vec![RunResult::Success], vec![]);
        assert_eq!((runs, asks), (1, 0));
    }

    /// `r` can make a failing command succeed, and then there is no block left
    /// to answer under `on_error`: it goes straight back to Trail.
    #[test]
    fn on_error_stops_asking_once_a_rerun_succeeds() {
        let (runs, asks, report) = drive(
            ShellPause::OnError,
            vec![RunResult::Failed { code: Some(1) }, RunResult::Success],
            vec![BlockAnswer::RunAgain],
        );
        assert_eq!((runs, asks), (2, 1));
        assert!(report.succeeded());
    }

    #[test]
    fn on_error_asks_about_a_command_that_could_not_start() {
        let (_, asks, _) = drive(
            ShellPause::OnError,
            vec![RunResult::CouldNotRun("no such program".to_owned())],
            vec![BlockAnswer::Return],
        );
        assert_eq!(asks, 1);
    }

    /// Under `never` there is no block, so the report is all there is, and it
    /// has to say the command failed.
    #[test]
    fn never_does_not_ask_but_still_reports_a_failure() {
        let (runs, asks, report) = drive(
            ShellPause::Never,
            vec![RunResult::Failed { code: Some(2) }],
            vec![],
        );
        assert_eq!((runs, asks), (1, 0));
        assert!(report.status_line().unwrap().contains("failed: exit 2"));
    }

    #[test]
    fn a_real_exit_code_reaches_the_report() {
        let cwd = std::env::current_dir().unwrap();
        let argv = shell_argv("", "exit 3");
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();

        let report = run_once(&argv, &cwd, Some("!exit 3"));

        assert_eq!(report.result, RunResult::Failed { code: Some(3) });
    }

    #[test]
    fn a_program_that_cannot_start_is_reported_not_returned_as_an_error() {
        let cwd = std::env::current_dir().unwrap();
        let report = run_once(&["trail-no-such-program"], &cwd, None);
        assert!(
            matches!(report.result, RunResult::CouldNotRun(_)),
            "{report:?}"
        );
        assert!(report.status_line().is_some(), "a failure is always said");
    }

    #[test]
    fn the_default_spec_is_itself_a_usable_argv() {
        // Guards against a typo in the const leaving an empty program name.
        assert!(!DEFAULT_SHELL_ARGV.is_empty());
        assert!(!DEFAULT_SHELL_ARGV[0].is_empty());
    }
}
