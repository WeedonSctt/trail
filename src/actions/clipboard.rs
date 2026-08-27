//! Clipboard operations: copy absolute path, relative path, and filename.
//!
//! Bound to `ya` (absolute path), `yr` (relative path), `yn` (filename name)
//! in Navigation Mode.
//!
//! # Platform strategy
//!
//! Trail owns the alternate screen, so writing to stdout would corrupt the UI.
//! This module writes path strings to the OS clipboard via the `arboard`
//! crate, which selects the platform's native mechanism (X11/Wayland on Linux,
//! AppKit on macOS, Win32 on Windows).
//!
//! Each operation is split into a pure `*_text` function that computes the
//! string and a `copy_*` wrapper that performs the clipboard write. The split
//! keeps the path arithmetic unit-testable on headless machines, where no
//! display server is available for `arboard` to talk to.
//!
//! The yanked string is also recorded in `AppState::last_yank` so the status
//! bar and tests can observe it, and logged at `info` level to the log file.

use std::path::Path;

use thiserror::Error;

/// Errors from clipboard operations.
#[derive(Debug, Error)]
pub enum ClipboardError {
    /// The source path could not be represented as a UTF-8 string.
    #[error("path is not valid UTF-8")]
    NotUtf8,
    /// The arboard crate failed to access the OS clipboard.
    #[error("clipboard error: {0}")]
    Arboard(#[from] arboard::Error),
}

// ── Pure path computation ─────────────────────────────────────────────────────
//
// The three `*_text` functions below compute the string to be yanked and do
// no I/O whatsoever.  Keeping them separate from the clipboard write is what
// makes them unit-testable: `arboard` needs a live display server (X11 /
// Wayland / Win32 / AppKit), which headless CI does not have, so any test that
// reached the clipboard would fail on ambient machine state — exactly what the
// coding standard (§7) forbids.

/// Returns the absolute path of `entry_path` as a string.
///
/// Pure: performs no clipboard or filesystem access.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the path cannot be UTF-8 encoded.
pub fn absolute_path_text(entry_path: &Path) -> Result<String, ClipboardError> {
    Ok(entry_path
        .to_str()
        .ok_or(ClipboardError::NotUtf8)?
        .to_owned())
}

/// Returns the path of `entry_path` relative to `cwd` as a string.
///
/// Falls back to the absolute path when `entry_path` is not under `cwd`.
/// Pure: performs no clipboard or filesystem access.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the resulting path is not valid UTF-8.
pub fn relative_path_text(entry_path: &Path, cwd: &Path) -> Result<String, ClipboardError> {
    // Attempt to strip `cwd` prefix; fall back to the absolute path on failure.
    let rel = entry_path.strip_prefix(cwd).unwrap_or(entry_path);
    Ok(rel.to_str().ok_or(ClipboardError::NotUtf8)?.to_owned())
}

/// Returns only the file-name component of `entry_path` as a string.
///
/// Pure: performs no clipboard or filesystem access.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the file name is missing or is not
/// valid UTF-8.
pub fn filename_text(entry_path: &Path) -> Result<String, ClipboardError> {
    Ok(entry_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(ClipboardError::NotUtf8)?
        .to_owned())
}

// ── Clipboard writes ──────────────────────────────────────────────────────────

/// Writes `text` to the OS clipboard.
///
/// The single place in this module that talks to `arboard`, so the pure
/// path-computation functions above stay free of I/O.
///
/// # Errors
///
/// Returns [`ClipboardError::Arboard`] if the OS clipboard cannot be opened
/// or written to (e.g. no display server available).
fn set_clipboard(text: &str) -> Result<(), ClipboardError> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(text.to_owned())?;
    Ok(())
}

/// Copies the absolute path of `entry_path` to the OS clipboard.
///
/// Returns the string that was yanked so the caller can store it in state.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the path cannot be UTF-8 encoded.
/// Returns [`ClipboardError::Arboard`] if the OS clipboard write fails.
pub fn copy_absolute_path(entry_path: &Path) -> Result<String, ClipboardError> {
    let s = absolute_path_text(entry_path)?;
    set_clipboard(&s)?;

    tracing::info!(yank = %s, "yanked absolute path");
    Ok(s)
}

/// Copies the path of `entry_path` relative to `cwd` to the OS clipboard.
///
/// Falls back to the absolute path if a relative path cannot be computed.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the resulting path is not valid UTF-8.
/// Returns [`ClipboardError::Arboard`] if the OS clipboard write fails.
pub fn copy_relative_path(entry_path: &Path, cwd: &Path) -> Result<String, ClipboardError> {
    let s = relative_path_text(entry_path, cwd)?;
    set_clipboard(&s)?;

    tracing::info!(yank = %s, "yanked relative path");
    Ok(s)
}

/// Copies only the file name component of `entry_path` to the OS clipboard.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the file name is not valid UTF-8.
/// Returns [`ClipboardError::Arboard`] if the OS clipboard write fails.
pub fn copy_filename(entry_path: &Path) -> Result<String, ClipboardError> {
    let s = filename_text(entry_path)?;
    set_clipboard(&s)?;

    tracing::info!(yank = %s, "yanked filename");
    Ok(s)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // These exercise the pure `*_text` helpers rather than the `copy_*`
    // wrappers: the assertions are about path arithmetic, and reaching the OS
    // clipboard would make them fail on any headless machine (CI included).

    #[test]
    fn absolute_path_returns_full_path() {
        let p = PathBuf::from("/home/user/project/main.rs");
        let s = absolute_path_text(&p).unwrap();
        assert_eq!(s, "/home/user/project/main.rs");
    }

    #[test]
    fn relative_path_strips_cwd() {
        let cwd = PathBuf::from("/home/user/project");
        let entry = PathBuf::from("/home/user/project/src/main.rs");
        let s = relative_path_text(&entry, &cwd).unwrap();
        assert_eq!(s, "src/main.rs");
    }

    #[test]
    fn relative_path_falls_back_to_absolute_when_not_under_cwd() {
        let cwd = PathBuf::from("/other/dir");
        let entry = PathBuf::from("/home/user/project/main.rs");
        let s = relative_path_text(&entry, &cwd).unwrap();
        // strip_prefix fails → falls back to the full path.
        assert_eq!(s, "/home/user/project/main.rs");
    }

    #[test]
    fn filename_returns_only_file_name() {
        let p = PathBuf::from("/home/user/project/main.rs");
        let s = filename_text(&p).unwrap();
        assert_eq!(s, "main.rs");
    }

    #[test]
    fn filename_of_root_path_is_not_utf8_error() {
        // A path with no file-name component (root) has nothing to yank.
        let p = PathBuf::from("/");
        assert!(matches!(filename_text(&p), Err(ClipboardError::NotUtf8)));
    }
}
