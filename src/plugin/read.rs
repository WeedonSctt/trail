//! The plugin read surface: Lua tables built from `&AppState`.
//!
//! Everything here reads state the UI thread already holds — no I/O — and is
//! called only from the scoped functions [`crate::plugin::lua_api`] installs
//! for the duration of one hook, so building a table costs nothing until a
//! plugin asks for it. Field meanings are documented for plugin authors in
//! `docs/plugin_guide.md`; `docs/plugin_api_plan.md` §4.2 has the reasoning.

use std::time::UNIX_EPOCH;

use mlua::{Lua, Table, Value};

use crate::app::mode::Mode;
use crate::app::state::{AppState, Entry, EntryKind, GitFileStatus};
use crate::pathfmt;

/// The table a plugin sees for one entry.
///
/// `size` is `nil` for a directory: its byte length describes its directory
/// record rather than its contents, the rule the details column follows too.
pub fn entry_table<'lua>(lua: &'lua Lua, entry: &Entry) -> mlua::Result<Table<'lua>> {
    let t = lua.create_table()?;
    t.set("path", pathfmt::display(&entry.path))?;
    t.set("name", entry.file_name.as_str())?;
    t.set(
        "kind",
        match entry.kind {
            EntryKind::File => "file",
            EntryKind::Dir => "dir",
            EntryKind::Symlink => "symlink",
        },
    )?;
    t.set("hidden", entry.is_hidden)?;
    if let Some(meta) = &entry.metadata {
        if entry.kind != EntryKind::Dir {
            t.set("size", meta.len())?;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        t.set("modified", modified)?;
    }
    t.set("git", entry.git_status.as_ref().and_then(git_label))?;
    t.set("is_text", entry.is_text)?;
    Ok(t)
}

/// How a git status is spelled to a plugin; `None` for a clean file, which is
/// the same as having no status to report.
fn git_label(status: &GitFileStatus) -> Option<&'static str> {
    match status {
        GitFileStatus::Untracked => Some("untracked"),
        GitFileStatus::Modified => Some("modified"),
        GitFileStatus::Added => Some("added"),
        GitFileStatus::Deleted => Some("deleted"),
        GitFileStatus::Renamed => Some("renamed"),
        GitFileStatus::Clean => None,
    }
}

/// `trail.selection()`: the selected entry, or `nil` in an empty listing.
pub fn selection<'lua>(lua: &'lua Lua, state: &AppState) -> mlua::Result<Option<Table<'lua>>> {
    state
        .selected_entry()
        .map(|entry| entry_table(lua, entry))
        .transpose()
}

/// `trail.entries()`: the listing the user is looking at, in display order.
///
/// Goes through `filtered_entries`, so it respects hidden files and an active
/// search exactly as the navigation panel does — a plugin sees what the user
/// sees, and an index into this array is an index the user can count to.
pub fn entries<'lua>(lua: &'lua Lua, state: &AppState) -> mlua::Result<Table<'lua>> {
    let list = lua.create_table()?;
    for (_, entry) in state.filtered_entries() {
        list.push(entry_table(lua, entry)?)?;
    }
    Ok(list)
}

/// `trail.cwd()`: the current directory and how it is being shown.
pub fn cwd<'lua>(lua: &'lua Lua, state: &AppState) -> mlua::Result<Table<'lua>> {
    let t = lua.create_table()?;
    t.set("path", pathfmt::display(&state.cwd))?;
    t.set("entry_count", state.status.entry_count)?;
    t.set("show_hidden", state.show_hidden)?;
    let sort = lua.create_table()?;
    sort.set("by", state.sort.by.as_str())?;
    sort.set("reverse", state.sort.reverse)?;
    sort.set("dirs_first", state.sort.dirs_first)?;
    t.set("sort", sort)?;
    if let Some(git) = &state.git {
        t.set("git_branch", git.branch.as_str())?;
        t.set("git_dirty", git.is_dirty)?;
    }
    Ok(t)
}

/// `trail.tabs()`: how many tabs are open and which one is active, 1-based
/// to match Lua's arrays and the `[2/3]` indicator.
pub fn tabs<'lua>(lua: &'lua Lua, state: &AppState) -> mlua::Result<Table<'lua>> {
    let t = lua.create_table()?;
    t.set("count", state.tab_manager.len())?;
    t.set("active", state.tab_manager.active + 1)?;
    Ok(t)
}

/// `trail.mode()`: which mode keystrokes are going to.
pub fn mode(state: &AppState) -> &'static str {
    match state.mode {
        Mode::Navigation => "navigation",
        Mode::Search { .. } => "search",
        Mode::Command { .. } => "command",
    }
}

/// The sections a bare key is looked up in, in order.
///
/// The same order `:set` resolves an alias in, so `trail.config("editor")`
/// and `:set editor …` always mean the same key.
const SECTIONS: &[&str] = &["general", "navigation", "preview", "theme", "plugins"];

/// `trail.config(key)`: any configuration value, by the name `:set` uses.
///
/// Accepts a dotted path (`"general.editor"`, `"keymap.navigation.quit"`) or a
/// bare key, which is looked up in each section in turn. Returns `nil` for a
/// key that does not exist rather than raising, so a plugin can probe for a
/// key a newer Trail added.
///
/// Walks a serialized copy of the config rather than matching on key names,
/// so a key added to the schema is readable here without a second list to
/// keep in step. The copy is made per call; calls are rare and the config is
/// small.
pub fn config<'lua>(lua: &'lua Lua, state: &AppState, key: &str) -> mlua::Result<Value<'lua>> {
    let root = toml::Value::try_from(&state.config)
        .map_err(|e| mlua::Error::RuntimeError(format!("config unavailable: {e}")))?;
    let found = lookup(&root, key);
    match found {
        Some(value) => toml_to_lua(lua, value),
        None => Ok(Value::Nil),
    }
}

fn lookup<'a>(root: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let walk = |start: &'a toml::Value, path: &str| {
        path.split('.')
            .try_fold(start, |node, part| node.as_table()?.get(part))
    };
    if key.contains('.') {
        return walk(root, key);
    }
    SECTIONS
        .iter()
        .find_map(|section| walk(root, &format!("{section}.{key}")))
}

fn toml_to_lua<'lua>(lua: &'lua Lua, value: &toml::Value) -> mlua::Result<Value<'lua>> {
    Ok(match value {
        toml::Value::String(s) => Value::String(lua.create_string(s)?),
        toml::Value::Integer(i) => Value::Integer(*i),
        toml::Value::Float(f) => Value::Number(*f),
        toml::Value::Boolean(b) => Value::Boolean(*b),
        toml::Value::Datetime(d) => Value::String(lua.create_string(d.to_string())?),
        toml::Value::Array(items) => {
            let t = lua.create_table()?;
            for item in items {
                t.push(toml_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        toml::Value::Table(map) => {
            let t = lua.create_table()?;
            for (k, v) in map {
                t.set(k.as_str(), toml_to_lua(lua, v)?)?;
            }
            Value::Table(t)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_value(key: &str) -> Option<toml::Value> {
        let config = crate::config::load(None).expect("defaults load");
        let root = toml::Value::try_from(&config).expect("config serializes");
        lookup(&root, key).cloned()
    }

    #[test]
    fn a_bare_key_finds_its_section() {
        assert_eq!(
            config_value("editor"),
            Some(toml::Value::String("nvim".to_owned()))
        );
        assert_eq!(
            config_value("sort_by"),
            Some(toml::Value::String("name".to_owned()))
        );
    }

    #[test]
    fn a_dotted_key_walks_the_path() {
        assert_eq!(
            config_value("keymap.navigation.quit"),
            Some(toml::Value::String("q".to_owned()))
        );
    }

    #[test]
    fn an_unknown_key_is_none_rather_than_an_error() {
        assert_eq!(config_value("no_such_key"), None);
        assert_eq!(config_value("general.no_such_key"), None);
    }
}
