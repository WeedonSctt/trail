//! How the navigation listing is ordered.
//!
//! Owns the sort key ([`SortBy`]), the per-tab settings that select one
//! ([`SortSettings`]), and the comparison itself ([`sort_entries`]). The
//! architecture doc gives the navigation panel the job of ordering the listing;
//! this is that ordering, kept out of `state.rs` so the rules can be read and
//! tested on their own.
//!
//! Every key is derived from data the directory listing already holds:
//! `Entry::file_name` and the `Entry::metadata` that `fs::DirEntry::metadata`
//! produced while the directory was being walked. Nothing here performs I/O, so
//! re-sorting is free and the UI thread never blocks on it.

use std::cmp::Ordering;

use crate::app::state::{Entry, EntryKind};

/// Which property the listing is ordered by.
///
/// Parsed from `[navigation] sort_by`, `:sort <key>` and `:set sort_by <key>`;
/// spelled back with [`SortBy::as_str`] so the config, the command and the
/// status-bar notice all use one vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    /// File name, case-insensitive. The default, and Trail's only order before
    /// v1.8.0.
    #[default]
    Name,
    /// Size in bytes, largest first.
    Size,
    /// Modification time, most recent first.
    Modified,
    /// File extension, case-insensitive, then name.
    Extension,
}

impl SortBy {
    /// Parses the spelling used in config and commands, or `None` if it names
    /// no known key.
    ///
    /// Case-insensitive, and `time` is accepted as an alias for `modified`
    /// because that is what the `st` binding is named after.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "name" => Some(Self::Name),
            "size" => Some(Self::Size),
            "modified" | "time" => Some(Self::Modified),
            "extension" | "ext" => Some(Self::Extension),
            _ => None,
        }
    }

    /// The canonical spelling of this key, as `[navigation] sort_by` takes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Modified => "modified",
            Self::Extension => "extension",
        }
    }

    /// A short spelling for the navigation panel's sort badge.
    ///
    /// Four characters at most, because the badge shares the panel's top border
    /// with the directory name.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Modified => "time",
            Self::Extension => "ext",
        }
    }

    /// Whether this key reads best from its high end down.
    ///
    /// Size and time do: you went looking for the biggest or the newest, so
    /// that end belongs at the top, which is what `ls -S` and `ls -t` do. Name
    /// and extension run A–Z.
    ///
    /// This is the single definition of the natural direction —
    /// [`sort_entries`] orders by it and the badge reports it, so the two
    /// cannot come to disagree about which way a listing runs.
    pub fn descends_by_default(self) -> bool {
        matches!(self, Self::Size | Self::Modified)
    }
}

/// Explanation attached to a rejected `sort_by` value.
pub const SORT_BY_REASON: &str = "must be name, size, modified or extension";

/// One tab's ordering of its listing.
///
/// Held per tab rather than per application: a tab is a place you are working,
/// and the order that suits a source tree ("name") is not the one that suits a
/// downloads folder ("modified"). [`crate::app::tabs::TabState`] carries a copy
/// and `AppState` mirrors the active tab's, the same way `cwd` and `selected`
/// are mirrored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSettings {
    /// Which property the listing is ordered by.
    pub by: SortBy,
    /// Whether the order within each group is flipped.
    pub reverse: bool,
    /// Whether directories are grouped ahead of files regardless of `by`.
    pub dirs_first: bool,
}

impl Default for SortSettings {
    /// Trail's ordering before v1.8.0: directories first, then name,
    /// case-insensitive.
    fn default() -> Self {
        Self {
            by: SortBy::Name,
            reverse: false,
            dirs_first: true,
        }
    }
}

impl SortSettings {
    /// Whether values descend as the listing is read downwards.
    ///
    /// The direction the badge's arrow reports: `true` means the largest, newest
    /// or last-alphabetically entry is at the top. `reverse` flips whatever
    /// [`SortBy::descends_by_default`] says.
    pub fn is_descending(self) -> bool {
        self.by.descends_by_default() != self.reverse
    }

    /// A one-line description for the status bar, e.g. `sort: size, reversed`.
    pub fn describe(self) -> String {
        let mut out = format!("sort: {}", self.by.as_str());
        if self.reverse {
            out.push_str(", reversed");
        }
        if !self.dirs_first {
            out.push_str(", mixed with directories");
        }
        out
    }
}

/// Orders `entries` in place according to `settings`.
///
/// The rules, in the order they are applied:
///
/// 1. **Directories first**, when `settings.dirs_first`. The grouping is an
///    axis of its own and `reverse` does not flip it — reversing a listing is
///    asking for the files in the other order, not for the directories to move
///    to the bottom.
/// 2. **The key**, from `settings.by`. `Size` and `Modified` read largest-first
///    and newest-first, the way `ls -S` and `ls -t` do, because that is the end
///    of the range you went looking for. `reverse` flips whichever order is
///    active.
/// 3. **Name, case-insensitive**, as the tie-break. Without it the order would
///    be arbitrary exactly where ties are commonest — every empty file shares a
///    size, and half a working tree shares an mtime after a checkout — and an
///    arbitrary order that changes between refreshes reads as a bug.
///
/// Two cases are deliberately not ranked by their key:
///
/// - **Directories in a size sort** keep name order. `Metadata::len` on a
///   directory is the size of its directory record, not of what is in it, so
///   ranking by it scatters directories through the list on a number that means
///   nothing.
/// - **An entry that could not be stat'd** sorts to the end of its group in a
///   `Size` or `Modified` sort, and stays there under `reverse`. It has no key
///   to rank; the point is to keep it out of the way rather than to give it a
///   position.
///
/// Symlink size and time are the link's own, not its target's, because the
/// listing's metadata comes from an `lstat` — following them would mean I/O per
/// entry on the UI thread and a cycle to guard against.
pub fn sort_entries(entries: &mut [Entry], settings: SortSettings) {
    entries.sort_by(|a, b| compare(a, b, settings));
}

/// Orders one pair of entries. Split out from [`sort_entries`] so the rules can
/// be exercised directly.
fn compare(a: &Entry, b: &Entry, settings: SortSettings) -> Ordering {
    if settings.dirs_first {
        let a_dir = a.kind == EntryKind::Dir;
        let b_dir = b.kind == EntryKind::Dir;
        match (a_dir, b_dir) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
    }

    // A directory's byte length says nothing about its contents, so two of them
    // fall back to the name rather than being ranked on it.
    let by = if settings.by == SortBy::Size && a.kind == EntryKind::Dir && b.kind == EntryKind::Dir
    {
        SortBy::Name
    } else {
        settings.by
    };

    if matches!(by, SortBy::Size | SortBy::Modified) {
        match (a.metadata.is_some(), b.metadata.is_some()) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
    }

    let key = match by {
        // The name is the tie-break below, so a name sort has no key of its own.
        SortBy::Name => Ordering::Equal,
        SortBy::Size => size_of(a).cmp(&size_of(b)),
        SortBy::Modified => modified_of(a).cmp(&modified_of(b)),
        SortBy::Extension => extension_of(a).cmp(&extension_of(b)),
    };

    // Size and time read from their high end down; see
    // `SortBy::descends_by_default`, which the panel's badge reads too.
    let key = if by.descends_by_default() {
        key.reverse()
    } else {
        key
    };

    let ord = key.then_with(|| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()));

    if settings.reverse {
        ord.reverse()
    } else {
        ord
    }
}

/// The entry's size in bytes, or `0` when it could not be stat'd.
///
/// A missing size never reaches a comparison against a present one — the
/// caller has already separated them — so the fallback only ever orders two
/// unknowns against each other, where the name tie-break decides.
fn size_of(entry: &Entry) -> u64 {
    entry.metadata.as_ref().map(|m| m.len()).unwrap_or(0)
}

/// The entry's modification time, or `None` when the platform will not say.
///
/// `Metadata::modified` is `Err` on the rare filesystem that does not record
/// one; such an entry sorts with the other unknowns rather than at an arbitrary
/// time.
fn modified_of(entry: &Entry) -> Option<std::time::SystemTime> {
    entry.metadata.as_ref().and_then(|m| m.modified().ok())
}

/// The entry's extension, lowercased, or the empty string when it has none.
///
/// `Path::extension` reports `None` for a leading-dot name such as
/// `.gitignore`, so dotfiles group with the extensionless rather than forming a
/// `gitignore` group of one.
fn extension_of(entry: &Entry) -> String {
    entry
        .path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_every_canonical_spelling() {
        for key in ["name", "size", "modified", "extension"] {
            let parsed = SortBy::parse(key).expect("canonical spelling parses");
            assert_eq!(parsed.as_str(), key, "round trip for {key}");
        }
    }

    #[test]
    fn parse_accepts_aliases_and_case() {
        assert_eq!(SortBy::parse("TIME"), Some(SortBy::Modified));
        assert_eq!(SortBy::parse(" ext "), Some(SortBy::Extension));
        assert_eq!(SortBy::parse("Name"), Some(SortBy::Name));
    }

    #[test]
    fn parse_rejects_an_unknown_key() {
        assert_eq!(SortBy::parse("created"), None);
        assert_eq!(SortBy::parse(""), None);
    }

    #[test]
    fn default_is_the_pre_1_8_order() {
        let settings = SortSettings::default();
        assert_eq!(settings.by, SortBy::Name);
        assert!(!settings.reverse);
        assert!(settings.dirs_first);
    }

    #[test]
    fn size_and_time_descend_by_default_and_the_others_do_not() {
        assert!(SortBy::Size.descends_by_default());
        assert!(SortBy::Modified.descends_by_default());
        assert!(!SortBy::Name.descends_by_default());
        assert!(!SortBy::Extension.descends_by_default());
    }

    #[test]
    fn reverse_flips_the_reported_direction_for_every_key() {
        for by in [
            SortBy::Name,
            SortBy::Size,
            SortBy::Modified,
            SortBy::Extension,
        ] {
            let forward = SortSettings {
                by,
                reverse: false,
                dirs_first: true,
            };
            let flipped = SortSettings {
                reverse: true,
                ..forward
            };
            assert_eq!(
                forward.is_descending(),
                by.descends_by_default(),
                "{by:?} unreversed should report its natural direction"
            );
            assert_ne!(
                forward.is_descending(),
                flipped.is_descending(),
                "{by:?} reversed should report the other direction"
            );
        }
    }

    #[test]
    fn short_labels_stay_inside_the_badge_budget() {
        for by in [
            SortBy::Name,
            SortBy::Size,
            SortBy::Modified,
            SortBy::Extension,
        ] {
            let label = by.short_label();
            assert!(
                (1..=4).contains(&label.chars().count()),
                "{by:?} label {label:?} does not fit the badge"
            );
        }
    }

    #[test]
    fn describe_names_only_what_is_not_default() {
        assert_eq!(SortSettings::default().describe(), "sort: name");
        let reversed = SortSettings {
            reverse: true,
            ..SortSettings::default()
        };
        assert_eq!(reversed.describe(), "sort: name, reversed");
    }
}
