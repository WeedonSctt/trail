//! Theme resolution: maps TOML `[theme]` configuration to ratatui `Style` objects.
//!
//! Corresponds to the architecture doc's configurable themes mechanism.

use ratatui::style::{Color, Modifier, Style};

use crate::config::ThemeConfig;

/// Resolved ratatui styles derived from [`ThemeConfig`].
#[derive(Debug, Clone)]
pub struct ThemeStyles {
    /// Default text style.
    pub normal: Style,
    /// Block border style.
    pub border: Style,
    /// Highlight style for selected list rows.
    pub selection: Style,
    /// Directory entry style.
    pub directory: Style,
    /// Symlink entry style.
    pub symlink: Style,
    /// Hidden entry style.
    pub hidden: Style,
    /// Status bar secondary text style.
    pub status: Style,
    /// Error message style.
    pub error: Style,
    /// Search accent style.
    pub search: Style,
    /// Command accent style.
    pub command: Style,
    /// Clean git indicator style.
    pub git_clean: Style,
    /// Dirty git indicator style.
    pub git_dirty: Style,
}

/// Resolves configured color strings into ratatui styles.
pub fn resolve(theme: &ThemeConfig) -> ThemeStyles {
    let foreground = parse_color(&theme.foreground);
    let background = parse_color(&theme.background);
    let selection_fg = parse_color(&theme.selection_fg);
    let selection_bg = parse_color(&theme.selection_bg);

    ThemeStyles {
        normal: Style::default().fg(foreground).bg(background),
        border: Style::default().fg(parse_color(&theme.border)),
        selection: Style::default()
            .fg(selection_fg)
            .bg(selection_bg)
            .add_modifier(Modifier::BOLD),
        directory: Style::default()
            .fg(parse_color(&theme.directory))
            .add_modifier(Modifier::BOLD),
        symlink: Style::default().fg(parse_color(&theme.symlink)),
        hidden: Style::default()
            .fg(parse_color(&theme.hidden))
            .add_modifier(Modifier::DIM),
        status: Style::default().fg(parse_color(&theme.status_fg)),
        error: Style::default()
            .fg(parse_color(&theme.error))
            .add_modifier(Modifier::BOLD),
        search: Style::default().fg(parse_color(&theme.search)),
        command: Style::default().fg(parse_color(&theme.command)),
        git_clean: Style::default()
            .fg(parse_color(&theme.git_clean))
            .add_modifier(Modifier::BOLD),
        git_dirty: Style::default()
            .fg(parse_color(&theme.git_dirty))
            .add_modifier(Modifier::BOLD),
    }
}

/// Parses a TOML color value into a ratatui [`Color`].
///
/// Named colors are case-insensitive and accept `dark_gray` or `darkgray`.
/// Hex values use `#rrggbb`. Unknown values fall back to white so rendering
/// remains usable even after a bad runtime `:set theme.*` value.
pub fn parse_color(value: &str) -> Color {
    match value.trim().to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "dark_gray" | "dark_grey" | "darkgray" | "darkgrey" => Color::DarkGray,
        "white" => Color::White,
        "reset" => Color::Reset,
        hex if hex.len() == 7 && hex.starts_with('#') => {
            let r = u8::from_str_radix(&hex[1..3], 16);
            let g = u8::from_str_radix(&hex[3..5], 16);
            let b = u8::from_str_radix(&hex[5..7], 16);
            match (r, g, b) {
                (Ok(r), Ok(g), Ok(b)) => Color::Rgb(r, g, b),
                _ => Color::White,
            }
        }
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_and_hex_colors() {
        assert_eq!(parse_color("dark_gray"), Color::DarkGray);
        assert_eq!(parse_color("#112233"), Color::Rgb(17, 34, 51));
    }
}
