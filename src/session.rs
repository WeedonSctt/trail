//! Session management: writes `--cwd-file` on normal exit.
//!
//! A subprocess cannot change its parent shell's working directory. Trail's
//! "shell continues in the directory currently displayed" behaviour requires a
//! shell-side wrapper function that reads the file Trail writes here and calls
//! `cd` on its contents.
//!
//! On **normal** exit (`q` / `Quit` action) Trail calls [`write_cwd_file`] to
//! record `state.cwd` in the path supplied by `--cwd-file`. On **cancellation**
//! (`Ctrl-c` / `Esc`-driven quit) the function is not called, so the file is
//! never written and the shell wrapper falls back to the original directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Writes `cwd` as a UTF-8 string to `cwd_file_path`.
///
/// Called on normal exit (the `Quit` action) when `--cwd-file` was supplied on
/// the command line. The shell wrapper reads this file after Trail exits and
/// calls `cd` if the file exists and contains a valid directory path.
///
/// On cancellation (`Ctrl-c` / forced kill) this function is **not** called,
/// so no file is written and the shell wrapper leaves the user in their
/// original directory.
///
/// # Errors
///
/// Returns an [`io::Error`] if the file cannot be created or written. The
/// caller logs the error at `debug` level and continues with a normal exit —
/// a failed write here is inconvenient but not catastrophic.
pub fn write_cwd_file(cwd: &Path, cwd_file_path: &Path) -> io::Result<()> {
    let cwd_str = cwd
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "cwd is not valid UTF-8"))?;

    #[cfg(windows)]
    {
        if let Some(stripped) = cwd_str.strip_prefix(r"\\?\UNC\") {
            let unc_path = format!(r"\\{}", stripped);
            return fs::write(cwd_file_path, unc_path);
        } else if let Some(stripped) = cwd_str.strip_prefix(r"\\?\") {
            return fs::write(cwd_file_path, stripped);
        }
    }

    fs::write(cwd_file_path, cwd_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn write_cwd_file_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("cwd.txt");
        let cwd = PathBuf::from("/some/test/path");
        write_cwd_file(&cwd, &out).expect("write_cwd_file");
        let contents = std::fs::read_to_string(&out).expect("read");
        assert_eq!(contents, "/some/test/path");
    }

    #[test]
    fn write_cwd_file_creates_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("new_file.txt");
        assert!(!out.exists());
        let cwd = PathBuf::from("/tmp");
        write_cwd_file(&cwd, &out).expect("write");
        assert!(out.exists());
    }

    #[test]
    #[cfg(windows)]
    fn write_cwd_file_strips_windows_verbatim_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("cwd.txt");
        let cwd = PathBuf::from(r"\\?\C:\Windows\System32");
        write_cwd_file(&cwd, &out).expect("write_cwd_file");
        let contents = std::fs::read_to_string(&out).expect("read");
        assert_eq!(contents, r"C:\Windows\System32");
    }

    #[test]
    #[cfg(windows)]
    fn write_cwd_file_strips_windows_verbatim_unc_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("cwd.txt");
        let cwd = PathBuf::from(r"\\?\UNC\server\share");
        write_cwd_file(&cwd, &out).expect("write_cwd_file");
        let contents = std::fs::read_to_string(&out).expect("read");
        assert_eq!(contents, r"\\server\share");
    }
}

use serde::{Deserialize, Serialize};

/// Maximum number of recent directories to keep.
const MAX_RECENT_DIRS: usize = 50;

/// Persisted recent directories store.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RecentDirs {
    /// List of paths, most recent first.
    pub paths: Vec<PathBuf>,
}

impl RecentDirs {
    /// Loads the recent directories from `file_path`.
    pub fn load(file_path: &Path) -> Self {
        if let Ok(content) = fs::read_to_string(file_path) {
            if let Ok(recent) = toml::from_str(&content) {
                return recent;
            }
        }
        Self::default()
    }

    /// Saves the recent directories to `file_path`.
    pub fn save(&self, file_path: &Path) {
        if let Ok(content) = toml::to_string_pretty(self) {
            let _ = fs::write(file_path, content);
        }
    }

    /// Records a new visit to `path`, bringing it to the top.
    pub fn visit(&mut self, path: PathBuf) {
        self.paths.retain(|p| p != &path);
        self.paths.insert(0, path);
        self.paths.truncate(MAX_RECENT_DIRS);
    }
}
