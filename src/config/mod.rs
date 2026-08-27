//! Configuration loading and schema definitions.
//!
//! TOML config loaded once at startup, resolved by the input handler and
//! theme module. Strict-mode deserialization rejects unknown keys.

pub mod last_used;
pub mod schema;

use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

pub use last_used::ConfigSource;
pub use schema::{
    GeneralConfig, KeymapConfig, PluginsConfig, SetConfigError, ThemeConfig, TrailConfig,
};

/// Built-in default configuration shipped with the binary.
pub const DEFAULT_CONFIG_TOML: &str = include_str!("default.toml");

/// Errors produced while loading Trail configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The built-in default configuration could not be parsed.
    #[error("built-in default config is invalid: {0}")]
    DefaultToml(String),
    /// A user-supplied config file could not be read.
    #[error("failed to read config {path}: {source}")]
    Read {
        /// Config file path.
        path: PathBuf,
        /// I/O source error.
        #[source]
        source: std::io::Error,
    },
    /// TOML deserialization failed.
    #[error("{source}")]
    Toml {
        /// Optional path for a user-supplied config.
        path: Option<PathBuf>,
        /// Helpful parse error with line/column context.
        source: TomlConfigError,
    },
    /// Config parsed but failed semantic validation.
    #[error("config validation error: {0}")]
    Validation(#[from] SetConfigError),
}

/// Human-readable TOML parse or schema validation error.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct TomlConfigError {
    message: String,
}

/// Loads configuration from built-in defaults and an optional user path.
///
/// User config files are partial overrides layered onto the shipped default
/// config. Unknown keys are still rejected in strict mode.
///
/// # Errors
///
/// Returns [`ConfigError`] if the default config is malformed, the user file
/// cannot be read, or TOML validation fails.
pub fn load(path: Option<&Path>) -> Result<TrailConfig, ConfigError> {
    let mut config = parse_toml(DEFAULT_CONFIG_TOML, None).map_err(|err| match err {
        ConfigError::Toml { source, .. } => ConfigError::DefaultToml(source.to_string()),
        other => other,
    })?;

    match path {
        Some(path) => {
            let content = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
                path: path.to_owned(),
                source,
            })?;
            let overrides = parse_overrides(&content, path)?;
            overrides.apply_to(&mut config);
            config.validate()?;
            Ok(config)
        }
        None => Ok(config),
    }
}

/// Parses a TOML string into a strict [`TrailConfig`].
///
/// # Errors
///
/// Returns [`ConfigError::Toml`] if the document is malformed, has missing
/// required fields, or includes unknown keys.
pub fn parse_toml(content: &str, path: Option<&Path>) -> Result<TrailConfig, ConfigError> {
    let config = toml::from_str::<TrailConfig>(content).map_err(|err| ConfigError::Toml {
        path: path.map(Path::to_owned),
        source: toml_error(content, path, err),
    })?;
    config.validate()?;
    Ok(config)
}

fn parse_overrides(content: &str, path: &Path) -> Result<ConfigOverrides, ConfigError> {
    toml::from_str::<ConfigOverrides>(content).map_err(|err| ConfigError::Toml {
        path: Some(path.to_owned()),
        source: toml_error(content, Some(path), err),
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigOverrides {
    general: Option<GeneralOverrides>,
    theme: Option<ThemeOverrides>,
    keymap: Option<KeymapOverrides>,
    plugins: Option<PluginsOverrides>,
}

impl ConfigOverrides {
    fn apply_to(self, config: &mut TrailConfig) {
        if let Some(general) = self.general {
            general.apply_to(&mut config.general);
        }
        if let Some(theme) = self.theme {
            theme.apply_to(&mut config.theme);
        }
        if let Some(keymap) = self.keymap {
            keymap.apply_to(&mut config.keymap);
        }
        if let Some(plugins) = self.plugins {
            plugins.apply_to(&mut config.plugins);
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneralOverrides {
    editor: Option<String>,
    text_sync_threshold_kb: Option<usize>,
    git_status_enabled: Option<bool>,
    fs_watch_debounce_ms: Option<u64>,
}

impl GeneralOverrides {
    fn apply_to(self, general: &mut GeneralConfig) {
        if let Some(editor) = self.editor {
            general.editor = editor;
        }
        if let Some(text_sync_threshold_kb) = self.text_sync_threshold_kb {
            general.text_sync_threshold_kb = text_sync_threshold_kb;
        }
        if let Some(git_status_enabled) = self.git_status_enabled {
            general.git_status_enabled = git_status_enabled;
        }
        if let Some(fs_watch_debounce_ms) = self.fs_watch_debounce_ms {
            general.fs_watch_debounce_ms = fs_watch_debounce_ms;
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeOverrides {
    foreground: Option<String>,
    background: Option<String>,
    border: Option<String>,
    selection_fg: Option<String>,
    selection_bg: Option<String>,
    directory: Option<String>,
    symlink: Option<String>,
    hidden: Option<String>,
    status_fg: Option<String>,
    error: Option<String>,
    search: Option<String>,
    command: Option<String>,
    git_clean: Option<String>,
    git_dirty: Option<String>,
}

impl ThemeOverrides {
    fn apply_to(self, theme: &mut ThemeConfig) {
        if let Some(value) = self.foreground {
            theme.foreground = value;
        }
        if let Some(value) = self.background {
            theme.background = value;
        }
        if let Some(value) = self.border {
            theme.border = value;
        }
        if let Some(value) = self.selection_fg {
            theme.selection_fg = value;
        }
        if let Some(value) = self.selection_bg {
            theme.selection_bg = value;
        }
        if let Some(value) = self.directory {
            theme.directory = value;
        }
        if let Some(value) = self.symlink {
            theme.symlink = value;
        }
        if let Some(value) = self.hidden {
            theme.hidden = value;
        }
        if let Some(value) = self.status_fg {
            theme.status_fg = value;
        }
        if let Some(value) = self.error {
            theme.error = value;
        }
        if let Some(value) = self.search {
            theme.search = value;
        }
        if let Some(value) = self.command {
            theme.command = value;
        }
        if let Some(value) = self.git_clean {
            theme.git_clean = value;
        }
        if let Some(value) = self.git_dirty {
            theme.git_dirty = value;
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapOverrides {
    navigation: Option<std::collections::HashMap<String, String>>,
    search: Option<std::collections::HashMap<String, String>>,
}

impl KeymapOverrides {
    fn apply_to(self, keymap: &mut KeymapConfig) {
        if let Some(navigation) = self.navigation {
            keymap.navigation.extend(navigation);
        }
        if let Some(search) = self.search {
            keymap.search.extend(search);
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginsOverrides {
    enabled: Option<Vec<String>>,
}

impl PluginsOverrides {
    fn apply_to(self, plugins: &mut PluginsConfig) {
        if let Some(enabled) = self.enabled {
            plugins.enabled = enabled;
        }
    }
}

fn toml_error(content: &str, path: Option<&Path>, err: toml::de::Error) -> TomlConfigError {
    let location = err
        .span()
        .map(|span| line_col(content, span.start))
        .map(|(line, col)| format!("line {line}, column {col}"))
        .unwrap_or_else(|| "unknown location".to_owned());
    let prefix = path
        .map(|p| format!("{}: ", p.display()))
        .unwrap_or_default();
    TomlConfigError {
        message: format!("{prefix}config error at {location}: {err}"),
    }
}

fn line_col(content: &str, byte_idx: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (idx, ch) in content.char_indices() {
        if idx >= byte_idx {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_loads() {
        let cfg = load(None).unwrap();
        assert_eq!(cfg.general.text_sync_threshold_kb, 256);
        assert!(cfg.general.git_status_enabled);
    }

    #[test]
    fn user_config_overrides_defaults_partially() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trail.toml");
        std::fs::write(
            &path,
            r##"
[general]
text_sync_threshold_kb = 128

[theme]
directory = "#112233"

[keymap]
navigation = { move_down = "n" }
"##,
        )
        .unwrap();

        let cfg = load(Some(&path)).unwrap();
        assert_eq!(cfg.general.text_sync_threshold_kb, 128);
        assert_eq!(cfg.general.editor, "nvim");
        assert_eq!(cfg.theme.directory, "#112233");
        assert_eq!(
            cfg.keymap.navigation.get("move_down"),
            Some(&"n".to_owned())
        );
        assert_eq!(cfg.keymap.navigation.get("move_up"), Some(&"k".to_owned()));
    }

    #[test]
    fn unknown_key_is_rejected_with_line_context() {
        let toml = r#"
[general]
editor = "vi"
text_sync_threshold_kb = 256
git_status_enabled = true
fs_watch_debounce_ms = 200
surprise = true

[theme]
foreground = "white"
background = "black"
border = "dark_gray"
selection_fg = "black"
selection_bg = "dark_gray"
directory = "blue"
symlink = "cyan"
hidden = "dark_gray"
status_fg = "dark_gray"
error = "red"
search = "yellow"
command = "cyan"
git_clean = "green"
git_dirty = "yellow"

[keymap]
navigation = {}
search = {}

[plugins]
enabled = []
"#;

        let err = parse_toml(toml, None).unwrap_err().to_string();
        assert!(err.contains("line"));
        assert!(err.contains("surprise"));
    }

    #[test]
    fn semantic_validation_rejects_bad_color() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trail.toml");
        std::fs::write(
            &path,
            r#"
[theme]
directory = "not-a-color"
"#,
        )
        .unwrap();

        let err = load(Some(&path)).unwrap_err().to_string();
        assert!(err.contains("theme.directory"));
        assert!(err.contains("named color"));
    }

    #[test]
    fn semantic_validation_rejects_unknown_keymap_action() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trail.toml");
        std::fs::write(
            &path,
            r#"
[keymap]
navigation = { fly = "f" }
"#,
        )
        .unwrap();

        let err = load(Some(&path)).unwrap_err().to_string();
        assert!(err.contains("keymap.navigation.fly"));
    }
}
