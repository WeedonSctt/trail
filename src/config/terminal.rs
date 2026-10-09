//! The `[terminal]` config section: shell profiles, the panel's height, and
//! when ending a shell needs a confirmation.
//!
//! Kept beside `schema.rs` rather than in it — the section's validation is
//! its own, and `schema.rs` was already past the size a file should be.

use serde::Deserialize;

use super::schema::{invalid_value, SetConfigError};

/// Terminal panel settings — see `docs/terminal_panel.md`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TerminalConfig {
    /// Name of the profile a new shell uses. Blank means the first profile, or
    /// the platform's own shell when there are none.
    pub default_profile: String,
    /// The panel's height, as a percentage of the area above the status bar.
    pub height: u16,
    /// When quitting Trail asks before ending the panel's shells:
    /// `when_busy`, `always` or `never`.
    ///
    /// Parsed by [`crate::terminal::ConfirmPolicy::parse`].
    pub confirm_quit: String,
    /// When `:term close` asks before ending a shell; same values.
    pub confirm_close: String,
    /// Named shells a new panel shell can run.
    pub profile: Vec<TerminalProfile>,
}

impl TerminalConfig {
    /// Whether `name` is blank (the default) or names a configured profile.
    #[must_use]
    pub fn has_profile(&self, name: &str) -> bool {
        name.trim().is_empty()
            || self
                .profile
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(name.trim()))
    }

    pub(super) fn validate(&self) -> Result<(), SetConfigError> {
        if !TERMINAL_HEIGHT_RANGE.contains(&self.height) {
            return Err(invalid_value(
                "terminal.height",
                &self.height.to_string(),
                TERMINAL_HEIGHT_REASON,
            ));
        }
        parse_confirm("terminal.confirm_quit", &self.confirm_quit)?;
        parse_confirm("terminal.confirm_close", &self.confirm_close)?;
        for (index, profile) in self.profile.iter().enumerate() {
            let key = format!("terminal.profile[{}]", index + 1);
            if profile.name.trim().is_empty() {
                return Err(invalid_value(&key, "", "needs a name"));
            }
            if profile
                .command
                .first()
                .map_or(true, |p| p.trim().is_empty())
            {
                return Err(invalid_value(
                    &key,
                    &profile.name,
                    "needs a command, e.g. command = [\"pwsh.exe\", \"-NoLogo\"]",
                ));
            }
            if self.profile[..index]
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(&profile.name))
            {
                return Err(invalid_value(&key, &profile.name, "is a duplicate name"));
            }
        }
        if !self.has_profile(&self.default_profile) {
            return Err(invalid_value(
                "terminal.default_profile",
                &self.default_profile,
                "names no [[terminal.profile]]",
            ));
        }
        Ok(())
    }
}

/// One `[[terminal.profile]]`: a name and the program it runs.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TerminalProfile {
    /// Shown on the panel's tab, and what `:term new <name>` takes.
    pub name: String,
    /// The program followed by its arguments, run directly — not through a
    /// shell, so nothing in it is expanded.
    pub command: Vec<String>,
}

/// Accepted range for `[terminal] height`, in percent.
pub(super) const TERMINAL_HEIGHT_RANGE: std::ops::RangeInclusive<u16> = 10..=90;
pub(super) const TERMINAL_HEIGHT_REASON: &str = "must be a percentage between 10 and 90";

pub(super) fn parse_confirm(key: &str, value: &str) -> Result<String, SetConfigError> {
    crate::terminal::ConfirmPolicy::parse(value)
        .ok_or_else(|| invalid_value(key, value, crate::terminal::CONFIRM_REASON))?;
    Ok(value.trim().to_ascii_lowercase())
}

pub(super) fn parse_terminal_chord(key: &str, value: &str) -> Result<(), SetConfigError> {
    crate::terminal::keys::KeyChord::parse(value)
        .map(|_| ())
        .ok_or_else(|| invalid_value(key, value, crate::terminal::keys::KEY_CHORD_REASON))
}
