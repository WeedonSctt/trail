//! Rendering filesystem paths for the two audiences Trail has: the operating
//! system, and the person reading the screen.
//!
//! `std::fs::canonicalize` returns every absolute Windows path in
//! *extended-length* (verbatim) form — `\\?\C:\Users\me`, not `C:\Users\me`.
//! That form is what Win32 wants when a path may be longer than `MAX_PATH`,
//! and it is not what anyone types, reads, or pastes back into a shell. Trail
//! canonicalizes its current directory at startup, so without this module the
//! prefix leaks into the nav panel title, the status bar, `ya` (yank absolute
//! path), the `--cwd-file` handoff and the `trail --paths` report.
//!
//! # The two operations are not the same
//!
//! [`display`] renders a path for a human and always drops the prefix. The
//! result is text that is never handed back to the filesystem, so dropping it
//! cannot break anything.
//!
//! [`simplified`] rewrites a path Trail keeps using, so it drops the prefix
//! only when the plain spelling addresses the very same file. A path past
//! `MAX_PATH`, and a name only the verbatim form can express — a reserved DOS
//! device name, a component ending in a dot or a space, a component holding a
//! character Win32 reads as a separator or a wildcard — keeps the prefix and
//! stays correct.
//!
//! On every non-Windows platform [`simplified`] is the identity: a backslash
//! is an ordinary character in a Unix file name, so a file really can be named
//! `\\?\C:\x` and rewriting it would point somewhere else.

use std::path::{Path, PathBuf};

/// The prefix `canonicalize` puts on an extended-length Windows path.
const VERBATIM_PREFIX: &str = r"\\?\";

/// The prefix on the extended-length form of a UNC network path.
const VERBATIM_UNC_PREFIX: &str = r"\\?\UNC\";

/// Longest path Win32 accepts without the verbatim prefix, the terminating
/// NUL included — so 259 usable characters.
///
/// Compared against the UTF-8 byte length, which is never smaller than the
/// UTF-16 length Win32 actually counts, so the check errs towards keeping the
/// prefix rather than producing a path the API would reject.
const MAX_PATH: usize = 260;

/// Names Win32 resolves to a DOS device rather than to a file, in any
/// directory and whatever extension follows.
const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Renders `path` the way a person would type it.
///
/// Drops the Windows verbatim prefix (`\\?\C:\x` reads as `C:\x`,
/// `\\?\UNC\host\share` as `\\host\share`) and leaves every other path
/// untouched. Device paths such as `\\?\Volume{…}`, which have no plain
/// spelling, keep the prefix — there is nothing shorter that names them.
///
/// The result is for display and for the clipboard, never for reopening the
/// file: use [`simplified`] when the path is going back to the filesystem.
///
/// Lossy for a path that is not valid UTF-8, as any rendering must be.
pub fn display(path: &Path) -> String {
    let text = path.to_string_lossy();
    plain_form(&text).unwrap_or_else(|| text.into_owned())
}

/// Returns `path` with the Windows verbatim prefix dropped when the plain
/// spelling addresses the same file, and unchanged otherwise.
///
/// Use this for a path Trail will keep passing to the filesystem — the
/// current directory, the entries listed under it, a yanked path the user
/// will paste into a shell. Unlike [`display`] it declines to rewrite a path
/// whose plain form would name something else, or nothing at all.
///
/// A no-op on non-Windows platforms.
pub fn simplified(path: &Path) -> PathBuf {
    if !cfg!(windows) {
        return path.to_owned();
    }
    // A path that is not valid UTF-8 is left alone: the rewrite is a string
    // operation, and there is nothing here worth a lossy round-trip for.
    match path
        .to_str()
        .and_then(plain_form)
        .filter(|p| is_safe_plain(p))
    {
        Some(plain) => PathBuf::from(plain),
        None => path.to_owned(),
    }
}

/// [`std::fs::canonicalize`] with the Windows verbatim prefix dropped where
/// [`simplified`] finds it safe to drop.
///
/// This is the form Trail stores in `AppState::cwd`, so that everything
/// derived from it — entry paths, the nav panel title, yanks, the `--cwd-file`
/// handoff — is readable without each call site stripping the prefix again.
///
/// # Errors
///
/// Returns whatever `std::fs::canonicalize` returns: the path must exist and
/// be reachable.
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(|resolved| simplified(&resolved))
}

/// Returns the plain spelling of a verbatim path, or `None` when `text` is
/// not verbatim or names something only the verbatim form can address.
fn plain_form(text: &str) -> Option<String> {
    if let Some(rest) = text.strip_prefix(VERBATIM_UNC_PREFIX) {
        // `\\?\UNC\host\share` is the verbatim spelling of `\\host\share`.
        if rest.is_empty() {
            return None;
        }
        return Some(format!(r"\\{rest}"));
    }

    let rest = text.strip_prefix(VERBATIM_PREFIX)?;
    // Only a drive-letter path also reads as a plain path. `\\?\Volume{…}`
    // and the other device paths do not, and must keep the prefix.
    starts_with_drive(rest).then(|| rest.to_owned())
}

/// True when `text` opens with a drive letter, a colon and a separator.
fn starts_with_drive(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.next() == Some(':')
        && chars.next() == Some('\\')
}

/// True when Win32 resolves `plain` to the same file the verbatim form named.
fn is_safe_plain(plain: &str) -> bool {
    plain.len() < MAX_PATH && components_after_root(plain).all(is_literal_component)
}

/// Yields the components of `plain` past its root — the parts that name files
/// and directories, and so the parts Win32 would reinterpret.
///
/// The root is `C:\` on a drive path and `\\host\share\` on a UNC one; a host
/// and share name are not file names and are not checked.
fn components_after_root(plain: &str) -> impl Iterator<Item = &str> {
    let body = match plain.strip_prefix(r"\\") {
        Some(unc) => unc.splitn(3, '\\').nth(2).unwrap_or(""),
        None => plain.get(3..).unwrap_or(""),
    };
    body.split('\\').filter(|part| !part.is_empty())
}

/// True when Win32 reads `component` as the literal file of that name once the
/// verbatim prefix is gone.
fn is_literal_component(component: &str) -> bool {
    // Legal file names under the verbatim prefix; directory traversal without
    // it.
    if component == "." || component == ".." {
        return false;
    }
    // Win32 trims a trailing dot or space off a plain path component, so
    // `report.` would resolve to `report`.
    if component.ends_with('.') || component.ends_with(' ') {
        return false;
    }
    // Legal in a verbatim component, and not literal in a plain one: `/`
    // separates, `?` and `*` are wildcards, `:` opens an alternate data
    // stream, and the rest are redirection syntax Win32 rejects outright.
    if component.contains(['/', '?', '*', ':', '<', '>', '"', '|']) {
        return false;
    }
    // A reserved device name beats a real file with the same stem.
    let stem = component.split('.').next().unwrap_or(component);
    !RESERVED_DEVICE_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The string-level helpers carry the whole rewrite, and are deliberately
    // platform-independent so they are covered on a Linux CI runner too.

    // ── plain_form ─────────────────────────────────────────────────────────────

    #[test]
    fn a_verbatim_drive_path_has_a_plain_form() {
        assert_eq!(
            plain_form(r"\\?\C:\Users\me\notes.txt").as_deref(),
            Some(r"C:\Users\me\notes.txt")
        );
    }

    #[test]
    fn a_verbatim_unc_path_folds_back_to_two_backslashes() {
        assert_eq!(
            plain_form(r"\\?\UNC\host\share\file.txt").as_deref(),
            Some(r"\\host\share\file.txt")
        );
    }

    #[test]
    fn a_plain_path_has_no_verbatim_form_to_strip() {
        assert_eq!(plain_form(r"C:\Users\me"), None);
        assert_eq!(plain_form("/home/me/notes.txt"), None);
    }

    #[test]
    fn a_device_path_keeps_its_prefix() {
        // `\\?\Volume{…}` names a volume with no drive letter mounted. There
        // is no shorter spelling, so rewriting it would break the path.
        assert_eq!(
            plain_form(r"\\?\Volume{9a1b2c3d-0000-0000-0000-000000000000}\data"),
            None
        );
        assert_eq!(plain_form(r"\\?\pipe\trail"), None);
    }

    // ── is_safe_plain ──────────────────────────────────────────────────────────

    #[test]
    fn an_ordinary_path_is_safe_to_simplify() {
        assert!(is_safe_plain(r"C:\Users\me\proj\trail\src\pathfmt.rs"));
        assert!(is_safe_plain(r"\\host\share\team\notes.txt"));
        // A bare root has no components to reinterpret.
        assert!(is_safe_plain(r"C:\"));
    }

    #[test]
    fn a_path_past_max_path_keeps_the_prefix() {
        let long = format!(r"C:\{}", "a".repeat(MAX_PATH));
        assert!(!is_safe_plain(&long));
    }

    #[test]
    fn a_reserved_device_name_keeps_the_prefix() {
        // Plain `C:\tmp\nul.txt` opens the NUL device, not the file — the
        // file is only reachable through the verbatim form.
        assert!(!is_safe_plain(r"C:\tmp\nul.txt"));
        assert!(!is_safe_plain(r"C:\tmp\CON"));
        assert!(!is_safe_plain(r"C:\tmp\com4.log"));
        // Not reserved: the stem has to match exactly.
        assert!(is_safe_plain(r"C:\tmp\console.log"));
        assert!(is_safe_plain(r"C:\tmp\com10"));
    }

    #[test]
    fn a_component_win32_would_rewrite_keeps_the_prefix() {
        // Win32 trims a trailing dot or space; the verbatim form does not.
        assert!(!is_safe_plain(r"C:\tmp\report."));
        assert!(!is_safe_plain(r"C:\tmp\report "));
        // `.` and `..` are literal names under the prefix and traversal
        // without it.
        assert!(!is_safe_plain(r"C:\tmp\..\other"));
        // A verbatim component may contain a slash; a plain one may not.
        assert!(!is_safe_plain(r"C:\tmp\a/b"));
        assert!(!is_safe_plain(r"C:\tmp\a:stream"));
        assert!(!is_safe_plain(r"C:\tmp\wild*card"));
    }

    #[test]
    fn a_unc_host_and_share_are_not_checked_as_file_names() {
        // The host and share sit in the root, not in the components Win32
        // would reinterpret — a share named `con$` is perfectly addressable.
        assert!(is_safe_plain(r"\\host\con$\notes.txt"));
    }

    // ── display ────────────────────────────────────────────────────────────────

    #[test]
    fn display_drops_the_verbatim_prefix() {
        assert_eq!(
            display(Path::new(r"\\?\C:\Users\me")),
            r"C:\Users\me".to_owned()
        );
        assert_eq!(
            display(Path::new(r"\\?\UNC\host\share")),
            r"\\host\share".to_owned()
        );
    }

    #[test]
    fn display_leaves_every_other_path_alone() {
        assert_eq!(display(Path::new("/home/me")), "/home/me".to_owned());
        assert_eq!(
            display(Path::new(r"C:\Users\me")),
            r"C:\Users\me".to_owned()
        );
    }

    #[test]
    fn display_renders_a_long_path_readably() {
        // `display` is text, never reopened, so readability wins over the
        // round-trip guarantee `simplified` has to keep.
        let long = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH));
        assert!(!display(Path::new(&long)).contains(VERBATIM_PREFIX));
    }

    // ── simplified ─────────────────────────────────────────────────────────────

    #[test]
    #[cfg(windows)]
    fn simplified_drops_a_prefix_it_can_drop() {
        assert_eq!(
            simplified(Path::new(r"\\?\C:\Users\me")),
            PathBuf::from(r"C:\Users\me")
        );
    }

    #[test]
    #[cfg(windows)]
    fn simplified_keeps_a_prefix_the_path_needs() {
        let long = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH));
        assert_eq!(simplified(Path::new(&long)), PathBuf::from(&long));
        assert_eq!(
            simplified(Path::new(r"\\?\C:\tmp\nul.txt")),
            PathBuf::from(r"\\?\C:\tmp\nul.txt")
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn simplified_is_the_identity_off_windows() {
        // A Unix file really can be named `\\?\C:\x`; rewriting it would name
        // a different file.
        for path in [r"\\?\C:\Users\me", "/home/me/notes.txt"] {
            assert_eq!(simplified(Path::new(path)), PathBuf::from(path));
        }
    }

    #[test]
    fn canonicalize_resolves_and_leaves_no_verbatim_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = canonicalize(dir.path()).expect("canonicalize");
        assert!(resolved.is_absolute());
        assert!(
            !resolved.to_string_lossy().starts_with(VERBATIM_PREFIX),
            "a temp directory is short and ordinary — it should simplify: {resolved:?}"
        );
    }
}
