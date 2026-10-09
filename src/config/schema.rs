//! Serde structs mirroring the TOML config shape.
//!
//! Covers `[general]`, `[navigation]`, `[preview]`, `[terminal]`, `[theme]`,
//! `[keymap]`, and `[plugins]` sections. All structs reject unknown TOML keys so user typos are
//! surfaced instead of silently ignored.

use std::collections::HashMap;

use serde::Deserialize;
use thiserror::Error;

use super::terminal::{
    parse_confirm, parse_terminal_chord, TERMINAL_HEIGHT_RANGE, TERMINAL_HEIGHT_REASON,
};
pub use super::terminal::{TerminalConfig, TerminalProfile};

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

/// Explanation attached to a rejected `[general] shell` value.
///
/// Shared by `validate` and `set_value` so a bad spec reads the same whether it
/// came from the config file at startup or from `:set` at runtime.
const SHELL_SPEC_REASON: &str =
    "must be a program optionally followed by flags, e.g. \"pwsh -NoProfile -Command\"; \
     quote a path containing spaces";

/// Explanation attached to a rejected `[general] shell_pause` value.
///
/// Shared by `validate` and `set_value`, for the same reason as
/// [`SHELL_SPEC_REASON`].
const SHELL_PAUSE_REASON: &str = "must be always, on_error or never";

/// Explanation attached to a rejected `[general] delete_mode` value.
const DELETE_MODE_REASON: &str = "must be trash or permanent";

/// Top-level Trail configuration.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TrailConfig {
    /// General behavior settings.
    pub general: GeneralConfig,
    /// Navigation panel settings: listing order and per-entry details.
    pub navigation: NavigationConfig,
    /// Preview pane settings, including inline image rendering.
    pub preview: PreviewConfig,
    /// Terminal panel settings: shell profiles, size and confirmations.
    pub terminal: TerminalConfig,
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
        // Blank is the documented "use the platform default" sentinel, so only
        // a spec the user actually wrote is held to being spawnable.
        if !self.general.shell.trim().is_empty()
            && crate::actions::shell_exec::split_shell_spec(&self.general.shell).is_none()
        {
            return Err(invalid_value(
                "general.shell",
                &self.general.shell,
                SHELL_SPEC_REASON,
            ));
        }
        if crate::actions::shell_exec::ShellPause::parse(&self.general.shell_pause).is_none() {
            return Err(invalid_value(
                "general.shell_pause",
                &self.general.shell_pause,
                SHELL_PAUSE_REASON,
            ));
        }
        if crate::actions::fs_ops::DeleteMode::parse(&self.general.delete_mode).is_none() {
            return Err(invalid_value(
                "general.delete_mode",
                &self.general.delete_mode,
                DELETE_MODE_REASON,
            ));
        }
        if self.general.text_sync_threshold_kb == 0 {
            return Err(invalid_value(
                "general.text_sync_threshold_kb",
                "0",
                "must be greater than zero",
            ));
        }
        if crate::app::sort::SortBy::parse(&self.navigation.sort_by).is_none() {
            return Err(invalid_value(
                "navigation.sort_by",
                &self.navigation.sort_by,
                crate::app::sort::SORT_BY_REASON,
            ));
        }
        if crate::ui::nav_panel::EntryDetails::parse(&self.navigation.entry_details).is_none() {
            return Err(invalid_value(
                "navigation.entry_details",
                &self.navigation.entry_details,
                crate::ui::nav_panel::ENTRY_DETAILS_REASON,
            ));
        }
        // `navigation.scroll_margin` needs no range check: zero is "no margin",
        // and a margin larger than half the pane is capped at render time, where
        // the pane height is known, rather than rejected here, where it is not.
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
        if self.preview.max_lines == 0 || self.preview.max_lines > PREVIEW_MAX_LINES_LIMIT {
            return Err(invalid_value(
                "preview.max_lines",
                &self.preview.max_lines.to_string(),
                "must be between 1 and 100000",
            ));
        }
        self.terminal.validate()?;
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
        for (action, binding) in &self.keymap.search {
            validate_search_binding(&format!("keymap.search.{action}"), binding)?;
        }
        for (action, binding) in &self.keymap.terminal {
            let key = format!("keymap.terminal.{action}");
            if !crate::terminal::TERMINAL_ACTIONS.contains(&action.as_str()) {
                return Err(SetConfigError::UnknownKey(key));
            }
            parse_terminal_chord(&key, binding)?;
        }
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
            "general.shell" | "shell" => {
                // An explicit empty value cannot arrive here — the command
                // parser rejects `:set shell` with no value — so any spec that
                // reaches this point is one the user meant, and must be usable.
                if crate::actions::shell_exec::split_shell_spec(value).is_none() {
                    return Err(invalid_value(key, value, SHELL_SPEC_REASON));
                }
                self.general.shell = value.trim().to_owned();
            }
            "general.shell_pause" | "shell_pause" => {
                if crate::actions::shell_exec::ShellPause::parse(value).is_none() {
                    return Err(invalid_value(key, value, SHELL_PAUSE_REASON));
                }
                self.general.shell_pause = value.trim().to_ascii_lowercase();
            }
            "general.delete_mode" | "delete_mode" => {
                if crate::actions::fs_ops::DeleteMode::parse(value).is_none() {
                    return Err(invalid_value(key, value, DELETE_MODE_REASON));
                }
                self.general.delete_mode = value.trim().to_ascii_lowercase();
            }
            "general.text_sync_threshold_kb" | "text_sync_threshold_kb" => {
                self.general.text_sync_threshold_kb = parse_positive_usize(key, value)?;
            }
            "general.show_version" | "show_version" => {
                self.general.show_version = parse_bool(key, value)?;
            }
            "general.git_status_enabled" | "git_status_enabled" => {
                self.general.git_status_enabled = parse_bool(key, value)?;
            }
            "general.fs_watch_debounce_ms" | "fs_watch_debounce_ms" => {
                self.general.fs_watch_debounce_ms = parse_u64(key, value)?;
            }
            "navigation.sort_by" | "sort_by" => {
                if crate::app::sort::SortBy::parse(value).is_none() {
                    return Err(invalid_value(key, value, crate::app::sort::SORT_BY_REASON));
                }
                self.navigation.sort_by = value.trim().to_ascii_lowercase();
            }
            "navigation.sort_reverse" | "sort_reverse" => {
                self.navigation.sort_reverse = parse_bool(key, value)?;
            }
            "navigation.dirs_first" | "dirs_first" => {
                self.navigation.dirs_first = parse_bool(key, value)?;
            }
            "navigation.entry_details" | "entry_details" => {
                if crate::ui::nav_panel::EntryDetails::parse(value).is_none() {
                    return Err(invalid_value(
                        key,
                        value,
                        crate::ui::nav_panel::ENTRY_DETAILS_REASON,
                    ));
                }
                self.navigation.entry_details = value.trim().to_ascii_lowercase();
            }
            "navigation.scroll_margin" | "scroll_margin" => {
                self.navigation.scroll_margin = parse_usize(key, value)?;
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
            "preview.max_lines" | "max_lines" => {
                let max_lines = parse_positive_usize(key, value)?;
                if max_lines > PREVIEW_MAX_LINES_LIMIT {
                    return Err(invalid_value(key, value, "must be at most 100000"));
                }
                self.preview.max_lines = max_lines;
            }
            "terminal.height" => {
                let height = value
                    .trim()
                    .parse::<u16>()
                    .map_err(|_| invalid_value(key, value, TERMINAL_HEIGHT_REASON))?;
                if !TERMINAL_HEIGHT_RANGE.contains(&height) {
                    return Err(invalid_value(key, value, TERMINAL_HEIGHT_REASON));
                }
                self.terminal.height = height;
            }
            "terminal.confirm_quit" | "confirm_quit" => {
                self.terminal.confirm_quit = parse_confirm(key, value)?;
            }
            "terminal.confirm_close" | "confirm_close" => {
                self.terminal.confirm_close = parse_confirm(key, value)?;
            }
            "terminal.default_profile" | "default_profile" => {
                let name = value.trim();
                if !self.terminal.has_profile(name) {
                    return Err(invalid_value(key, value, "names no [[terminal.profile]]"));
                }
                self.terminal.default_profile = name.to_owned();
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
                let binding = parse_key_binding(key, value)?;
                validate_search_binding(key, &binding)?;
                self.keymap.search.insert(action.to_owned(), binding);
            }
            key if key.starts_with("keymap.terminal.") => {
                let action = key.trim_start_matches("keymap.terminal.");
                if !crate::terminal::TERMINAL_ACTIONS.contains(&action) {
                    return Err(SetConfigError::UnknownKey(key.to_owned()));
                }
                parse_terminal_chord(key, value)?;
                self.keymap
                    .terminal
                    .insert(action.to_owned(), value.trim().to_owned());
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
    /// Shell that runs `!<command>` and `:git`, as a program plus the flags
    /// that make it read a command string — e.g. `pwsh -NoProfile -Command`.
    ///
    /// Blank means "use the platform default", which is
    /// [`crate::actions::shell_exec::DEFAULT_SHELL_ARGV`]. Parsed by
    /// [`crate::actions::shell_exec::split_shell_spec`]; this is not itself a
    /// shell command, so it gets no expansion or pipelines.
    pub shell: String,
    /// When to wait for Enter after a `!<command>` or `:git` finishes, before
    /// Trail takes the screen back: `always`, `on_error` or `never`.
    ///
    /// Parsed by [`crate::actions::shell_exec::ShellPause::parse`].
    pub shell_pause: String,
    /// How much of a file, in KiB, a preview reads before stopping short.
    ///
    /// Named for what it used to decide — which files were highlighted on the UI
    /// thread, which none are now. Renaming it would break every config that sets
    /// it for no behavioural gain, so it keeps the name and has a documented
    /// meaning instead.
    pub text_sync_threshold_kb: usize,
    /// Where `dd` sends the selected entry: `trash` or `permanent`.
    ///
    /// Parsed by [`crate::actions::fs_ops::DeleteMode::parse`].
    pub delete_mode: String,
    /// Whether the preview panel's top border carries Trail's version.
    ///
    /// A diagnostic rather than chrome: it answers "which build is this
    /// window?" without leaving the session, which `trail --version` cannot do
    /// for a process that is already running. Off by default.
    pub show_version: bool,
    /// Whether git status workers should run.
    pub git_status_enabled: bool,
    /// Filesystem watcher debounce window in milliseconds.
    pub fs_watch_debounce_ms: u64,
}

/// Navigation panel settings: how the listing is ordered and what it shows.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NavigationConfig {
    /// Which property the listing is ordered by: `name`, `size`, `modified` or
    /// `extension`.
    ///
    /// Parsed by [`crate::app::sort::SortBy::parse`]. This seeds the first
    /// tab's order only — each tab owns its own from then on, and `:sort`
    /// changes the active tab's without touching the others.
    pub sort_by: String,
    /// Whether the order within each group is flipped.
    pub sort_reverse: bool,
    /// Whether directories are grouped ahead of files whatever `sort_by` says.
    pub dirs_first: bool,
    /// What the listing shows beside each name: `none`, `size`, `modified` or
    /// `both`.
    ///
    /// Parsed by [`crate::ui::nav_panel::EntryDetails::parse`]. Unlike the sort
    /// keys this is not per tab — it describes the panel rather than a place
    /// you are working.
    pub entry_details: String,
    /// Rows kept between the selection and the top or bottom of the pane
    /// while scrolling, like vim's `scrolloff`.
    ///
    /// `0` lets the selection reach either edge. Any value is accepted: the
    /// renderer caps it at half the pane, which keeps the selection centred.
    pub scroll_margin: usize,
}

impl NavigationConfig {
    /// The [`crate::app::sort::SortSettings`] this section describes.
    ///
    /// Falls back to the default order for an unparseable `sort_by`, which
    /// [`TrailConfig::validate`] has already rejected — the fallback exists so
    /// this cannot panic rather than because it is reachable.
    pub fn sort_settings(&self) -> crate::app::sort::SortSettings {
        crate::app::sort::SortSettings {
            by: crate::app::sort::SortBy::parse(&self.sort_by).unwrap_or_default(),
            reverse: self.sort_reverse,
            dirs_first: self.dirs_first,
        }
    }

    /// The [`crate::ui::nav_panel::EntryDetails`] this section describes.
    ///
    /// Falls back to showing nothing for an unparseable value, on the same
    /// reasoning as [`NavigationConfig::sort_settings`].
    pub fn details(&self) -> crate::ui::nav_panel::EntryDetails {
        crate::ui::nav_panel::EntryDetails::parse(&self.entry_details).unwrap_or_default()
    }
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
    /// Maximum number of lines a text preview loads.
    ///
    /// The preview scrolls within this window, and a longer file is marked as
    /// truncated in the pane's footer. Raising it costs the highlight worker
    /// proportionally more time and memory per preview; capped at
    /// [`PREVIEW_MAX_LINES_LIMIT`].
    pub max_lines: usize,
}

/// Upper bound accepted for `[preview] max_lines`.
///
/// A cap is needed because the value decides how much of a file one worker task
/// reads and highlights: a mistyped value must not pin a worker on a multi-
/// gigabyte file. 100 000 lines is far past any file a preview pane is useful
/// for, so it constrains only typos.
pub const PREVIEW_MAX_LINES_LIMIT: usize = 100_000;

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
    /// Terminal panel bindings by action name. Unlike the other tables these
    /// are single chords (`ctrl-.`, `f12`), never sequences.
    pub terminal: HashMap<String, String>,
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

fn parse_usize(key: &str, value: &str) -> Result<usize, SetConfigError> {
    value
        .trim()
        .parse::<usize>()
        .map_err(|_| invalid_value(key, value, "expected a non-negative integer"))
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

/// Rejects a Search Mode binding that would shadow typing.
///
/// Search Mode appends every unmodified character to the query, so binding a
/// bare character to an action would make that character unsearchable. The
/// shipped defaults used to bind `move_down = "j"` and `move_up = "k"`, which
/// meant neither letter could be typed into a search at all.
///
/// Named and modified keys are unaffected, because they are not text: `up`,
/// `down`, `enter`, `esc`, `backspace`, `ctrl-n`.
fn validate_search_binding(key: &str, binding: &str) -> Result<(), SetConfigError> {
    if binding.chars().count() == 1 {
        return Err(invalid_value(
            key,
            binding,
            "must not be a single character - those are typed into the query;              use a named or modified key such as up, down, enter, esc or ctrl-n",
        ));
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
        "home",
        "end",
    ];
    if known.contains(&lower.as_str()) {
        return Ok(());
    }
    if let Some(rest) = lower.strip_prefix("ctrl-") {
        if rest.chars().count() == 1 {
            return Ok(());
        }
    }
    // `shift-<named key>` — shift is only meaningful on keys that are not text;
    // a shifted character arrives as the capital itself (`G`, `J`).
    if let Some(rest) = lower.strip_prefix("shift-") {
        if known.contains(&rest) {
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

pub(super) fn invalid_value(key: &str, value: &str, reason: &str) -> SetConfigError {
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
    "sort_name",
    "sort_size",
    "sort_time",
    "sort_extension",
    "sort_reverse",
    "sort_dirs_first",
    "toggle_details",
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
    "preview_scroll_down",
    "preview_scroll_up",
    "preview_page_down",
    "preview_page_up",
    "preview_scroll_top",
    "preview_scroll_bottom",
];

const SEARCH_ACTIONS: &[&str] = &["exit", "confirm", "move_down", "move_up", "delete_char"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_bindings_reject_bare_characters() {
        let mut config = crate::config::load(None).unwrap();
        config
            .keymap
            .search
            .insert("move_down".to_owned(), "j".to_owned());

        let err = config.validate().unwrap_err();
        assert!(
            format!("{err}").contains("single character"),
            "expected the shadowing explanation, got: {err}"
        );
    }

    #[test]
    fn set_value_rejects_a_bare_character_search_binding() {
        let mut config = crate::config::load(None).unwrap();
        let err = config.set_value("keymap.search.move_up", "k").unwrap_err();
        assert!(format!("{err}").contains("single character"));
        // The rejected binding must not have been applied.
        assert_eq!(config.keymap.search.get("move_up"), Some(&"up".to_owned()));
    }

    #[test]
    fn set_value_accepts_and_bounds_preview_max_lines() {
        let mut config = crate::config::load(None).unwrap();
        config.set_value("preview.max_lines", "8000").unwrap();
        assert_eq!(config.preview.max_lines, 8000);
        // The short alias reaches the same field.
        config.set_value("max_lines", "500").unwrap();
        assert_eq!(config.preview.max_lines, 500);

        for rejected in ["0", "200000"] {
            assert!(
                config.set_value("preview.max_lines", rejected).is_err(),
                "{rejected} must be rejected"
            );
        }
        // A rejected value must not have been applied.
        assert_eq!(config.preview.max_lines, 500);
    }

    #[test]
    fn preview_scroll_bindings_are_configurable() {
        let mut config = crate::config::load(None).unwrap();
        config
            .set_value("keymap.navigation.preview_scroll_down", "J")
            .unwrap();
        config
            .set_value("keymap.navigation.preview_scroll_bottom", "shift-end")
            .unwrap();
        assert_eq!(
            config.keymap.navigation.get("preview_scroll_bottom"),
            Some(&"shift-end".to_owned())
        );
        config.validate().expect("shipped defaults must validate");
    }

    #[test]
    fn search_bindings_accept_named_and_modified_keys() {
        let mut config = crate::config::load(None).unwrap();
        for binding in ["down", "ctrl-n", "enter", "esc"] {
            config
                .set_value("keymap.search.move_down", binding)
                .unwrap_or_else(|e| panic!("{binding} should be allowed: {e}"));
        }
    }

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
    fn the_shipped_shell_default_is_blank_and_means_platform_default() {
        let config = crate::config::load(None).unwrap();
        assert_eq!(config.general.shell, "");
        // Blank must survive validation — it is the sentinel, not a mistake.
        config.validate().expect("a blank shell must be accepted");
    }

    #[test]
    fn set_value_accepts_a_shell_with_flags() {
        let mut config = crate::config::load(None).unwrap();
        config
            .set_value("general.shell", "pwsh -NoProfile -Command")
            .unwrap();
        assert_eq!(config.general.shell, "pwsh -NoProfile -Command");
        // The short alias reaches the same field.
        config.set_value("shell", "bash -c").unwrap();
        assert_eq!(config.general.shell, "bash -c");
        config.validate().expect("a set shell must still validate");
    }

    #[test]
    fn set_value_rejects_an_unspawnable_shell_and_keeps_the_old_one() {
        let mut config = crate::config::load(None).unwrap();
        config.set_value("shell", "bash -c").unwrap();

        for rejected in ["\"pwsh -Command", "\"\""] {
            let err = config.set_value("shell", rejected).unwrap_err();
            assert!(
                matches!(err, SetConfigError::InvalidValue { .. }),
                "{rejected:?} should be an invalid value, got: {err}"
            );
        }
        // A rejected value must not have been applied.
        assert_eq!(config.general.shell, "bash -c");
    }

    #[test]
    fn shell_pause_accepts_the_three_policies_and_rejects_anything_else() {
        let mut config = crate::config::load(None).unwrap();
        assert_eq!(
            config.general.shell_pause, "always",
            "the shipped default holds the screen, so command output can be read"
        );

        for accepted in ["never", "on_error", "ALWAYS"] {
            config
                .set_value("shell_pause", accepted)
                .unwrap_or_else(|e| panic!("{accepted:?} should be accepted: {e}"));
            assert_eq!(config.general.shell_pause, accepted.to_ascii_lowercase());
            config.validate().expect("a set policy must still validate");
        }

        let err = config
            .set_value("general.shell_pause", "sometimes")
            .unwrap_err();
        assert!(
            matches!(err, SetConfigError::InvalidValue { .. }),
            "an unknown policy should be rejected, got: {err}"
        );
        assert_eq!(
            config.general.shell_pause, "always",
            "a rejected value must not be applied"
        );
    }

    #[test]
    fn validate_rejects_an_unknown_shell_pause() {
        let mut config = crate::config::load(None).unwrap();
        config.general.shell_pause = "maybe".to_owned();
        let err = config.validate().unwrap_err();
        assert!(
            format!("{err}").contains("general.shell_pause"),
            "the error must name the key, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_a_shell_with_an_unterminated_quote() {
        let mut config = crate::config::load(None).unwrap();
        config.general.shell = "\"pwsh -Command".to_owned();
        let err = config.validate().unwrap_err();
        assert!(
            format!("{err}").contains("general.shell"),
            "the error must name the key, got: {err}"
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
