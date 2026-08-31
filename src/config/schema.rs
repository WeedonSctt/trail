//! Serde structs mirroring the TOML config shape.
//!
//! Covers `[general]`, `[preview]`, `[theme]`, `[keymap]`, and `[plugins]`
//! sections. All structs reject unknown TOML keys so user typos are surfaced
//! instead of silently ignored.

use std::collections::HashMap;

use serde::Deserialize;
use thiserror::Error;

/// Errors produced while applying a runtime `:set` update.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SetConfigError {
    /// The provided config key is not known to the schema.
    #[error("unknown config key '{0}'")]
    UnknownKey(String),
    /// A value could not be parsed as the type required by the key.
    #[error("{key}: invalid value '{value}' ({reason})")]
    InvalidValue {
        /// The key being updated.
        key: String,
        /// The raw value entered by the user.
        value: String,
        /// Human-readable parse or validation reason.
        reason: String,
    },
}

/// Top-level Trail configuration.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TrailConfig {
    /// General behavior settings.
    pub general: GeneralConfig,
    /// Preview pane settings, including inline image rendering.
    pub preview: PreviewConfig,
    /// UI color settings.
    pub theme: ThemeConfig,
    /// Key binding overrides.
    pub keymap: KeymapConfig,
    /// Plugin loader settings.
    pub plugins: PluginsConfig,
}

impl TrailConfig {
    /// Validates semantic constraints that serde cannot express for map keys.
    ///
    /// # Errors
    ///
    /// Returns [`SetConfigError`] if a keymap action name or binding string is
    /// not recognised, or if a color value is invalid.
    pub fn validate(&self) -> Result<(), SetConfigError> {
        if self.general.editor.trim().is_empty() {
            return Err(invalid_value("general.editor", "", "must not be empty"));
        }
        if self.general.text_sync_threshold_kb == 0 {
            return Err(invalid_value(
                "general.text_sync_threshold_kb",
                "0",
                "must be greater than zero",
            ));
        }
        if !self.preview.image_protocol.eq_ignore_ascii_case("auto")
            && crate::preview::graphics::ImageProtocol::parse(&self.preview.image_protocol)
                .is_none()
        {
            return Err(invalid_value(
                "preview.image_protocol",
                &self.preview.image_protocol,
                "must be auto, kitty, iterm2, sixel, halfblocks or none",
            ));
        }
        // The cell size needs no range check: zero means "detect it", and any
        // other u16 is a legitimate, if unusual, cell dimension.
        validate_color_value("theme.foreground", &self.theme.foreground)?;
        validate_color_value("theme.background", &self.theme.background)?;
        validate_color_value("theme.border", &self.theme.border)?;
        validate_color_value("theme.selection_fg", &self.theme.selection_fg)?;
        validate_color_value("theme.selection_bg", &self.theme.selection_bg)?;
        validate_color_value("theme.directory", &self.theme.directory)?;
        validate_color_value("theme.symlink", &self.theme.symlink)?;
        validate_color_value("theme.hidden", &self.theme.hidden)?;
        validate_color_value("theme.status_fg", &self.theme.status_fg)?;
        validate_color_value("theme.error", &self.theme.error)?;
        validate_color_value("theme.search", &self.theme.search)?;
        validate_color_value("theme.command", &self.theme.command)?;
        validate_color_value("theme.git_clean", &self.theme.git_clean)?;
        validate_color_value("theme.git_dirty", &self.theme.git_dirty)?;
        validate_keymap_table("keymap.navigation", &self.keymap.navigation, NAV_ACTIONS)?;
        validate_keymap_table("keymap.search", &self.keymap.search, SEARCH_ACTIONS)?;
        Ok(())
    }

    /// Applies a typed runtime setting update.
    ///
    /// Supports section-qualified keys such as `general.git_status_enabled`
    /// and common short aliases such as `git_status_enabled`.
    ///
    /// # Errors
    ///
    /// Returns [`SetConfigError`] when `key` is unknown or `value` does not
    /// parse for the selected setting.
    pub fn set_value(&mut self, key: &str, value: &str) -> Result<(), SetConfigError> {
        match key {
            "general.editor" | "editor" => {
                let editor = value.trim();
                if editor.is_empty() {
                    return Err(invalid_value(key, value, "must not be empty"));
                }
                self.general.editor = editor.to_owned();
            }
            "general.text_sync_threshold_kb" | "text_sync_threshold_kb" => {
                self.general.text_sync_threshold_kb = parse_positive_usize(key, value)?;
            }
            "general.git_status_enabled" | "git_status_enabled" => {
                self.general.git_status_enabled = parse_bool(key, value)?;
            }
            "general.fs_watch_debounce_ms" | "fs_watch_debounce_ms" => {
                self.general.fs_watch_debounce_ms = parse_u64(key, value)?;
            }
            "preview.image_protocol" | "image_protocol" => {
                if !value.trim().eq_ignore_ascii_case("auto")
                    && crate::preview::graphics::ImageProtocol::parse(value).is_none()
                {
                    return Err(invalid_value(
                        key,
                        value,
                        "must be auto, kitty, iterm2, sixel, halfblocks or none",
                    ));
                }
                self.preview.image_protocol = value.trim().to_ascii_lowercase();
            }
            "preview.image_cell_width" | "image_cell_width" => {
                self.preview.image_cell_width = parse_cell_size(key, value)?;
            }
            "preview.image_cell_height" | "image_cell_height" => {
                self.preview.image_cell_height = parse_cell_size(key, value)?;
            }
            "theme.foreground" => self.theme.foreground = parse_color_value(key, value)?,
            "theme.background" => self.theme.background = parse_color_value(key, value)?,
            "theme.border" => self.theme.border = parse_color_value(key, value)?,
            "theme.selection_fg" => self.theme.selection_fg = parse_color_value(key, value)?,
            "theme.selection_bg" => self.theme.selection_bg = parse_color_value(key, value)?,
            "theme.directory" => self.theme.directory = parse_color_value(key, value)?,
            "theme.symlink" => self.theme.symlink = parse_color_value(key, value)?,
            "theme.hidden" => self.theme.hidden = parse_color_value(key, value)?,
            "theme.status_fg" => self.theme.status_fg = parse_color_value(key, value)?,
            "theme.error" => self.theme.error = parse_color_value(key, value)?,
            "theme.search" => self.theme.search = parse_color_value(key, value)?,
            "theme.command" => self.theme.command = parse_color_value(key, value)?,
            "theme.git_clean" => self.theme.git_clean = parse_color_value(key, value)?,
            "theme.git_dirty" => self.theme.git_dirty = parse_color_value(key, value)?,
            key if key.starts_with("keymap.navigation.") => {
                let action = key.trim_start_matches("keymap.navigation.");
                if !NAV_ACTIONS.contains(&action) {
                    return Err(SetConfigError::UnknownKey(key.to_owned()));
                }
                self.keymap
                    .navigation
                    .insert(action.to_owned(), parse_key_binding(key, value)?);
            }
            key if key.starts_with("keymap.search.") => {
                let action = key.trim_start_matches("keymap.search.");
                if !SEARCH_ACTIONS.contains(&action) {
                    return Err(SetConfigError::UnknownKey(key.to_owned()));
                }
                self.keymap
                    .search
                    .insert(action.to_owned(), parse_key_binding(key, value)?);
            }
            _ => return Err(SetConfigError::UnknownKey(key.to_owned())),
        }

        Ok(())
    }
}

/// General behavior settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeneralConfig {
    /// Editor command used when opening a file.
    pub editor: String,
    /// Maximum file size, in KiB, previewed synchronously on the UI thread.
    pub text_sync_threshold_kb: usize,
    /// Whether git status workers should run.
    pub git_status_enabled: bool,
    /// Filesystem watcher debounce window in milliseconds.
    pub fs_watch_debounce_ms: u64,
}

/// Preview pane settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreviewConfig {
    /// Inline-image protocol to use: `"auto"` to detect from the environment,
    /// or one of `kitty`, `iterm2`, `sixel`, `halfblocks`, `none`.
    pub image_protocol: String,
    /// Width, in pixels, of one terminal character cell, or `0` to detect it.
    ///
    /// Used to convert the preview pane's size in cells into the pixel size an
    /// image is encoded at. Only some platforms report it, so `0` means
    /// "measure it, and assume a conservative default if that fails" — see
    /// `crate::preview::graphics`.
    pub image_cell_width: u16,
    /// Height, in pixels, of one terminal character cell, or `0` to detect it.
    pub image_cell_height: u16,
}

/// UI color configuration.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ThemeConfig {
    /// Default foreground color.
    pub foreground: String,
    /// Default background color.
    pub background: String,
    /// Panel border color.
    pub border: String,
    /// Selected row foreground color.
    pub selection_fg: String,
    /// Selected row background color.
    pub selection_bg: String,
    /// Directory entry color.
    pub directory: String,
    /// Symlink entry color.
    pub symlink: String,
    /// Hidden entry color.
    pub hidden: String,
    /// Status bar foreground color.
    pub status_fg: String,
    /// Error message color.
    pub error: String,
    /// Search mode accent color.
    pub search: String,
    /// Command mode accent color.
    pub command: String,
    /// Clean git indicator color.
    pub git_clean: String,
    /// Dirty git indicator color.
    pub git_dirty: String,
}

/// Configurable key bindings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KeymapConfig {
    /// Navigation-mode bindings by action name.
    pub navigation: HashMap<String, String>,
    /// Search-mode bindings by action name.
    pub search: HashMap<String, String>,
}

/// Plugin loader settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginsConfig {
    /// Plugin names enabled for loading in Phase 8.
    pub enabled: Vec<String>,
}

/// Parses one axis of `[preview]`'s character cell size.
///
/// `0` is accepted, and means "detect it" — see
/// [`crate::preview::graphics::AUTO_CELL_SIZE`].
fn parse_cell_size(key: &str, value: &str) -> Result<u16, SetConfigError> {
    value
        .trim()
        .parse()
        .map_err(|_| invalid_value(key, value, "expected a whole number, or 0 to detect"))
}

fn parse_bool(key: &str, value: &str) -> Result<bool, SetConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(invalid_value(key, value, "expected true or false")),
    }
}

fn parse_positive_usize(key: &str, value: &str) -> Result<usize, SetConfigError> {
    let parsed = value
        .trim()
        .parse::<usize>()
        .map_err(|_| invalid_value(key, value, "expected a positive integer"))?;
    if parsed == 0 {
        return Err(invalid_value(key, value, "must be greater than zero"));
    }
    Ok(parsed)
}

fn parse_u64(key: &str, value: &str) -> Result<u64, SetConfigError> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| invalid_value(key, value, "expected a non-negative integer"))
}

fn parse_color_value(key: &str, value: &str) -> Result<String, SetConfigError> {
    let trimmed = value.trim();
    validate_color_value(key, trimmed)?;
    Ok(trimmed.to_owned())
}

fn parse_key_binding(key: &str, value: &str) -> Result<String, SetConfigError> {
    let trimmed = value.trim();
    validate_key_binding(key, trimmed)?;
    Ok(trimmed.to_owned())
}

fn validate_keymap_table(
    table: &str,
    values: &HashMap<String, String>,
    allowed_actions: &[&str],
) -> Result<(), SetConfigError> {
    for (action, binding) in values {
        if !allowed_actions.contains(&action.as_str()) {
            return Err(SetConfigError::UnknownKey(format!("{table}.{action}")));
        }
        validate_key_binding(&format!("{table}.{action}"), binding)?;
    }
    Ok(())
}

fn validate_key_binding(key: &str, value: &str) -> Result<(), SetConfigError> {
    if value.is_empty() {
        return Err(invalid_value(key, value, "must not be empty"));
    }
    if value.chars().count() == 1 {
        return Ok(());
    }
    let lower = value.to_ascii_lowercase();
    let known = [
        "enter",
        "esc",
        "backspace",
        "tab",
        "left",
        "right",
        "up",
        "down",
    ];
    if known.contains(&lower.as_str()) {
        return Ok(());
    }
    if let Some(rest) = lower.strip_prefix("ctrl-") {
        if rest.chars().count() == 1 {
            return Ok(());
        }
    }
    if value.chars().all(|ch| !ch.is_control()) {
        return Ok(());
    }
    Err(invalid_value(
        key,
        value,
        "expected a printable key, named key, or ctrl-x chord",
    ))
}

fn validate_color_value(key: &str, value: &str) -> Result<(), SetConfigError> {
    let lower = value.trim().to_ascii_lowercase();
    let named = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "gray",
        "grey",
        "dark_gray",
        "dark_grey",
        "darkgray",
        "darkgrey",
        "white",
        "reset",
    ];
    if named.contains(&lower.as_str()) {
        return Ok(());
    }
    if lower.len() == 7
        && lower.starts_with('#')
        && lower[1..].chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return Ok(());
    }
    Err(invalid_value(
        key,
        value,
        "expected a named color or #rrggbb",
    ))
}

fn invalid_value(key: &str, value: &str, reason: &str) -> SetConfigError {
    SetConfigError::InvalidValue {
        key: key.to_owned(),
        value: value.to_owned(),
        reason: reason.to_owned(),
    }
}

const NAV_ACTIONS: &[&str] = &[
    "move_down",
    "move_up",
    "jump_top",
    "jump_bottom",
    "enter_or_open",
    "go_parent",
    "history_back",
    "history_forward",
    "refresh",
    "toggle_hidden",
    "copy_absolute_path",
    "copy_relative_path",
    "copy_filename",
    "copy_content",
    "delete",
    "enter_search",
    "enter_command",
    "quit",
    "open_with_os",
    "new_tab",
    "close_tab",
    "switch_tab_next",
    "switch_tab_prev",
];

const SEARCH_ACTIONS: &[&str] = &["exit", "confirm", "move_down", "move_up", "delete_char"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_value_updates_general_and_keymap_values() {
        let mut config = crate::config::load(None).unwrap();
        config
            .set_value("git_status_enabled", "false")
            .expect("bool setting should parse");
        config
            .set_value("keymap.navigation.move_down", "n")
            .expect("keymap setting should parse");

        assert!(!config.general.git_status_enabled);
        assert_eq!(
            config.keymap.navigation.get("move_down"),
            Some(&"n".to_owned())
        );
    }

    #[test]
    fn set_value_rejects_unknown_keys_and_invalid_values() {
        let mut config = crate::config::load(None).unwrap();

        let unknown = config.set_value("keymap.navigation.fly", "f").unwrap_err();
        assert!(matches!(unknown, SetConfigError::UnknownKey(_)));

        let invalid = config.set_value("theme.directory", "nope").unwrap_err();
        assert!(matches!(invalid, SetConfigError::InvalidValue { .. }));
    }
}
