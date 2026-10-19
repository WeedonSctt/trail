//! Filesystem mutation operations: rename, move, duplicate, delete, create.
//!
//! Higher-risk module — destructive operations always go through the
//! confirmation flow the spec requires. All functions operate relative to a
//! given `cwd` and return typed `FsError` values; they never unwrap or panic.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

// ── Error type ────────────────────────────────────────────────────────────────

/// Where a deleted entry goes.
///
/// `dd` used to be `remove_dir_all` after a single `y`, which on a directory is
/// as irreversible as it gets. The recycle bin is the platform's own answer to
/// that, and it already solves the four problems a hand-rolled trash folder
/// would have to: name collisions, moves across devices, unbounded growth, and
/// when to purge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    /// Hand the entry to the platform's recycle bin, from which the user can
    /// restore it. The default.
    Trash,
    /// Unlink it. Nothing to restore.
    Permanent,
}

impl DeleteMode {
    /// Parses a `[general] delete_mode` value.
    ///
    /// Returns `None` for an unrecognised name so the config layer can report it
    /// with the key that carried it.
    pub fn parse(value: &str) -> Option<DeleteMode> {
        match value.trim().to_ascii_lowercase().as_str() {
            "trash" | "recycle" => Some(DeleteMode::Trash),
            "permanent" | "delete" => Some(DeleteMode::Permanent),
            _ => None,
        }
    }
}

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
    /// The platform refused to move the entry to its recycle bin.
    ///
    /// Reported rather than silently falling back to an unlink: a user who asked
    /// for a recoverable delete must not get an unrecoverable one because the
    /// recycle bin was unavailable.
    #[error("could not move {path} to the recycle bin: {source}")]
    Trash {
        /// The entry that was to be trashed.
        path: PathBuf,
        #[source]
        source: trash::Error,
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

/// The path `source` would land on for a `:mv`/`:cp` to `dest_str`.
///
/// `dest_str` is resolved against `cwd` when relative. An existing directory
/// means "into it, under the same name"; anything else is the new path itself.
/// Only stats the destination, so it is cheap enough for the UI thread — the
/// copy or move itself is not.
pub fn resolve_target(source: &Path, dest_str: &str, cwd: &Path) -> PathBuf {
    let raw_dest = resolve_dest(dest_str, cwd);
    if raw_dest.is_dir() {
        raw_dest.join(source.file_name().unwrap_or_default())
    } else {
        raw_dest
    }
}

/// Moves `source` to exactly `dest`, across drives if it has to.
///
/// A rename first, since on one volume that is instant and atomic. A rename
/// cannot cross volumes, though — `C:` to `D:` on Windows, or two mounts
/// elsewhere — and "move here" from another drive is the common case rather
/// than the odd one. So when the rename fails for that reason the entry is
/// copied and the source removed, and the source is removed **only** once the
/// copy has fully succeeded.
///
/// The caller checks that `dest` is free; this does not overwrite, but it does
/// not re-check either.
///
/// # Errors
///
/// Returns [`FsError::Io`] if the rename fails for any other reason, if the
/// fallback copy fails (the partial copy is removed, the source is untouched),
/// or if the source cannot be removed after a successful copy (both then
/// exist, and the message says so).
pub fn move_path(source: &Path, dest: &Path) -> Result<(), FsError> {
    move_path_with(source, dest, |from, to| fs::rename(from, to))
}

/// [`move_path`] with the rename injected, so the cross-drive fallback can be
/// tested on a machine with one drive.
fn move_path_with(
    source: &Path,
    dest: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), FsError> {
    let err = match rename(source, dest) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    if !is_cross_device(&err) {
        return Err(io_err(format!("mv → {}", dest.display()), err));
    }

    if let Err(e) = copy_path(source, dest) {
        // `dest` was free before the copy started, so whatever is there now is
        // the half-finished copy. Leaving it would make a retry fail with
        // "destination already exists".
        let _ = remove_path(dest);
        return Err(e);
    }
    remove_path(source).map_err(|e| FsError::Io {
        context: format!(
            "copied to {} but could not remove the original {}",
            dest.display(),
            source.display()
        ),
        source: e,
    })
}

/// Whether a rename failed because source and destination are on different
/// volumes — the one failure a copy can get around.
///
/// Matched on the raw OS code: `io::ErrorKind::CrossesDevices` is newer than
/// this crate's minimum Rust version.
fn is_cross_device(err: &std::io::Error) -> bool {
    // ERROR_NOT_SAME_DEVICE on Windows, EXDEV elsewhere.
    let code = if cfg!(windows) { 17 } else { 18 };
    err.raw_os_error() == Some(code)
}

/// Copies `source` — a file, or a directory and everything under it — to
/// exactly `dest`.
///
/// # Errors
///
/// Returns [`FsError::InvalidName`] if `dest` is inside `source`: copying a
/// directory into itself never finishes, because every level it creates is
/// another level to copy. Returns [`FsError::Io`] for filesystem errors.
pub fn copy_path(source: &Path, dest: &Path) -> Result<(), FsError> {
    if source.is_dir() {
        if dest.starts_with(source) {
            return Err(FsError::InvalidName(
                dest.display().to_string(),
                "cannot copy a directory into itself",
            ));
        }
        copy_dir_recursive(source, dest)
    } else {
        fs::copy(source, dest)
            .map(|_| ())
            .map_err(|e| io_err(format!("cp → {}", dest.display()), e))
    }
}

/// Unlinks `path`, recursively for a directory. No recycle bin: this is the
/// second half of a move, not a delete the user asked for.
fn remove_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
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

/// Deletes `target` — file or directory (recursively) — the way `mode` says.
///
/// [`DeleteMode::Trash`] is recoverable by the user through their desktop's
/// recycle bin; [`DeleteMode::Permanent`] is not recoverable at all, and on a
/// directory it takes everything under it. Either way the caller is responsible
/// for the confirmation prompt first; that flow lives in `actions/mod.rs` and
/// `ui/status_bar.rs`, and the prompt says which of the two is about to happen.
///
/// # Errors
///
/// Returns [`FsError::SourceNotFound`] if `target` does not exist.
/// Returns [`FsError::Trash`] if the recycle bin refused the entry — never
/// falling back to an unlink, since that would turn a recoverable delete the
/// user asked for into one that is not.
/// Returns [`FsError::Io`] for other filesystem errors.
pub fn delete(target: &Path, mode: DeleteMode) -> Result<(), FsError> {
    if !target.exists() {
        return Err(FsError::SourceNotFound(target.to_owned()));
    }
    match mode {
        DeleteMode::Trash => trash::delete(target).map_err(|e| FsError::Trash {
            path: target.to_owned(),
            source: e,
        }),
        DeleteMode::Permanent => {
            if target.is_dir() {
                fs::remove_dir_all(target)
                    .map_err(|e| io_err(format!("delete dir {}", target.display()), e))
            } else {
                fs::remove_file(target)
                    .map_err(|e| io_err(format!("delete file {}", target.display()), e))
            }
        }
    }
}

// ── Wildcard matching ─────────────────────────────────────────────────────────

/// Whether `token` asks to be matched against several names rather than naming
/// one.
///
/// `*` and `?` are not legal in a Windows filename at all, and are rare enough
/// in a Unix one that reading them as wildcards is what a user typing them into
/// a file manager means. This is what lets `:mv` keep its one-argument form —
/// `:mv ../dest` still moves the selection — and grow a two-argument one without
/// the two becoming ambiguous.
pub fn is_pattern(token: &str) -> bool {
    token.contains('*') || token.contains('?')
}

/// Entries of `cwd` whose names match `pattern`, in listing order.
///
/// `*` matches any run of characters including none, `?` exactly one. Matching is
/// case-insensitive on Windows, where file names are, and case-sensitive
/// elsewhere, where they are not.
///
/// A leading dot is protected the way a shell protects it: `*` does not match a
/// hidden name unless the pattern itself starts with a dot. Otherwise `:mv *`
/// in a repository would quietly take `.git` with it.
///
/// # Errors
///
/// Returns [`FsError::Io`] if `cwd` cannot be read.
pub fn matching_entries(cwd: &Path, pattern: &str) -> Result<Vec<PathBuf>, FsError> {
    let read = fs::read_dir(cwd).map_err(|e| io_err(format!("readdir {}", cwd.display()), e))?;
    let include_hidden = pattern.starts_with('.');

    let mut matches: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') && !include_hidden {
                return None;
            }
            matches_pattern(&name, pattern).then(|| entry.path())
        })
        .collect();

    matches.sort();
    Ok(matches)
}

/// Whether `name` matches `pattern`, with `*` and `?` as the only wildcards.
///
/// Iterative with one backtrack point, so a pattern full of stars cannot make
/// this blow up the way a naive recursive matcher does: `last_star` remembers
/// where to resume, which is all the backtracking `*` needs.
fn matches_pattern(name: &str, pattern: &str) -> bool {
    // Windows file names are case-insensitive, so a pattern typed in either case
    // has to match either case. Elsewhere the filesystem is literal and so is
    // this.
    let (name, pattern) = if cfg!(windows) {
        (name.to_lowercase(), pattern.to_lowercase())
    } else {
        (name.to_owned(), pattern.to_owned())
    };
    let name: Vec<char> = name.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();

    let mut n = 0; // index into `name`
    let mut p = 0; // index into `pattern`
    let mut last_star: Option<(usize, usize)> = None;

    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                // Try the shortest match first and come back for more if the
                // rest of the pattern fails.
                last_star = Some((p, n));
                p += 1;
            }
            Some('?') => {
                p += 1;
                n += 1;
            }
            Some(&ch) if ch == name[n] => {
                p += 1;
                n += 1;
            }
            // Mismatch: let the last `*` swallow one more character, if there
            // was one. If not, this name does not match.
            _ => match last_star {
                Some((star_p, star_n)) => {
                    p = star_p + 1;
                    n = star_n + 1;
                    last_star = Some((star_p, n));
                }
                None => return false,
            },
        }
    }

    // Any pattern left over may only be stars.
    pattern[p..].iter().all(|&ch| ch == '*')
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

    /// What a `:mv` does with one entry, minus the background thread.
    fn mv(source: &Path, dest: &str, cwd: &Path) -> Result<PathBuf, FsError> {
        let target = resolve_target(source, dest, cwd);
        move_path(source, &target)?;
        Ok(target)
    }

    /// What a `:cp` does with one entry, minus the background thread.
    fn cp(source: &Path, dest: &str, cwd: &Path) -> Result<PathBuf, FsError> {
        let target = resolve_target(source, dest, cwd);
        copy_path(source, &target)?;
        Ok(target)
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

    #[test]
    fn cp_refuses_to_copy_a_directory_into_itself() {
        let dir = tmp();
        let src = dir.path().join("srcdir");
        fs::create_dir(&src).unwrap();
        let err = cp(&src, "srcdir", dir.path()).unwrap_err();
        assert!(matches!(err, FsError::InvalidName(..)), "{err}");
    }

    // ── move across drives ────────────────────────────────────────────────────

    /// The rename failure a second volume gives. There is no second volume in
    /// CI, so it is injected.
    fn cross_device(_: &Path, _: &Path) -> std::io::Result<()> {
        let code = if cfg!(windows) { 17 } else { 18 };
        Err(std::io::Error::from_raw_os_error(code))
    }

    #[test]
    fn a_move_across_drives_copies_then_removes_the_source() {
        let dir = tmp();
        let src = dir.path().join("tree");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("leaf.txt"), b"leaf").unwrap();
        let dest = dir.path().join("moved");

        move_path_with(&src, &dest, cross_device).unwrap();

        assert!(!src.exists(), "the source goes once the copy is complete");
        assert_eq!(fs::read(dest.join("leaf.txt")).unwrap(), b"leaf");
    }

    #[test]
    fn a_failed_copy_keeps_the_source_and_cleans_up_the_partial_copy() {
        let dir = tmp();
        let src = dir.path().join("tree");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("leaf.txt"), b"leaf").unwrap();
        // A destination inside the source makes the copy fail.
        let dest = src.join("inside");

        assert!(move_path_with(&src, &dest, cross_device).is_err());
        assert!(src.join("leaf.txt").exists(), "the original is untouched");
        assert!(!dest.exists());
    }

    #[test]
    fn any_other_rename_failure_is_reported_without_copying() {
        let dir = tmp();
        let src = dir.path().join("file.txt");
        fs::write(&src, b"x").unwrap();
        let dest = dir.path().join("dest.txt");

        let denied = |_: &Path, _: &Path| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "denied",
            ))
        };
        assert!(move_path_with(&src, &dest, denied).is_err());
        assert!(src.exists());
        assert!(
            !dest.exists(),
            "only a cross-drive failure falls back to a copy"
        );
    }

    // ── delete ────────────────────────────────────────────────────────────────

    #[test]
    fn delete_removes_file() {
        let dir = tmp();
        let f = dir.path().join("to_delete.txt");
        fs::write(&f, b"").unwrap();
        delete(&f, DeleteMode::Permanent).unwrap();
        assert!(!f.exists());
    }

    #[test]
    fn delete_removes_directory_recursively() {
        let dir = tmp();
        let d = dir.path().join("to_delete");
        fs::create_dir(&d).unwrap();
        fs::write(d.join("child.txt"), b"").unwrap();
        delete(&d, DeleteMode::Permanent).unwrap();
        assert!(!d.exists());
    }

    #[test]
    fn delete_fails_if_missing() {
        let dir = tmp();
        // Checked before either mode is consulted, so one case covers both.
        let err = delete(&dir.path().join("nope"), DeleteMode::Trash).unwrap_err();
        assert!(matches!(err, FsError::SourceNotFound(_)));
    }

    // -- wildcard matching ----------------------------------------------------

    #[test]
    fn a_star_matches_any_run_including_none() {
        assert!(matches_pattern("notes.md", "*.md"));
        assert!(matches_pattern(".md", "*.md"));
        assert!(matches_pattern("a.md", "*"));
        assert!(!matches_pattern("notes.txt", "*.md"));
        assert!(matches_pattern("README", "*"));
    }

    #[test]
    fn a_question_mark_matches_exactly_one_character() {
        assert!(matches_pattern("a.md", "?.md"));
        assert!(!matches_pattern("ab.md", "?.md"));
        assert!(!matches_pattern(".md", "?.md"));
    }

    /// The case a naive matcher gets wrong: a `*` has to give characters back
    /// when what follows it fails to line up.
    #[test]
    fn a_star_backtracks_when_the_tail_does_not_fit() {
        assert!(matches_pattern("archive.tar.gz", "*.tar.gz"));
        assert!(matches_pattern("a.b.c.d", "*.d"));
        assert!(matches_pattern("xxxxab", "*ab"));
        assert!(!matches_pattern("xxxxabc", "*ab"));
        assert!(matches_pattern("abcabcabd", "*abc*abd"));
        // Many stars must not make this fall over.
        assert!(matches_pattern("aaaaaaaaaaaaaaaaaaaab", "*a*a*a*a*a*b"));
        assert!(!matches_pattern("aaaaaaaaaaaaaaaaaaaa", "*a*a*a*a*a*b"));
    }

    #[test]
    fn a_pattern_is_recognised_by_its_wildcards() {
        assert!(is_pattern("*.md"));
        assert!(is_pattern("note?.txt"));
        // A destination with a space in it is not a pattern, which is what keeps
        // the one-argument form of `:mv` working.
        assert!(!is_pattern("my"));
        assert!(!is_pattern("../backup"));
    }

    #[test]
    fn matching_entries_skips_hidden_names_unless_asked() {
        let dir = tmp();
        fs::write(dir.path().join("one.md"), b"").unwrap();
        fs::write(dir.path().join("two.md"), b"").unwrap();
        fs::write(dir.path().join(".hidden.md"), b"").unwrap();

        let visible = matching_entries(dir.path(), "*.md").unwrap();
        assert_eq!(visible.len(), 2, "a bare `*` must not take hidden files");

        let hidden = matching_entries(dir.path(), ".*.md").unwrap();
        assert_eq!(
            hidden.len(),
            1,
            "a pattern starting with a dot asks for them"
        );
        assert!(hidden[0].ends_with(".hidden.md"));
    }

    /// The recycle bin itself is not exercised here: it is per-user desktop
    /// state, and a test suite that fills someone's bin with fixtures — or that
    /// depends on the machine having one — is exactly the ambient-state
    /// dependency the coding standard rules out. What is testable is that the
    /// config can only ask for one of the two behaviours.
    #[test]
    fn delete_modes_parse_from_what_the_config_can_say() {
        assert_eq!(DeleteMode::parse("trash"), Some(DeleteMode::Trash));
        assert_eq!(DeleteMode::parse(" TRASH "), Some(DeleteMode::Trash));
        assert_eq!(DeleteMode::parse("recycle"), Some(DeleteMode::Trash));
        assert_eq!(DeleteMode::parse("permanent"), Some(DeleteMode::Permanent));
        assert_eq!(DeleteMode::parse("delete"), Some(DeleteMode::Permanent));
        assert_eq!(DeleteMode::parse("bin"), None);
        assert_eq!(DeleteMode::parse(""), None);
    }
}
