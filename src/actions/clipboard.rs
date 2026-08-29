//! Clipboard operations: copy absolute path, relative path, filename, and
//! the content of the selected entry.
//!
//! Bound to `ya` (absolute path), `yr` (relative path), `yn` (filename name)
//! and `yc` (content) in Navigation Mode.
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
//! `content_text` is the one function here that reads from disk, since the
//! content of the selection is not derivable from its path. Its read is
//! bounded by [`CONTENT_YANK_MAX_BYTES`] so the UI thread cannot stall on it.
//!
//! The yanked string is also recorded in `AppState::last_yank` so the status
//! bar and tests can observe it, and logged at `info` level to the log file.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// Largest file, in bytes, whose content `yc` will place on the clipboard.
///
/// Clipboards are not archives: a yank of an unbounded file would stall the UI
/// thread on the read and hand the OS a payload no paste target wants. One
/// mebibyte covers source files, configs and logs-in-progress while keeping
/// the read bounded.
///
/// Named constant per coding-standard §10: no magic numbers.
pub const CONTENT_YANK_MAX_BYTES: u64 = 1024 * 1024;

/// Bytes inspected when deciding whether a file is binary.
///
/// Matches the probe size used by `preview::text::is_text_file`, so `yc`
/// agrees with the preview pane about what counts as text.
const BINARY_PROBE_BYTES: usize = 8192;

/// Errors from clipboard operations.
#[derive(Debug, Error)]
pub enum ClipboardError {
    /// The source path could not be represented as a UTF-8 string.
    #[error("path is not valid UTF-8")]
    NotUtf8,
    /// The arboard crate failed to access the OS clipboard.
    #[error("clipboard error: {0}")]
    Arboard(#[from] arboard::Error),
    /// The entry could not be read from disk.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The file holds binary data, which has no meaningful clipboard form.
    #[error("binary file — nothing to yank")]
    BinaryContent,
    /// The file is larger than [`CONTENT_YANK_MAX_BYTES`].
    #[error("file is {size} bytes, over the {limit}-byte yank limit")]
    TooLarge {
        /// Size of the offending file, in bytes.
        size: u64,
        /// The limit it exceeded, in bytes.
        limit: u64,
    },
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

/// Returns the path of `entry_path` relative to `base` as a string.
///
/// `base` is the directory Trail was launched from (`AppState::launch_dir`),
/// not the directory currently being browsed. Relative to the browsed
/// directory every selection is just its own file name — which is what `yn`
/// already yanks. The path is only useful as something that can be pasted back
/// into the shell Trail was started in.
///
/// Entries outside `base` are expressed with `..` segments, so the result stays
/// relative wherever the user has navigated to. Falls back to the absolute path
/// only when the two cannot be related at all — a different Windows drive, or a
/// `base` that is not absolute.
///
/// Pure: performs no clipboard or filesystem access.
///
/// # Errors
///
/// Returns [`ClipboardError::NotUtf8`] if the resulting path is not valid UTF-8.
pub fn relative_path_text(entry_path: &Path, base: &Path) -> Result<String, ClipboardError> {
    let relative = relative_to(entry_path, base);
    let rel = relative.as_deref().unwrap_or(entry_path);
    Ok(rel.to_str().ok_or(ClipboardError::NotUtf8)?.to_owned())
}

/// Expresses `target` relative to `base`, climbing out of `base` with `..`.
///
/// Returns `None` when no relative path exists: the two disagree about being
/// absolute, they sit on different Windows drives, or `base` contains `..`
/// components that cannot be undone without touching the filesystem.
///
/// Both inputs are expected to be canonical — in Trail they are: `base` comes
/// from `AppState::launch_dir` and `target` from a listing of an
/// already-canonicalized `cwd` — so no normalization is attempted here.
fn relative_to(target: &Path, base: &Path) -> Option<PathBuf> {
    if target.is_absolute() != base.is_absolute() {
        return None;
    }

    let mut target_parts = target.components();
    let mut base_parts = base.components();
    let mut result: Vec<Component> = Vec::new();

    loop {
        match (target_parts.next(), base_parts.next()) {
            // Both exhausted: `target` is `base` itself.
            (None, None) => break,
            // `base` ran out: whatever is left of `target` is the answer.
            (Some(t), None) => {
                result.push(t);
                result.extend(target_parts.by_ref());
                break;
            }
            // `target` ran out: climb once per remaining `base` component.
            (None, Some(_)) => result.push(Component::ParentDir),
            // Still walking the shared prefix.
            (Some(t), Some(b)) if result.is_empty() && t == b => {}
            // A `..` in `base` cannot be undone without resolving it on disk.
            (Some(_), Some(Component::ParentDir)) => return None,
            // Differing roots or Windows drive prefixes cannot be related.
            (Some(t), Some(b))
                if matches!(t, Component::Prefix(_) | Component::RootDir)
                    || matches!(b, Component::Prefix(_) | Component::RootDir) =>
            {
                return None;
            }
            // The paths diverge: climb out of the rest of `base`, then descend.
            (Some(t), Some(_)) => {
                result.push(Component::ParentDir);
                result.extend(base_parts.by_ref().map(|_| Component::ParentDir));
                result.push(t);
                result.extend(target_parts.by_ref());
                break;
            }
        }
    }

    // `target` is `base`: the relative path to a directory from itself is `.`,
    // not the empty string, which no shell would accept.
    if result.is_empty() {
        result.push(Component::CurDir);
    }

    Some(result.iter().map(|c| c.as_os_str()).collect())
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

// ── Content ───────────────────────────────────────────────────────────────────

/// Returns the content of `path` as a string, ready to be yanked.
///
/// For a regular file this is the file's text. For a directory it is the
/// listing — one entry name per line, directories suffixed with `/`, sorted
/// directories-first then case-insensitively, matching what the preview pane
/// shows. `show_hidden` mirrors the navigation panel's own filter, so the
/// yanked listing is the listing the user is looking at.
///
/// Symlinks are followed, so what is yanked is the content of the target.
///
/// Unlike the `*_text` helpers above, this one reads from disk. The read is
/// bounded: a file over [`CONTENT_YANK_MAX_BYTES`] is rejected rather than
/// truncated, since half a file pasted silently is worse than no paste at all.
///
/// # Errors
///
/// - [`ClipboardError::Io`] if `path` cannot be read.
/// - [`ClipboardError::TooLarge`] if the file exceeds [`CONTENT_YANK_MAX_BYTES`].
/// - [`ClipboardError::BinaryContent`] if the file is not text.
/// - [`ClipboardError::NotUtf8`] if the file's bytes are not valid UTF-8, or a
///   directory holds an entry whose name is not UTF-8.
pub fn content_text(path: &Path, show_hidden: bool) -> Result<String, ClipboardError> {
    // `metadata` follows symlinks, so a link to a directory lists the target.
    let metadata = std::fs::metadata(path)?;

    if metadata.is_dir() {
        return directory_listing_text(path, show_hidden);
    }

    let size = metadata.len();
    if size > CONTENT_YANK_MAX_BYTES {
        return Err(ClipboardError::TooLarge {
            size,
            limit: CONTENT_YANK_MAX_BYTES,
        });
    }

    let bytes = std::fs::read(path)?;
    let probe = &bytes[..bytes.len().min(BINARY_PROBE_BYTES)];
    if content_inspector::inspect(probe).is_binary() {
        return Err(ClipboardError::BinaryContent);
    }

    String::from_utf8(bytes).map_err(|_| ClipboardError::NotUtf8)
}

/// Builds the newline-separated listing yanked for a directory.
///
/// Shares its sorting and `/` suffix convention with
/// `preview::directory::build_directory_preview` so the clipboard matches what
/// the preview pane displayed — but without that function's display cap, since
/// a truncated listing is not worth pasting.
fn directory_listing_text(path: &Path, show_hidden: bool) -> Result<String, ClipboardError> {
    let mut names: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or(ClipboardError::NotUtf8)?
            .to_owned();

        if name.starts_with('.') && !show_hidden {
            continue;
        }

        // A failed stat means the entry exists but is unreadable; list it as a
        // plain file rather than dropping it from the listing.
        let is_dir = entry.metadata().map(|m| m.is_dir()).unwrap_or(false);
        names.push(if is_dir { format!("{name}/") } else { name });
    }

    names.sort_by(|a, b| {
        let a_dir = a.ends_with('/');
        let b_dir = b.ends_with('/');
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.to_lowercase().cmp(&b.to_lowercase()),
        }
    });

    Ok(names.join("\n"))
}

// ── Clipboard write ───────────────────────────────────────────────────────────

/// Writes `text` to the OS clipboard.
///
/// The single place in this module that talks to `arboard`, so the pure
/// path-computation functions above stay free of I/O.
///
/// Callers are expected to treat a failure here as non-fatal: the text was
/// computed successfully, only the hand-off to the OS failed. `actions::apply`
/// still records the yank in `AppState::last_yank` and surfaces the error in
/// the status bar, so the operation degrades gracefully on machines with no
/// reachable clipboard (headless servers, bare TTYs, CI runners).
///
/// # Errors
///
/// Returns [`ClipboardError::Arboard`] if the OS clipboard cannot be opened
/// or written to.
pub fn set_clipboard(text: &str) -> Result<(), ClipboardError> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(text.to_owned())?;
    Ok(())
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

    /// Rewrites native separators to `/` so the path assertions below read the
    /// same on Windows, where `relative_to` collects components with `\`.
    fn norm(s: &str) -> String {
        s.replace('\\', "/")
    }

    #[test]
    fn relative_path_strips_base() {
        let base = PathBuf::from("/home/user/project");
        let entry = PathBuf::from("/home/user/project/src/main.rs");
        let s = relative_path_text(&entry, &base).unwrap();
        assert_eq!(norm(&s), "src/main.rs");
    }

    #[test]
    fn relative_path_climbs_out_of_base_with_parent_segments() {
        // Trail was launched in `project` and the user has navigated up and
        // across; the yank must still be pasteable in the launch shell.
        let base = PathBuf::from("/home/user/project");
        let entry = PathBuf::from("/home/user/notes/todo.md");
        let s = relative_path_text(&entry, &base).unwrap();
        assert_eq!(norm(&s), "../notes/todo.md");
    }

    #[test]
    fn relative_path_of_base_itself_is_current_dir() {
        // Reachable: navigate to the launch directory's parent and select it.
        let base = PathBuf::from("/home/user/project");
        let s = relative_path_text(&base, &base).unwrap();
        assert_eq!(s, ".");
    }

    #[test]
    fn relative_path_falls_back_to_absolute_for_unrelatable_paths() {
        // A relative base cannot be related to an absolute entry: nothing
        // sensible to compute, so the absolute path is yanked instead.
        let base = PathBuf::from("project");
        let entry = PathBuf::from("/home/user/project/main.rs");
        let s = relative_path_text(&entry, &base).unwrap();
        assert_eq!(s, "/home/user/project/main.rs");
    }

    #[cfg(windows)]
    #[test]
    fn relative_path_falls_back_to_absolute_across_windows_drives() {
        let base = PathBuf::from(r"C:\Users\dev");
        let entry = PathBuf::from(r"D:\data\notes.md");
        let s = relative_path_text(&entry, &base).unwrap();
        assert_eq!(s, r"D:\data\notes.md");
    }

    #[test]
    fn content_of_text_file_is_the_file_text() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        std::fs::write(&file, b"line one\nline two\n").unwrap();

        let s = content_text(&file, false).unwrap();
        assert_eq!(s, "line one\nline two\n");
    }

    #[test]
    fn content_of_directory_is_a_sorted_listing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), b"").unwrap();
        std::fs::write(dir.path().join("a.txt"), b"").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let s = content_text(dir.path(), false).unwrap();
        // Directories first (with a `/` suffix), then files case-insensitively.
        assert_eq!(s, "sub/\na.txt\nb.txt");
    }

    #[test]
    fn content_of_directory_honours_show_hidden() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("visible.txt"), b"").unwrap();
        std::fs::write(dir.path().join(".hidden"), b"").unwrap();

        assert_eq!(content_text(dir.path(), false).unwrap(), "visible.txt");
        assert_eq!(
            content_text(dir.path(), true).unwrap(),
            ".hidden\nvisible.txt"
        );
    }

    #[test]
    fn content_of_binary_file_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("blob.bin");
        std::fs::write(&file, [0u8, 1, 2, 0, 3, 0]).unwrap();

        assert!(matches!(
            content_text(&file, false),
            Err(ClipboardError::BinaryContent)
        ));
    }

    #[test]
    fn content_over_the_size_limit_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("big.txt");
        let big = vec![b'a'; (CONTENT_YANK_MAX_BYTES + 1) as usize];
        std::fs::write(&file, big).unwrap();

        // Rejected rather than truncated: a partial paste is a silent lie.
        assert!(matches!(
            content_text(&file, false),
            Err(ClipboardError::TooLarge { .. })
        ));
    }

    #[test]
    fn content_of_missing_path_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.txt");
        assert!(matches!(
            content_text(&missing, false),
            Err(ClipboardError::Io(_))
        ));
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
