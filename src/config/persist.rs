//! `:config write`: saves the session's `:set` changes into a config file.
//!
//! `:set` changes only the running session. Writing it back by serializing the
//! whole config would erase the comments in the user's file, which are the
//! documentation of each trade-off (`upcoming_features.md` §5.1). So this edits
//! the file in place with `toml_edit`: only the keys that were `:set` are
//! touched, and every comment, blank line and key order survives.
//!
//! Each value is written with the TOML type the shipped default config gives
//! that key — `true`, `3`, `"size"` — so the file loads back exactly as typed
//! in by hand would.

use std::path::{Path, PathBuf};

use thiserror::Error;
use toml_edit::{DocumentMut, Item, Table, Value};

use super::DEFAULT_CONFIG_TOML;

/// File name used when there is no config file yet to write into.
pub const CONFIG_FILE_NAME: &str = "trail.toml";

/// Errors from [`write_settings`].
#[derive(Debug, Error)]
pub enum PersistError {
    /// The file could not be read.
    #[error("could not read {path}: {source}")]
    Read {
        /// The config file.
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid TOML, so editing it in place would be guesswork.
    #[error("{path} is not valid TOML, so it was left alone: {source}")]
    Parse {
        /// The config file.
        path: PathBuf,
        #[source]
        source: Box<toml_edit::TomlError>,
    },
    /// The file could not be written.
    #[error("could not write {path}: {source}")]
    Write {
        /// The config file.
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The `:set` changes made this session, and where `:config write` puts them.
#[derive(Debug, Default)]
pub struct SessionSettings {
    /// The config file this session loaded, if any.
    pub file: Option<PathBuf>,
    /// Where to record a newly created config file so the next start loads it
    /// (Trail's `last_config.toml`). `None` in tests, which must not touch the
    /// user's data directory.
    pub remember_in: Option<PathBuf>,
    /// Changed keys as `(section.key, value as typed)`, oldest first, one entry
    /// per key.
    changes: Vec<(String, String)>,
}

impl SessionSettings {
    /// Records a successful `:set key value`.
    ///
    /// `key` may be a short alias (`sort_by`); it is stored under its full
    /// name (`navigation.sort_by`) so one key is written once however it was
    /// spelled. A key with no full name is ignored.
    pub fn record(&mut self, key: &str, value: &str) {
        let Some(full) = canonical_key(key) else {
            return;
        };
        self.changes.retain(|(k, _)| *k != full);
        self.changes.push((full, value.trim().to_owned()));
    }

    /// The changes waiting to be written.
    pub fn changes(&self) -> &[(String, String)] {
        &self.changes
    }

    /// Forgets the changes, once they are written.
    pub fn clear(&mut self) {
        self.changes.clear();
    }
}

/// Short `:set` aliases and the section each belongs to.
///
/// Mirrors the alias arms of `TrailConfig::set_value`; a test sets every one
/// through both spellings to keep the two in step.
const ALIASES: &[(&str, &str)] = &[
    ("editor", "general"),
    ("shell", "general"),
    ("shell_pause", "general"),
    ("delete_mode", "general"),
    ("text_sync_threshold_kb", "general"),
    ("show_version", "general"),
    ("git_status_enabled", "general"),
    ("fs_watch_debounce_ms", "general"),
    ("sort_by", "navigation"),
    ("sort_reverse", "navigation"),
    ("dirs_first", "navigation"),
    ("entry_details", "navigation"),
    ("scroll_margin", "navigation"),
    ("image_protocol", "preview"),
    ("image_cell_width", "preview"),
    ("image_cell_height", "preview"),
    ("max_lines", "preview"),
    ("confirm_quit", "terminal"),
    ("confirm_close", "terminal"),
    ("default_profile", "terminal"),
];

/// The full `section.key` name for a `:set` key, or `None` for a key that has
/// none. A key that already contains a `.` is taken to be full.
pub fn canonical_key(key: &str) -> Option<String> {
    if key.contains('.') {
        return Some(key.to_owned());
    }
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == key)
        .map(|(alias, section)| format!("{section}.{alias}"))
}

/// Where `:config write` writes: the file this session loaded, else
/// [`CONFIG_FILE_NAME`] in `config_dir`.
pub fn target_file(loaded: Option<&Path>, config_dir: Option<&Path>) -> Option<PathBuf> {
    loaded
        .map(Path::to_path_buf)
        .or_else(|| config_dir.map(|dir| dir.join(CONFIG_FILE_NAME)))
}

/// Writes `changes` into the TOML file at `path`, creating it (and its
/// directory) if it does not exist.
///
/// Only the named keys change; everything else in the file, comments
/// included, is left byte for byte. The new content is written to a sibling
/// temporary file and renamed into place, so a failure part-way through never
/// leaves a half-written config.
///
/// # Errors
///
/// Returns [`PersistError`] if the file cannot be read, is not valid TOML, or
/// cannot be written.
pub fn write_settings(path: &Path, changes: &[(String, String)]) -> Result<(), PersistError> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(PersistError::Read {
                path: path.to_owned(),
                source,
            })
        }
    };
    let mut doc: DocumentMut = existing.parse().map_err(|source| PersistError::Parse {
        source: Box::new(source),
        path: path.to_owned(),
    })?;
    let defaults: DocumentMut = DEFAULT_CONFIG_TOML.parse().unwrap_or_default();

    for (key, raw) in changes {
        let (sections, name) = match key.rsplit_once('.') {
            Some(split) => split,
            None => continue,
        };
        let value = typed_value(lookup(&defaults, sections, name), raw);
        if let Some(table) = table_at(doc.as_table_mut(), sections) {
            table[name] = Item::Value(value);
        }
    }

    let write_err = |source| PersistError::Write {
        path: path.to_owned(),
        source,
    };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(write_err)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(write_err)?;
    std::fs::rename(&tmp, path).map_err(write_err)
}

/// The value at `sections.name` in `doc`, if present.
fn lookup<'a>(doc: &'a DocumentMut, sections: &str, name: &str) -> Option<&'a Item> {
    let mut item = doc.as_item();
    for part in sections.split('.') {
        item = item.get(part)?;
    }
    item.get(name)
}

/// The table at `sections` (`keymap.navigation`) under `table`, created if
/// missing. `None` only if `toml_edit` refused a table it was just given.
///
/// A table created here is implicit, so a new `[keymap.navigation]` does not
/// come with an empty `[keymap]` header above it.
fn table_at<'a>(table: &'a mut Table, sections: &str) -> Option<&'a mut Table> {
    let (head, rest) = match sections.split_once('.') {
        Some((head, rest)) => (head, Some(rest)),
        None => (sections, None),
    };
    let entry = table.entry(head).or_insert_with(|| {
        let mut new = Table::new();
        new.set_implicit(true);
        Item::Table(new)
    });
    if !entry.is_table() {
        *entry = Item::Table(Table::new());
    }
    let child = entry.as_table_mut()?;
    match rest {
        Some(rest) => table_at(child, rest),
        None => Some(child),
    }
}

/// `raw` as the TOML type `default` has, or as a string when the default is
/// absent or a string.
///
/// `:set` has already validated `raw`, so a bool here is one of the spellings
/// it accepts and a number parses.
fn typed_value(default: Option<&Item>, raw: &str) -> Value {
    match default.and_then(Item::as_value) {
        Some(Value::Boolean(_)) => {
            let truthy = matches!(
                raw.to_ascii_lowercase().as_str(),
                "true" | "yes" | "on" | "1"
            );
            Value::from(truthy)
        }
        Some(Value::Integer(_)) => match raw.parse::<i64>() {
            Ok(n) => Value::from(n),
            Err(_) => Value::from(raw),
        },
        _ => Value::from(raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_keeps_comments_and_untouched_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trail.toml");
        std::fs::write(
            &path,
            "# my settings\n[general]\n# which editor\neditor = \"nvim\"\n\n[navigation]\nsort_by = \"name\" # tidy\n",
        )
        .unwrap();

        write_settings(
            &path,
            &[
                ("navigation.sort_by".to_owned(), "size".to_owned()),
                ("navigation.scroll_margin".to_owned(), "5".to_owned()),
                ("general.show_version".to_owned(), "yes".to_owned()),
            ],
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my settings"), "{text}");
        assert!(text.contains("# which editor"), "{text}");
        assert!(text.contains("editor = \"nvim\""), "{text}");
        assert!(text.contains("sort_by = \"size\""), "{text}");
        assert!(
            text.contains("scroll_margin = 5"),
            "typed as an integer: {text}"
        );
        assert!(
            text.contains("show_version = true"),
            "typed as a bool: {text}"
        );

        // And it loads back.
        let config = crate::config::load(Some(&path)).unwrap();
        assert_eq!(config.navigation.sort_by, "size");
        assert_eq!(config.navigation.scroll_margin, 5);
        assert!(config.general.show_version);
    }

    #[test]
    fn a_missing_file_is_created_with_just_the_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("trail.toml");
        write_settings(
            &path,
            &[("keymap.navigation.quit".to_owned(), "Q".to_owned())],
        )
        .unwrap();

        let config = crate::config::load(Some(&path)).unwrap();
        assert_eq!(config.keymap.navigation.get("quit"), Some(&"Q".to_owned()));
    }

    #[test]
    fn a_broken_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trail.toml");
        std::fs::write(&path, "[general\neditor = ").unwrap();
        let err = write_settings(&path, &[("general.editor".to_owned(), "vi".to_owned())]);
        assert!(matches!(err, Err(PersistError::Parse { .. })));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[general\neditor = "
        );
    }

    /// `ALIASES` mirrors `set_value`: every alias must reach the same field
    /// as its full name, or `:config write` would put it under the wrong
    /// section.
    #[test]
    fn every_alias_names_the_field_its_full_name_does() {
        let base = crate::config::load(None).unwrap();
        for (alias, section) in ALIASES {
            let full = format!("{section}.{alias}");
            let mut by_alias = base.clone();
            let mut by_full = base.clone();
            // A value the field accepts: whatever it already holds.
            let value = sample_value(&full);
            let alias_result = by_alias.set_value(alias, &value);
            let full_result = by_full.set_value(&full, &value);
            // Some keys reject every sample (`default_profile` needs a
            // profile to exist); what matters is that the full name is a key
            // `set_value` knows, and that both spellings do the same thing.
            assert!(
                !matches!(
                    full_result,
                    Err(crate::config::SetConfigError::UnknownKey(_))
                ),
                "{full} is not a key set_value knows"
            );
            assert_eq!(
                alias_result.is_ok(),
                full_result.is_ok(),
                "{alias} vs {full}"
            );
            assert_eq!(by_alias, by_full, "{alias} vs {full}");
            assert_eq!(canonical_key(alias).as_deref(), Some(full.as_str()));
        }
    }

    /// The default for `full` in the shipped config, as `:set` would type it.
    fn sample_value(full: &str) -> String {
        let defaults: DocumentMut = DEFAULT_CONFIG_TOML.parse().unwrap();
        let (sections, name) = full.rsplit_once('.').unwrap();
        match lookup(&defaults, sections, name).and_then(Item::as_value) {
            Some(Value::String(s)) if s.value().is_empty() => "x".to_owned(),
            Some(Value::String(s)) => s.value().clone(),
            Some(Value::Boolean(b)) => (!*b.value()).to_string(),
            Some(Value::Integer(n)) => (n.value() + 1).to_string(),
            other => panic!("no default for {full}: {other:?}"),
        }
    }

    #[test]
    fn the_target_is_the_loaded_file_else_the_config_dir() {
        let loaded = Path::new("/a/mine.toml");
        let dir = Path::new("/cfg");
        assert_eq!(
            target_file(Some(loaded), Some(dir)),
            Some(loaded.to_path_buf())
        );
        assert_eq!(target_file(None, Some(dir)), Some(dir.join("trail.toml")));
        assert_eq!(target_file(None, None), None);
    }
}
