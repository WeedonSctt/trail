//! How a file's metadata is spelled for a human.
//!
//! The counterpart to [`crate::pathfmt`], which does the same job for paths:
//! one place decides what a size and a timestamp look like, so the listing's
//! details column and the binary preview cannot drift into disagreeing about
//! the same file.
//!
//! Every function here is pure and takes metadata the caller already holds —
//! the directory listing collects it while walking the directory, so nothing
//! in this module performs I/O.

use chrono::{DateTime, Local};

/// What a size or time is shown as when the platform will not report one.
///
/// A visible placeholder rather than a blank: a column that silently goes empty
/// looks like a rendering bug, and "the filesystem did not say" is information.
pub const UNKNOWN: &str = "—";

/// Formats a byte count the way a person reads it: `4.2 kB`, `1.31 MB`.
///
/// Decimal units (kB, MB, GB), matching what the binary preview has always
/// shown and what file managers and browsers agree on for file sizes.
pub fn size(bytes: u64) -> String {
    humansize::format_size(bytes, humansize::DECIMAL)
}

/// Formats a modification time as `YYYY-MM-DD HH:MM` in the local timezone.
///
/// Returns [`UNKNOWN`] when the platform does not record one, which some
/// filesystems do not.
pub fn modified(time: Option<std::time::SystemTime>) -> String {
    match time {
        Some(t) => {
            let dt: DateTime<Local> = t.into();
            dt.format("%Y-%m-%d %H:%M").to_string()
        }
        None => UNKNOWN.to_owned(),
    }
}

/// The width in columns that [`modified`] always occupies.
///
/// `%Y-%m-%d %H:%M` is fixed-width, so the details column can reserve exactly
/// this much without measuring a value first.
pub const MODIFIED_WIDTH: usize = 16;

/// A compact modification time — `YYYY-MM-DD` — for when the full stamp will
/// not fit.
///
/// The date is the part that distinguishes files; the clock time only
/// distinguishes edits made on the same day, which is the detail worth losing
/// first when the panel is narrow.
pub fn modified_short(time: Option<std::time::SystemTime>) -> String {
    match time {
        Some(t) => {
            let dt: DateTime<Local> = t.into();
            dt.format("%Y-%m-%d").to_string()
        }
        None => UNKNOWN.to_owned(),
    }
}

/// The width in columns that [`modified_short`] always occupies.
pub const MODIFIED_SHORT_WIDTH: usize = 10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_uses_decimal_units() {
        assert_eq!(size(0), "0 B");
        // 1000 bytes is a kilobyte in decimal units; 1024 would be 1.02 kB.
        assert_eq!(size(1000), "1 kB");
    }

    #[test]
    fn modified_without_a_time_is_the_placeholder() {
        assert_eq!(modified(None), UNKNOWN);
        assert_eq!(modified_short(None), UNKNOWN);
    }

    #[test]
    fn modified_widths_match_what_is_formatted() {
        let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        assert_eq!(modified(Some(t)).chars().count(), MODIFIED_WIDTH);
        assert_eq!(
            modified_short(Some(t)).chars().count(),
            MODIFIED_SHORT_WIDTH
        );
    }
}
