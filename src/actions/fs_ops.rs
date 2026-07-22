//! Filesystem mutation operations: rename, move, duplicate, delete, create.
//!
//! Higher-risk module — destructive operations always go through the
//! confirmation flow the spec requires. All functions operate relative to a
//! given `cwd` and return typed `FsError` values; they never unwrap or panic.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors from filesystem mutation operations.
#[derive(Debug, Error)]
pub enum FsError {
    /// A required source path does not exist or cannot be accessed.
    #[error("source path not found: {0}")]
    SourceNotFound(PathBuf),
    /// The destination path already exists and would be overwritten.
    #[error("destination already exists: {0}")]
    DestExists(PathBuf),
    /// The name contains invalid characters (e.g. path separators in a rename).
    #[error("invalid name '{0}': {1}")]
    InvalidName(String, &'static str),
    /// An underlying I/O error.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

/// Convenience constructor for [`FsError::Io`].
fn io_err(context: impl Into<String>, source: std::io::Error) -> FsError {
    FsError::Io {
        context: context.into(),
        source,
    }
}

// ── Create operations ─────────────────────────────────────────────────────────

/// Creates a new directory named `name` inside `cwd`.
///
/// # Errors
///
/// Returns [`FsError::InvalidName`] if `name` contains a path separator.
/// Returns [`FsError::DestExists`] if the directory already exists.
/// Returns [`FsError::Io`] for other filesystem errors.
pub fn mkdir(cwd: &Path, name: &str) -> Result<PathBuf, FsError> {
    validate_simple_name(name)?;
    let target = cwd.join(name);
    if target.exists() {
        return Err(FsError::DestExists(target));
    }
    fs::create_dir(&target).map_err(|e| io_err(format!("mkdir {name}"), e))?;
    Ok(target)
}

/// Creates an empty file named `name` inside `cwd`.
///
/// # Errors
///
/// Returns [`FsError::InvalidName`] if `name` contains a path separator.
/// Returns [`FsError::DestExists`] if the file already exists.
/// Returns [`FsError::Io`] for other filesystem errors.
pub fn touch(cwd: &Path, name: &str) -> Result<PathBuf, FsError> {
    validate_simple_name(name)?;
    let target = cwd.join(name);
    if target.exists() {
        return Err(FsError::DestExists(target));
    }
    fs::write(&target, b"").map_err(|e| io_err(format!("touch {name}"), e))?;
    Ok(target)
}

// ── Rename / move / copy ──────────────────────────────────────────────────────

/// Renames `source` to `new_name` within the same directory.
///
/// `new_name` must be a plain filename (no path separators). The rename is
/// atomic on most platforms (POSIX `rename(2)`).
///
/// # Errors
///
/// Returns [`FsError::SourceNotFound`] if `source` does not exist.
/// Returns [`FsError::InvalidName`] if `new_name` contains path separators.
/// Returns [`FsError::DestExists`] if the target name is already in use.
/// Returns [`FsError::Io`] for other filesystem errors.
pub fn rename(source: &Path, new_name: &str) -> Result<PathBuf, FsError> {
    if !source.exists() {
        return Err(FsError::SourceNotFound(source.to_owned()));
    }
    validate_simple_name(new_name)?;
    let dest = source.parent().unwrap_or(Path::new(".")).join(new_name);
    if dest.exists() {
        return Err(FsError::DestExists(dest));
    }
    fs::rename(source, &dest).map_err(|e| io_err(format!("rename → {new_name}"), e))?;
    Ok(dest)
}

/// Moves `source` to `dest`.
///
/// `dest` may be absolute or relative to `cwd`. If `dest` is an existing
/// directory, `source` is moved inside it.
///
/// # Errors
///
/// Returns [`FsError::SourceNotFound`] if `source` does not exist.
/// Returns [`FsError::DestExists`] if the resulting path is occupied.
/// Returns [`FsError::Io`] for other filesystem errors.
pub fn mv(source: &Path, dest_str: &str, cwd: &Path) -> Result<PathBuf, FsError> {
    if !source.exists() {
        return Err(FsError::SourceNotFound(source.to_owned()));
    }
    let raw_dest = resolve_dest(dest_str, cwd);
    let dest = if raw_dest.is_dir() {
        // Moving into an existing directory: keep the source filename.
        raw_dest.join(source.file_name().unwrap_or_default())
    } else {
        raw_dest
    };
    if dest.exists() {
        return Err(FsError::DestExists(dest));
    }
    fs::rename(source, &dest).map_err(|e| io_err(format!("mv → {dest_str}"), e))?;
    Ok(dest)
}

/// Copies `source` to `dest`.
///
/// `dest` may be absolute or relative to `cwd`. If `dest` is an existing
/// directory, the source is copied inside it. Directories are copied
/// recursively.
///
/// # Errors
///
/// Returns [`FsError::SourceNotFound`] if `source` does not exist.
/// Returns [`FsError::DestExists`] if the resulting path is occupied.
/// Returns [`FsError::Io`] for other filesystem errors.
pub fn cp(source: &Path, dest_str: &str, cwd: &Path) -> Result<PathBuf, FsError> {
    if !source.exists() {
        return Err(FsError::SourceNotFound(source.to_owned()));
    }
    let raw_dest = resolve_dest(dest_str, cwd);
    let dest = if raw_dest.is_dir() {
        raw_dest.join(source.file_name().unwrap_or_default())
    } else {
        raw_dest
    };
    if dest.exists() {
        return Err(FsError::DestExists(dest));
    }
    if source.is_dir() {
        copy_dir_recursive(source, &dest)?;
    } else {
        fs::copy(source, &dest).map_err(|e| io_err(format!("cp → {dest_str}"), e))?;
    }
    Ok(dest)
}

/// Recursively copies directory `src` to `dst`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), FsError> {
    fs::create_dir(dst).map_err(|e| io_err(format!("mkdir {}", dst.display()), e))?;
    for entry in fs::read_dir(src).map_err(|e| io_err(format!("readdir {}", src.display()), e))? {
        let entry = entry.map_err(|e| io_err("dir entry", e))?;
        let ty = entry.file_type().map_err(|e| io_err("file_type", e))?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)
                .map_err(|e| io_err(format!("cp {}", entry.path().display()), e))?;
        }
    }
    Ok(())
}

// ── Delete ────────────────────────────────────────────────────────────────────

/// Deletes `target` — file or directory (recursively).
///
/// This is a destructive, irreversible operation. The caller is responsible
/// for showing the confirmation prompt before invoking this function; the
/// confirmation flow lives in `actions/mod.rs` and `app/state.rs`.
///
/// # Errors
///
/// Returns [`FsError::SourceNotFound`] if `target` does not exist.
/// Returns [`FsError::Io`] for other filesystem errors.
pub fn delete(target: &Path) -> Result<(), FsError> {
    if !target.exists() {
        return Err(FsError::SourceNotFound(target.to_owned()));
    }
    if target.is_dir() {
        fs::remove_dir_all(target)
            .map_err(|e| io_err(format!("delete dir {}", target.display()), e))?;
    } else {
        fs::remove_file(target)
            .map_err(|e| io_err(format!("delete file {}", target.display()), e))?;
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Validates that `name` is a simple, non-path component (no separators).
fn validate_simple_name(name: &str) -> Result<(), FsError> {
    if name.is_empty() {
        return Err(FsError::InvalidName(
            name.to_owned(),
            "name must not be empty",
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(FsError::InvalidName(
            name.to_owned(),
            "name must not contain path separators",
        ));
    }
    Ok(())
}

/// Resolves a destination string to an absolute path, treating relative paths
/// as relative to `cwd`.
fn resolve_dest(dest_str: &str, cwd: &Path) -> PathBuf {
    let p = PathBuf::from(dest_str);
    if p.is_absolute() {
        p
    } else {
        cwd.join(p)
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // ── mkdir ──────────────────────────────────────────────────────────────────

    #[test]
    fn mkdir_creates_directory() {
        let dir = tmp();
        let created = mkdir(dir.path(), "new_dir").unwrap();
        assert!(created.is_dir());
    }

    #[test]
    fn mkdir_rejects_path_separator() {
        let dir = tmp();
        let err = mkdir(dir.path(), "foo/bar").unwrap_err();
        assert!(matches!(err, FsError::InvalidName(..)));
    }

    #[test]
    fn mkdir_fails_if_exists() {
        let dir = tmp();
        fs::create_dir(dir.path().join("exists")).unwrap();
        let err = mkdir(dir.path(), "exists").unwrap_err();
        assert!(matches!(err, FsError::DestExists(_)));
    }

    // ── touch ─────────────────────────────────────────────────────────────────

    #[test]
    fn touch_creates_empty_file() {
        let dir = tmp();
        let created = touch(dir.path(), "new.txt").unwrap();
        assert!(created.is_file());
        assert_eq!(fs::read(created).unwrap(), b"");
    }

    #[test]
    fn touch_rejects_path_separator() {
        let dir = tmp();
        let err = touch(dir.path(), "foo/bar.txt").unwrap_err();
        assert!(matches!(err, FsError::InvalidName(..)));
    }

    // ── rename ────────────────────────────────────────────────────────────────

    #[test]
    fn rename_renames_file() {
        let dir = tmp();
        let src = dir.path().join("old.txt");
        fs::write(&src, b"content").unwrap();
        let dest = rename(&src, "new.txt").unwrap();
        assert!(!src.exists());
        assert!(dest.is_file());
    }

    #[test]
    fn rename_rejects_slash_in_name() {
        let dir = tmp();
        let src = dir.path().join("old.txt");
        fs::write(&src, b"").unwrap();
        let err = rename(&src, "new/name.txt").unwrap_err();
        assert!(matches!(err, FsError::InvalidName(..)));
    }

    #[test]
    fn rename_fails_if_source_missing() {
        let dir = tmp();
        let err = rename(&dir.path().join("missing.txt"), "new.txt").unwrap_err();
        assert!(matches!(err, FsError::SourceNotFound(_)));
    }

    // ── mv ────────────────────────────────────────────────────────────────────

    #[test]
    fn mv_moves_file() {
        let dir = tmp();
        let src = dir.path().join("src.txt");
        fs::write(&src, b"hello").unwrap();
        let dest = mv(&src, "dest.txt", dir.path()).unwrap();
        assert!(!src.exists());
        assert!(dest.is_file());
    }

    #[test]
    fn mv_into_existing_directory() {
        let dir = tmp();
        let src = dir.path().join("file.txt");
        fs::write(&src, b"").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let dest = mv(&src, "sub", dir.path()).unwrap();
        assert_eq!(dest, sub.join("file.txt"));
    }

    // ── cp ────────────────────────────────────────────────────────────────────

    #[test]
    fn cp_copies_file() {
        let dir = tmp();
        let src = dir.path().join("src.txt");
        fs::write(&src, b"data").unwrap();
        let dest = cp(&src, "copy.txt", dir.path()).unwrap();
        assert!(src.exists());
        assert!(dest.is_file());
        assert_eq!(fs::read(dest).unwrap(), b"data");
    }

    #[test]
    fn cp_copies_directory_recursively() {
        let dir = tmp();
        let src = dir.path().join("srcdir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file.txt"), b"hi").unwrap();
        let dest = cp(&src, "dstdir", dir.path()).unwrap();
        assert!(dest.join("file.txt").exists());
    }

    // ── delete ────────────────────────────────────────────────────────────────

    #[test]
    fn delete_removes_file() {
        let dir = tmp();
        let f = dir.path().join("to_delete.txt");
        fs::write(&f, b"").unwrap();
        delete(&f).unwrap();
        assert!(!f.exists());
    }

    #[test]
    fn delete_removes_directory_recursively() {
        let dir = tmp();
        let d = dir.path().join("to_delete");
        fs::create_dir(&d).unwrap();
        fs::write(d.join("child.txt"), b"").unwrap();
        delete(&d).unwrap();
        assert!(!d.exists());
    }

    #[test]
    fn delete_fails_if_missing() {
        let dir = tmp();
        let err = delete(&dir.path().join("nope")).unwrap_err();
        assert!(matches!(err, FsError::SourceNotFound(_)));
    }
}
