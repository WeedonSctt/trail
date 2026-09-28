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

use std::io::stdout;
use std::path::Path;
use std::process::Command;

use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use thiserror::Error;

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

/// Errors that can arise while running an external process.
#[derive(Debug, Error)]
pub enum ShellExecError {
    /// Terminal state manipulation failed.
    #[error("terminal error: {0}")]
    Terminal(#[source] std::io::Error),
    /// Could not spawn the subprocess.
    #[error("failed to spawn process: {0}")]
    Spawn(#[source] std::io::Error),
    /// Could not wait for the subprocess to finish.
    #[error("failed to wait for process: {0}")]
    Wait(#[source] std::io::Error),
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
/// # Errors
///
/// Returns [`ShellExecError`] if terminal manipulation or process I/O fails.
/// Even on error we make a best-effort attempt to restore terminal state.
pub fn run_external(argv: &[&str], cwd: &Path) -> Result<(), ShellExecError> {
    if argv.is_empty() {
        // Nothing to run.
        return Ok(());
    }

    // ── Step 1: leave alternate screen / cooked mode ──────────────────────────
    // Best-effort: if either of these fails we still attempt to restore later.
    execute!(stdout(), LeaveAlternateScreen).map_err(ShellExecError::Terminal)?;
    terminal::disable_raw_mode().map_err(ShellExecError::Terminal)?;

    // ── Step 2 & 3: spawn and wait ────────────────────────────────────────────
    let spawn_result = Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .spawn();

    let wait_result = match spawn_result {
        Ok(mut child) => child.wait().map(|_| ()).map_err(ShellExecError::Wait),
        Err(e) => Err(ShellExecError::Spawn(e)),
    };

    // ── Step 4: restore raw mode and alternate screen ─────────────────────────
    // Always restore regardless of spawn/wait errors, so the TUI isn't left
    // in a broken state. Log restoration failures but don't override the
    // spawn/wait error.
    if let Err(e) = terminal::enable_raw_mode() {
        tracing::error!("failed to re-enable raw mode after subprocess: {e}");
    }
    if let Err(e) = execute!(stdout(), EnterAlternateScreen) {
        tracing::error!("failed to re-enter alternate screen after subprocess: {e}");
    }

    wait_result
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
    fn the_default_spec_is_itself_a_usable_argv() {
        // Guards against a typo in the const leaving an empty program name.
        assert!(!DEFAULT_SHELL_ARGV.is_empty());
        assert!(!DEFAULT_SHELL_ARGV[0].is_empty());
    }
}
