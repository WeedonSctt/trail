//! Plugin system: Lua scripting surface via `mlua`.
//!
//! Provides hooks (`on_select`, `on_enter_dir`, `register_action`) for
//! user-defined commands and custom preview providers. In v1 the scripting
//! engine is Lua (`mlua` with embedded Lua 5.4); WASM/`extism` is deferred
//! to v2 (decision log).
//!
//! The bookmark store (`bookmarks`) is a pure-Rust companion module that
//! the example bookmarks plugin delegates to for persistence.

pub mod bookmarks;
pub mod lua_api;

// clippy: unused_imports — API consumed by plugin developers
#[allow(unused_imports)]
pub use lua_api::{PluginEngine, PluginError};

/// The example bookmarks plugin, embedded as a Lua source string so that no
/// external file is required.
///
/// This plugin is automatically available; enable it by adding `"bookmarks"`
/// to `[plugins].enabled` in `trail.toml`.
pub const EXAMPLE_BOOKMARKS_PLUGIN: &str = include_str!("example_bookmarks.lua");

/// Loads all plugins listed in `enabled` into `engine`.
///
/// Each name is resolved in the following order:
/// 1. The built-in embedded plugin (currently only `"bookmarks"`).
/// 2. A file path in the user's Trail config directory, with `.lua` appended.
///
/// Failures to load a single plugin are logged at `debug` level and do not
/// prevent other plugins from loading.
pub fn load_enabled_plugins(engine: &mut PluginEngine, enabled: &[String]) {
    for name in enabled {
        match name.as_str() {
            "bookmarks" => {
                if let Err(e) = engine.load_plugin_str("bookmarks", EXAMPLE_BOOKMARKS_PLUGIN) {
                    tracing::debug!("failed to load built-in bookmarks plugin: {e}");
                } else {
                    tracing::debug!("loaded built-in plugin: bookmarks");
                }
            }
            other => {
                // Try to load as a file from the user's plugin dir.
                let plugin_path =
                    if let Some(dirs) = directories::ProjectDirs::from("", "", "trail") {
                        dirs.config_dir().join(other).with_extension("lua")
                    } else {
                        std::path::PathBuf::from(other).with_extension("lua")
                    };

                if let Err(e) = engine.load_plugin(&plugin_path) {
                    tracing::debug!("failed to load plugin '{other}': {e}");
                } else {
                    tracing::debug!("loaded plugin: {other}");
                }
            }
        }
    }
}
