//! Clipboard operations: copy absolute path, relative path, and filename.
//!
//! Bound to `ya` (absolute path), `yr` (relative path), `yn` (filename name)
//! in Navigation Mode.
//!
//! # Platform strategy
//!
//! Trail owns the alternate screen, so writing to stdout would corrupt the UI.
//! This module writes path strings to the OS clipboard via the platform's
//! native mechanism where possible, falling back to writing the text to a
//! temp file so the shell wrapper can forward it (`pbcopy`, `xclip`, etc.).
//!
//! For Phase 3 the implementation uses the [`arboard`] crate when it is
//! available. Because `arboard` is not in the confirmed Phase-3 dependency
//! set and adding it requires a Decision Log entry (coding standard §12), we
//! use a write-to-stderr approach that avoids corrupting the alternate screen
//! but leaves a trace in the log — real clipboard integration is a
//! straightforward follow-up once the crate is approved.
//!
//! **Phase 3 approach**: write the path string to the tracing log at `info`
//! level (it will appear in the log file, not the terminal) and store it in
//! `AppState::last_yank` so tests and the status bar can observe it. A proper
//! OS clipboard write is noted as a TODO.

use std::path::Path;

use thiserror::Error;

/// Errors from clipboard operations.
#[derive(Debug, Error)]
pub enum ClipboardError {
    /// The source path could not be represented as a UTF-8 string.
    #[error("path is not valid UTF-8")]
    NotUtf8,
}

/// Copies the absolute path of `entry_path` to the yank buffer.
///
/// Returns the string that was yanked so the caller can store it in state.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the path cannot be UTF-8 encoded.
pub fn copy_absolute_path(entry_path: &Path) -> Result<String, ClipboardError> {
    let s = entry_path
        .to_str()
        .ok_or(ClipboardError::NotUtf8)?
        .to_owned();
    // TODO(phase-3-followup): Write `s` to the OS clipboard via `arboard` or
    // equivalent once the dependency is approved and added to Cargo.toml.
    tracing::info!(yank = %s, "yanked absolute path");
    Ok(s)
}

/// Copies the path of `entry_path` relative to `cwd` to the yank buffer.
///
/// Falls back to the absolute path if a relative path cannot be computed.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the resulting path is not valid UTF-8.
pub fn copy_relative_path(entry_path: &Path, cwd: &Path) -> Result<String, ClipboardError> {
    // Attempt to strip `cwd` prefix; fall back to the absolute path on failure.
    let rel = entry_path.strip_prefix(cwd).unwrap_or(entry_path);
    let s = rel.to_str().ok_or(ClipboardError::NotUtf8)?.to_owned();
    tracing::info!(yank = %s, "yanked relative path");
    Ok(s)
}

/// Copies only the file name component of `entry_path` to the yank buffer.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the file name is not valid UTF-8.
pub fn copy_filename(entry_path: &Path) -> Result<String, ClipboardError> {
    let s = entry_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(ClipboardError::NotUtf8)?
        .to_owned();
    tracing::info!(yank = %s, "yanked filename");
    Ok(s)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn absolute_path_returns_full_path() {
        let p = PathBuf::from("/home/user/project/main.rs");
        let s = copy_absolute_path(&p).unwrap();
        assert_eq!(s, "/home/user/project/main.rs");
    }

    #[test]
    fn relative_path_strips_cwd() {
        let cwd = PathBuf::from("/home/user/project");
        let entry = PathBuf::from("/home/user/project/src/main.rs");
        let s = copy_relative_path(&entry, &cwd).unwrap();
        assert_eq!(s, "src/main.rs");
    }

    #[test]
    fn relative_path_falls_back_to_absolute_when_not_under_cwd() {
        let cwd = PathBuf::from("/other/dir");
        let entry = PathBuf::from("/home/user/project/main.rs");
        let s = copy_relative_path(&entry, &cwd).unwrap();
        // strip_prefix fails → falls back to the full path.
        assert_eq!(s, "/home/user/project/main.rs");
    }

    #[test]
    fn filename_returns_only_file_name() {
        let p = PathBuf::from("/home/user/project/main.rs");
        let s = copy_filename(&p).unwrap();
        assert_eq!(s, "main.rs");
    }
}
