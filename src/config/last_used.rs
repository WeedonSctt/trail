//! Remembers the config file last loaded via `--config`.
//!
//! Passing `--config <path>` records that path so a later bare `trail` picks
//! the same configuration back up, instead of silently falling back to the
//! built-in defaults. The path is stored in Trail's data directory alongside
//! `bookmarks.toml` and `recent_dirs.toml`.
//!
//! # Why the source matters
//!
//! A config that fails to load is fatal when the user asked for it by name —
//! they passed `--config`, so a silent fallback would hide a typo. The same
//! failure must *not* be fatal for a remembered path: the user may have moved,
//! deleted or broken that file long after the run that recorded it, and a bare
//! `trail` refusing to start would leave them with no obvious way back. The
//! [`ConfigSource`] returned by [`resolve`] carries that distinction so the
//! caller applies the right policy instead of inferring it.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name, inside Trail's data directory, holding the remembered path.
pub const STATE_FILE_NAME: &str = "last_config.toml";

/// On-disk shape of [`STATE_FILE_NAME`].
#[derive(Debug, Serialize, Deserialize)]
struct StoredConfigPath {
    /// Absolute path to the config file last loaded via `--config`.
    path: PathBuf,
}

/// Where this run's configuration comes from.
///
/// The variant determines how a load failure is handled — see the module
/// documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// No config file: use the built-in defaults.
    Defaults,

    /// A path the user named on this run via `--config`.
    ///
    /// A load failure here is fatal: the user asked for this file by name.
    Explicit(PathBuf),

    /// A path recalled from an earlier run.
    ///
    /// A load failure here degrades to the built-in defaults, because the file
    /// may have changed or disappeared since it was recorded.
    Remembered(PathBuf),
}

/// Decides which configuration this run should load.
///
/// Precedence: an `explicit` `--config` path wins; otherwise a `remembered`
/// path is used unless `ignore_remembered` (`--no-config`) is set; otherwise
/// the built-in defaults.
///
/// Pure: performs no I/O, so the precedence rules are testable on their own.
pub fn resolve(
    explicit: Option<PathBuf>,
    remembered: Option<PathBuf>,
    ignore_remembered: bool,
) -> ConfigSource {
    if let Some(path) = explicit {
        return ConfigSource::Explicit(path);
    }
    if ignore_remembered {
        return ConfigSource::Defaults;
    }
    match remembered {
        Some(path) => ConfigSource::Remembered(path),
        None => ConfigSource::Defaults,
    }
}

/// Reads the remembered config path from `state_file`.
///
/// Returns `None` when the file is absent, unreadable, or malformed — a
/// corrupt state file must never stop Trail from starting, so every failure
/// degrades to "nothing remembered" and is logged at `debug` level.
pub fn remembered(state_file: &Path) -> Option<PathBuf> {
    let content = match std::fs::read_to_string(state_file) {
        Ok(c) => c,
        Err(e) => {
            // Absent is the normal case on a first run, so this is not a warning.
            tracing::debug!(?state_file, "no remembered config: {e}");
            return None;
        }
    };

    match toml::from_str::<StoredConfigPath>(&content) {
        Ok(stored) => Some(stored.path),
        Err(e) => {
            tracing::debug!(?state_file, "ignoring malformed remembered config: {e}");
            None
        }
    }
}

/// Records `config_path` in `state_file` as the config to reuse next run.
///
/// The path is made absolute first: Trail is normally launched from whichever
/// directory the user happens to be in, so a relative `--config trail.toml`
/// would not resolve on a later run from elsewhere.
///
/// Call this only after the config has loaded successfully, so a file that
/// cannot be parsed is never recorded as the one to reuse.
///
/// # Errors
///
/// Returns an [`io::Error`] if the state file cannot be serialized or written.
/// Callers should treat that as non-fatal — failing to remember is an
/// inconvenience, not a reason to abort the session.
pub fn remember(state_file: &Path, config_path: &Path) -> io::Result<()> {
    // absolute() does not resolve symlinks and does not add a Windows verbatim
    // (\\?\) prefix, so the stored path stays readable if a human opens the
    // state file. Fall back to the path as given if it cannot be absolutized.
    let absolute = std::path::absolute(config_path).unwrap_or_else(|e| {
        tracing::debug!(?config_path, "could not absolutize config path: {e}");
        config_path.to_owned()
    });

    let stored = StoredConfigPath { path: absolute };
    let content = toml::to_string_pretty(&stored)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(state_file, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_prefers_explicit_over_remembered() {
        let source = resolve(
            Some(PathBuf::from("/explicit.toml")),
            Some(PathBuf::from("/remembered.toml")),
            false,
        );
        assert_eq!(
            source,
            ConfigSource::Explicit(PathBuf::from("/explicit.toml"))
        );
    }

    #[test]
    fn resolve_uses_remembered_when_no_explicit() {
        let source = resolve(None, Some(PathBuf::from("/remembered.toml")), false);
        assert_eq!(
            source,
            ConfigSource::Remembered(PathBuf::from("/remembered.toml"))
        );
    }

    #[test]
    fn resolve_ignores_remembered_when_bypassed() {
        let source = resolve(None, Some(PathBuf::from("/remembered.toml")), true);
        assert_eq!(source, ConfigSource::Defaults);
    }

    #[test]
    fn resolve_falls_back_to_defaults() {
        assert_eq!(resolve(None, None, false), ConfigSource::Defaults);
    }

    #[test]
    fn explicit_still_wins_when_remembered_is_bypassed() {
        // --config and --no-config conflict at the CLI layer, but `resolve`
        // stays total rather than relying on that guarantee.
        let source = resolve(Some(PathBuf::from("/explicit.toml")), None, true);
        assert_eq!(
            source,
            ConfigSource::Explicit(PathBuf::from("/explicit.toml"))
        );
    }

    #[test]
    fn remember_then_recall_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join(STATE_FILE_NAME);
        let config = dir.path().join("trail.toml");
        std::fs::write(&config, "").unwrap();

        remember(&state, &config).unwrap();
        assert_eq!(remembered(&state), Some(config));
    }

    #[test]
    fn remember_stores_an_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join(STATE_FILE_NAME);

        remember(&state, Path::new("trail.toml")).unwrap();

        let recalled = remembered(&state).expect("a path should be remembered");
        assert!(
            recalled.is_absolute(),
            "a relative --config path must be stored absolute so it still \
             resolves when Trail is next launched from another directory; got {recalled:?}"
        );
        assert!(recalled.ends_with("trail.toml"));
    }

    #[test]
    fn recall_returns_none_when_state_file_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(remembered(&dir.path().join("does-not-exist.toml")), None);
    }

    #[test]
    fn recall_returns_none_for_a_malformed_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join(STATE_FILE_NAME);
        std::fs::write(&state, "this is not valid toml {{{").unwrap();

        // A corrupt state file must degrade to "nothing remembered" rather
        // than stopping Trail from starting.
        assert_eq!(remembered(&state), None);
    }

    #[test]
    fn recall_returns_none_when_key_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join(STATE_FILE_NAME);
        std::fs::write(&state, "unrelated = \"value\"\n").unwrap();

        assert_eq!(remembered(&state), None);
    }

    #[test]
    fn remember_overwrites_a_previous_path() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join(STATE_FILE_NAME);
        let first = dir.path().join("first.toml");
        let second = dir.path().join("second.toml");

        remember(&state, &first).unwrap();
        remember(&state, &second).unwrap();

        assert_eq!(remembered(&state), Some(second));
    }
}
