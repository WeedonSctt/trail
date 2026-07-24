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

use std::io::stdout;
use std::path::Path;
use std::process::Command;

use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use thiserror::Error;

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

/// Resolves the editor command string from the `$EDITOR` environment variable,
/// falling back to `vi` if `$EDITOR` is unset or empty.
///
/// The returned `String` is a single token (no argument splitting); it is used
/// as `argv[0]` and the file path is passed as `argv[1]`. Complex `$EDITOR`
/// values that include arguments (e.g. `"nano -w"`) are not split — that is a
/// known limitation and is deferred to Phase 7 when the config schema can
/// provide a proper `Vec<String>` editor command.
pub fn resolve_editor() -> String {
    resolve_editor_internal(std::env::var("EDITOR").ok())
}

/// Internal helper for `resolve_editor` to allow testing without unsafe
/// environment variable mutations.
fn resolve_editor_internal(env_var: Option<String>) -> String {
    env_var
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "vi".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_editor_falls_back_to_vi() {
        assert_eq!(resolve_editor_internal(None), "vi");
        assert_eq!(resolve_editor_internal(Some("".to_string())), "vi");
        assert_eq!(resolve_editor_internal(Some("   ".to_string())), "vi");
    }

    #[test]
    fn resolve_editor_uses_env_var() {
        assert_eq!(resolve_editor_internal(Some("nano".to_string())), "nano");
    }
}
