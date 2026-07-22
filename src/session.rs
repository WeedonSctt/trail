//! Session management: writes `--cwd-file` on normal exit.
//!
//! On normal exit, writes `state.cwd` to the path given by `--cwd-file`.
//! On cancellation (`Esc`-driven quit or `Ctrl-c`), writes nothing, so the
//! shell falls back to its original directory.
// TODO(phase-6): Implement cwd-file writing on normal exit.
