//! Suspend/resume subprocess execution.
//!
//! Implements the suspend/resume sequence from the architecture doc:
//! leave alternate screen → spawn subprocess → wait → re-enter raw mode.
//! Used for both editor-open and Command Mode's `!<shell command>`.
// TODO(phase-6): Implement run_external() with terminal suspend/resume.
