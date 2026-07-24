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
use std::path::Path;

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
        if cwd_str.starts_with(r"\\?\UNC\") {
            let unc_path = format!(r"\\{}", &cwd_str[8..]);
            return fs::write(cwd_file_path, unc_path);
        } else if cwd_str.starts_with(r"\\?\") {
            return fs::write(cwd_file_path, &cwd_str[4..]);
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
