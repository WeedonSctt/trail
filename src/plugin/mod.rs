//! Plugin system: Lua scripting surface via `mlua`.
//!
//! Plugins are Lua files named in `[plugins] enabled`. They can react to
//! events, read the session, request changes, bind keys, run background jobs
//! and register previewers — see `docs/plugins.md` for the reference and
//! `docs/plugin_api_plan.md` for the design. The modules split along the
//! mechanism:
//!
//! - [`lua_api`] — the engine: loading, calling into Lua, the time budget.
//! - [`api`] — the `trail` table's registration and write functions.
//! - [`read`] — the read surface, built from `&AppState` on demand.
//! - [`request`] — what a plugin can ask for.
//! - [`host`] — applying those requests after the call returns.
//! - [`previewer`] — declarative, command-backed preview providers.
//!
//! The bookmark store (`bookmarks`) is a pure-Rust companion module that
//! the example bookmarks plugin delegates to for persistence.

pub mod api;
pub mod bookmarks;
pub mod host;
pub mod lua_api;
pub mod previewer;
pub mod read;
pub mod request;

// clippy: unused_imports — `PluginError` is re-exported for the library
// surface; the binary names it only through `PluginEngine`'s signatures.
#[allow(unused_imports)]
pub use lua_api::{ActionOutcome, PluginEngine, PluginError};

/// The example bookmarks plugin, embedded as a Lua source string so that no
/// external file is required.
///
/// This plugin is automatically available; enable it by adding `"bookmarks"`
/// to `[plugins].enabled` in `trail.toml`.
pub const EXAMPLE_BOOKMARKS_PLUGIN: &str = include_str!("example_bookmarks.lua");

/// Loads all plugins listed in `enabled` into `engine`, returning one message
/// per plugin that failed.
///
/// Each name is resolved in the following order:
/// 1. The built-in embedded plugin (currently only `"bookmarks"`).
/// 2. A file in the user's Trail config directory, with `.lua` appended.
///
/// A failing plugin does not stop the others loading. The messages are for
/// the status bar: a plugin the user enabled and that silently is not there
/// looks like a plugin that does nothing, which is the worse failure.
pub fn load_enabled_plugins(engine: &mut PluginEngine, enabled: &[String]) -> Vec<String> {
    let mut failures = Vec::new();
    for name in enabled {
        let result = match name.as_str() {
            "bookmarks" => engine.load_plugin_str("bookmarks", EXAMPLE_BOOKMARKS_PLUGIN),
            other => {
                let plugin_path = match crate::paths::config_dir() {
                    Some(dir) => dir.join(other).with_extension("lua"),
                    None => std::path::PathBuf::from(other).with_extension("lua"),
                };
                engine.load_plugin(&plugin_path)
            }
        };
        match result {
            Ok(()) => tracing::info!("loaded plugin: {name}"),
            Err(e) => {
                tracing::warn!("failed to load plugin '{name}': {e}");
                failures.push(format!("plugin {name} failed to load: {e}"));
            }
        }
    }
    failures
}

/// Describes every plugin key binding that can never fire, because the
/// keymap already uses the key or makes it unreachable, or because it names
/// an action no plugin registered.
///
/// A plugin can claim keys Trail does not use; it cannot take one over (see
/// `docs/plugin_api_plan.md` §4.5). Saying so at startup is what stops that
/// rule from looking like a broken binding.
pub fn dead_bindings(engine: &PluginEngine, keymap: &crate::config::KeymapConfig) -> Vec<String> {
    let actions = engine.action_names();
    let mut dead = Vec::new();
    for binding in engine.bindings() {
        if !actions.contains(&binding.action) {
            dead.push(format!(
                "plugin {}: `{}` is bound to unknown action '{}'",
                binding.plugin, binding.keys, binding.action
            ));
        } else if let Some(reason) = crate::input::keymap::shadowed_by(&binding.keys, keymap) {
            dead.push(format!(
                "plugin {}: `{}` can never fire — {reason}",
                binding.plugin, binding.keys
            ));
        }
    }
    dead
}
