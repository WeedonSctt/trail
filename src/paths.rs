//! Every filesystem location Trail owns, resolved in one place.
//!
//! Three call sites used to resolve `ProjectDirs::from("", "", "trail")`
//! independently — `main`, `plugin::bookmarks` and `plugin` — which meant the
//! answer to "where does Trail keep my bookmarks?" was spread across the
//! codebase and could drift. Everything now goes through this module, and
//! [`report`] prints exactly what it resolves, so `trail --paths`, the
//! uninstall scripts and the running program cannot disagree about where
//! anything lives.
//!
//! # Why this matters for uninstalling
//!
//! The paths are platform-dependent and not guessable: the same install puts
//! data in `~/.local/share/trail` on Linux, `~/Library/Application
//! Support/trail` on macOS and `%APPDATA%\trail\data` on Windows. A user who
//! wants Trail gone needs that list, and reading it out of the binary is the
//! only way to be sure it matches the build they are actually running.

use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs};

/// Bookmark store, inside [`data_dir`].
pub const BOOKMARKS_FILE: &str = "bookmarks.toml";

/// Recent-directory store, inside [`data_dir`].
pub const RECENT_DIRS_FILE: &str = "recent_dirs.toml";

/// Log file, written into the system temp directory.
///
/// Trail owns the alternate screen, so log output can never go to stdout; it
/// goes here instead. Temp is deliberate — the log is a debugging aid, not
/// state worth preserving, and the OS reclaims it.
pub const LOG_FILE: &str = "trail.log";

/// The platform's project directories for Trail, if the OS reports a home.
fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "trail")
}

/// Directory holding everything Trail persists between runs: bookmarks,
/// recent directories, and the remembered `--config` path.
///
/// Falls back to the current directory when the platform will not name a home
/// directory, so persistence degrades to "beside the user" rather than being
/// dropped silently.
pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// Directory Trail reads user configuration and Lua plugins from.
///
/// `None` when the platform will not name a home directory, in which case
/// plugins are resolved relative to the current directory instead.
pub fn config_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().to_path_buf())
}

/// Path to the bookmark store.
pub fn bookmarks_file() -> PathBuf {
    data_dir().join(BOOKMARKS_FILE)
}

/// Path to the recent-directory store.
pub fn recent_dirs_file() -> PathBuf {
    data_dir().join(RECENT_DIRS_FILE)
}

/// Path to the file recording the config last loaded with `--config`.
pub fn state_file() -> PathBuf {
    data_dir().join(crate::config::last_used::STATE_FILE_NAME)
}

/// Path to the log file, or `None` if the temp directory cannot be resolved.
///
/// Mirrors the resolution in `main`, including the `canonicalize` step, so the
/// reported path is the one actually written to.
pub fn log_file() -> Option<PathBuf> {
    std::env::temp_dir()
        .canonicalize()
        .ok()
        .map(|dir| dir.join(LOG_FILE))
}

/// Path to the running executable, or `None` if the OS will not report it.
pub fn binary() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Conventional locations of the shell wrappers, for the current platform.
///
/// Trail does not install these — the install scripts and the package managers
/// do, and any of them can be pointed somewhere else. This is therefore a list
/// of places worth *looking*, which [`report`] annotates with what it finds,
/// not a claim about where they must be.
pub fn wrapper_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let bin = PathBuf::from(local).join("trail").join("bin");
            candidates.push(bin.join("trail.ps1"));
        }
        if let Some(scoop) = std::env::var_os("SCOOP") {
            candidates.push(
                PathBuf::from(scoop)
                    .join("apps")
                    .join("trail")
                    .join("current")
                    .join("shell")
                    .join("trail.ps1"),
            );
        }
    }

    if let Some(base) = BaseDirs::new() {
        let home = base.home_dir();
        let shell_dir = home
            .join(".local")
            .join("share")
            .join("trail")
            .join("shell");
        candidates.push(shell_dir.join("trail.bash"));
        candidates.push(shell_dir.join("trail.zsh"));
        candidates.push(
            home.join(".config")
                .join("fish")
                .join("functions")
                .join("trail.fish"),
        );
    }

    if !cfg!(windows) {
        // Package-manager prefixes: AUR installs to /usr/share, Homebrew to
        // its own prefix, which differs between Apple Silicon and Intel.
        candidates.push(PathBuf::from("/usr/share/trail/shell/trail.bash"));
        candidates.push(PathBuf::from("/opt/homebrew/share/trail/shell/trail.bash"));
        candidates.push(PathBuf::from("/usr/local/share/trail/shell/trail.bash"));
    }

    candidates
}

/// Renders the `--paths` report.
///
/// Every path Trail reads or writes, annotated with whether it currently
/// exists, followed by a pointer at the uninstall documentation. Printed
/// before the terminal is put into raw mode, so it is ordinary stdout output
/// that can be piped or redirected.
pub fn report() -> String {
    let mut lines = vec![
        format!("trail {}", env!("CARGO_PKG_VERSION")),
        String::new(),
    ];

    lines.push("Binary".to_owned());
    match binary() {
        Some(path) => lines.push(entry(&path)),
        None => lines.push("  (could not be determined)".to_owned()),
    }
    lines.push(String::new());

    lines.push("Configuration  — yours; no uninstall removes it unless asked".to_owned());
    match config_dir() {
        Some(dir) => lines.push(entry(&dir)),
        None => lines.push("  (no home directory; plugins resolve against the cwd)".to_owned()),
    }
    let state = state_file();
    match crate::config::last_used::remembered(&state) {
        Some(path) => lines.push(format!("  remembered --config: {}", path.display())),
        None => lines.push("  remembered --config: none".to_owned()),
    }
    lines.push(String::new());

    lines.push("Data  — yours; no uninstall removes it unless asked".to_owned());
    lines.push(entry(&data_dir()));
    lines.push(entry(&bookmarks_file()));
    lines.push(entry(&recent_dirs_file()));
    lines.push(entry(&state));
    lines.push(String::new());

    lines.push("Log".to_owned());
    match log_file() {
        Some(path) => lines.push(entry(&path)),
        None => lines.push("  (temp directory unavailable; logging is off)".to_owned()),
    }
    lines.push(String::new());

    lines.push("Shell wrappers  — conventional locations, not an exhaustive list".to_owned());
    let candidates = wrapper_candidates();
    let found = candidates.iter().filter(|p| p.exists()).count();
    for path in &candidates {
        if path.exists() {
            lines.push(entry(path));
        }
    }
    if found == 0 {
        lines.push("  none found in the conventional locations".to_owned());
    }
    lines.push(String::new());

    lines.push("To remove Trail, see the Uninstalling section of".to_owned());
    lines.push("https://github.com/WeedonSctt/trail/blob/main/docs/installation.md".to_owned());

    lines.join("\n")
}

/// Formats one path line, marking whether it is present on disk.
fn entry(path: &std::path::Path) -> String {
    let mark = if path.exists() { "" } else { "  (absent)" };
    format!("  {}{}", display_path(path), mark)
}

/// Renders a path for human consumption.
///
/// `canonicalize` returns Windows paths in extended-length form (`\\?\C:\…`),
/// which is correct, accepted by the API, and unreadable in a report — and
/// would be pasted straight into a shell by anyone following the uninstall
/// docs. The prefix is display-only, so dropping it loses nothing.
fn display_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_files_sit_inside_the_data_directory() {
        let dir = data_dir();
        assert!(bookmarks_file().starts_with(&dir));
        assert!(recent_dirs_file().starts_with(&dir));
        assert!(state_file().starts_with(&dir));
    }

    #[test]
    fn data_files_use_the_documented_names() {
        assert!(bookmarks_file().ends_with(BOOKMARKS_FILE));
        assert!(recent_dirs_file().ends_with(RECENT_DIRS_FILE));
        assert!(state_file().ends_with(crate::config::last_used::STATE_FILE_NAME));
    }

    #[test]
    fn the_report_names_every_section_a_user_needs() {
        let report = report();
        for heading in ["Binary", "Configuration", "Data", "Log", "Shell wrappers"] {
            assert!(
                report.contains(heading),
                "report is missing the {heading} section:\n{report}"
            );
        }
    }

    #[test]
    fn the_report_points_at_the_uninstall_documentation() {
        assert!(report().contains("installation.md"));
    }

    #[test]
    fn the_report_never_shows_an_extended_length_prefix() {
        // `\\?\C:\…` is what canonicalize hands back on Windows, and it must
        // not reach a report a user is meant to read paths out of.
        assert!(
            !report().contains(r"\\?\"),
            "report leaked a verbatim prefix"
        );
    }

    #[test]
    fn display_path_strips_only_the_verbatim_prefix() {
        assert_eq!(
            display_path(std::path::Path::new(r"\\?\C:\tmp\trail.log")),
            r"C:\tmp\trail.log"
        );
        assert_eq!(
            display_path(std::path::Path::new("/tmp/trail.log")),
            "/tmp/trail.log"
        );
    }

    #[test]
    fn wrapper_candidates_are_absolute() {
        for path in wrapper_candidates() {
            assert!(
                path.is_absolute(),
                "candidate must be absolute so the report is unambiguous: {}",
                path.display()
            );
        }
    }
}
