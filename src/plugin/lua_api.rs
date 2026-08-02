//! Lua scripting API (v1, via `mlua`).
//!
//! Exposes `on_select`, `on_enter_dir`, and `register_action` hooks to
//! Lua scripts. Hooks are registered during plugin load; Trail fires them at
//! appropriate runtime moments via the `fire_*` methods.
//!
//! # Hook Contract
//!
//! Each loaded plugin is a Lua chunk executed once at load time. The chunk
//! calls `trail.on_select(fn)`, `trail.on_enter_dir(fn)`, or
//! `trail.register_action(name, fn)` to register callbacks. Trail fires those
//! callbacks by calling `fire_on_select`, `fire_on_enter_dir`, or
//! `fire_action`.
//!
//! Hook errors are logged at `debug` level and never propagate — a
//! misbehaving plugin must not crash the application.
//!
//! # Lifetime notes
//!
//! `mlua::Function` carries a `'lua` lifetime bound to the `Lua` instance.
//! We store callbacks using `mlua::RegistryKey` instead, which is `'static`
//! and keeps the value alive as long as the `Lua` state exists.

use std::path::{Path, PathBuf};

use mlua::{Function, Lua, RegistryKey, Table};
use thiserror::Error;

/// Errors that can arise when loading or running Lua plugins.
#[derive(Debug, Error)]
pub enum PluginError {
    /// A Lua runtime error occurred while loading or executing a plugin.
    #[error("Lua error: {0}")]
    Lua(#[from] mlua::Error),
    /// A plugin file could not be read from disk.
    #[error("failed to read plugin {path}: {source}")]
    Io {
        /// Path of the plugin file.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// The v1 plugin engine: an embedded Lua interpreter with Trail's hook API.
///
/// Holds a single `mlua::Lua` state shared across all loaded plugins.
/// Callbacks are stored as [`mlua::RegistryKey`] values so they remain
/// `'static` while the `Lua` state is alive.
///
/// The hook API is intentionally minimal for v1 — resist expanding it until
/// a real plugin author needs more (coding standard §13 / decision log).
pub struct PluginEngine {
    lua: Lua,
    /// Registry keys for registered `on_select` callbacks, in load order.
    on_select_keys: Vec<RegistryKey>,
    /// Registry keys for registered `on_enter_dir` callbacks, in load order.
    on_enter_dir_keys: Vec<RegistryKey>,
    /// Registry keys for registered custom action callbacks: `(name, key)`.
    registered_action_keys: Vec<(String, RegistryKey)>,
}

impl std::fmt::Debug for PluginEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginEngine")
            .field("on_select_hooks", &self.on_select_keys.len())
            .field("on_enter_dir_hooks", &self.on_enter_dir_keys.len())
            .field("registered_actions", &self.registered_action_keys.len())
            .finish()
    }
}

impl PluginEngine {
    /// Creates a new `PluginEngine` and installs the Trail Lua API surface.
    ///
    /// Sets up the `trail` global table with `on_select`, `on_enter_dir`,
    /// `register_action`, and `log` functions for use by plugins.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::Lua`] if the Lua state cannot be created or the
    /// API table cannot be installed.
    pub fn new() -> Result<Self, PluginError> {
        let lua = Lua::new();
        Self::install_trail_api(&lua)?;

        Ok(Self {
            lua,
            on_select_keys: Vec::new(),
            on_enter_dir_keys: Vec::new(),
            registered_action_keys: Vec::new(),
        })
    }

    /// Installs the `trail` global API table into `lua`.
    ///
    /// Also installs a `__trail_registry` table used internally during plugin
    /// loading to capture hook registrations.
    fn install_trail_api(lua: &Lua) -> Result<(), mlua::Error> {
        // __trail_registry collects registrations during a single load_plugin call.
        let registry = lua.create_table()?;
        registry.set("_on_select", lua.create_table()?)?;
        registry.set("_on_enter_dir", lua.create_table()?)?;
        registry.set("_actions", lua.create_table()?)?;
        lua.globals().set("__trail_registry", registry)?;

        // trail.on_select(fn) — appends fn to _on_select.
        let on_select_fn = lua.create_function(|lua, f: Function| {
            let registry: Table = lua.globals().get("__trail_registry")?;
            let tbl: Table = registry.get("_on_select")?;
            tbl.push(f)?;
            Ok(())
        })?;

        // trail.on_enter_dir(fn) — appends fn to _on_enter_dir.
        let on_enter_dir_fn = lua.create_function(|lua, f: Function| {
            let registry: Table = lua.globals().get("__trail_registry")?;
            let tbl: Table = registry.get("_on_enter_dir")?;
            tbl.push(f)?;
            Ok(())
        })?;

        // trail.register_action(name, fn) — appends {name, fn} pair to _actions.
        let register_action_fn = lua.create_function(|lua, (name, f): (String, Function)| {
            let registry: Table = lua.globals().get("__trail_registry")?;
            let actions: Table = registry.get("_actions")?;
            // Store as a two-element table: {name, fn}.
            let entry = lua.create_table()?;
            entry.set(1, name)?;
            entry.set(2, f)?;
            actions.push(entry)?;
            Ok(())
        })?;

        // trail.log(msg) — emits an info log from plugin code.
        let log_fn = lua.create_function(|_lua, msg: String| {
            tracing::info!(plugin = true, "{}", msg);
            Ok(())
        })?;

        let trail_api = lua.create_table()?;
        trail_api.set("on_select", on_select_fn)?;
        trail_api.set("on_enter_dir", on_enter_dir_fn)?;
        trail_api.set("register_action", register_action_fn)?;
        trail_api.set("log", log_fn)?;
        lua.globals().set("trail", trail_api)?;

        Ok(())
    }

    /// Drains newly registered hooks from `__trail_registry` into the engine's
    /// internal key lists, then resets the registry tables for the next load.
    fn drain_registry(&mut self) -> Result<(), mlua::Error> {
        let registry: Table = self.lua.globals().get("__trail_registry")?;

        // Drain on_select hooks.
        let on_select_tbl: Table = registry.get("_on_select")?;
        for val in on_select_tbl.sequence_values::<Function>() {
            let key = self.lua.create_registry_value(val?)?;
            self.on_select_keys.push(key);
        }
        registry.set("_on_select", self.lua.create_table()?)?;

        // Drain on_enter_dir hooks.
        let on_enter_dir_tbl: Table = registry.get("_on_enter_dir")?;
        for val in on_enter_dir_tbl.sequence_values::<Function>() {
            let key = self.lua.create_registry_value(val?)?;
            self.on_enter_dir_keys.push(key);
        }
        registry.set("_on_enter_dir", self.lua.create_table()?)?;

        // Drain registered actions.
        let actions_tbl: Table = registry.get("_actions")?;
        for val in actions_tbl.sequence_values::<Table>() {
            let entry = val?;
            let name: String = entry.get(1)?;
            let func: Function = entry.get(2)?;
            let key = self.lua.create_registry_value(func)?;
            self.registered_action_keys.push((name, key));
        }
        registry.set("_actions", self.lua.create_table()?)?;

        Ok(())
    }

    /// Loads and executes a Lua plugin from `path`.
    ///
    /// The plugin chunk runs once; any `trail.*` registration calls are
    /// captured and stored as registry keys.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::Io`] if the file cannot be read, or
    /// [`PluginError::Lua`] if the chunk fails to compile or execute.
    pub fn load_plugin(&mut self, path: &Path) -> Result<(), PluginError> {
        let source = std::fs::read_to_string(path).map_err(|e| PluginError::Io {
            path: path.to_owned(),
            source: e,
        })?;
        self.lua
            .load(&source)
            .set_name(path.display().to_string())
            .exec()?;
        self.drain_registry()?;
        tracing::debug!("loaded plugin: {}", path.display());
        Ok(())
    }

    /// Loads a Lua plugin from a source string.
    ///
    /// `name` is used as a debug label in error messages. This is the
    /// preferred path for embedded (built-in) plugins and unit tests.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::Lua`] if the chunk fails to compile or execute.
    pub fn load_plugin_str(&mut self, name: &str, source: &str) -> Result<(), PluginError> {
        self.lua.load(source).set_name(name).exec()?;
        self.drain_registry()?;
        Ok(())
    }

    /// Fires all registered `on_select` hooks with the path of the selected entry.
    ///
    /// Hook errors are logged at `debug` level and do not propagate.
    pub fn fire_on_select(&self, path: &Path) {
        let path_str = path.display().to_string();
        for key in &self.on_select_keys {
            let func: Result<Function, _> = self.lua.registry_value(key);
            match func {
                Ok(f) => {
                    if let Err(e) = f.call::<_, ()>(path_str.clone()) {
                        tracing::debug!("on_select hook error: {e}");
                    }
                }
                Err(e) => tracing::debug!("on_select registry lookup error: {e}"),
            }
        }
    }

    /// Fires all registered `on_enter_dir` hooks with the entered directory path.
    ///
    /// Hook errors are logged at `debug` level and do not propagate.
    pub fn fire_on_enter_dir(&self, dir: &Path) {
        let dir_str = dir.display().to_string();
        for key in &self.on_enter_dir_keys {
            let func: Result<Function, _> = self.lua.registry_value(key);
            match func {
                Ok(f) => {
                    if let Err(e) = f.call::<_, ()>(dir_str.clone()) {
                        tracing::debug!("on_enter_dir hook error: {e}");
                    }
                }
                Err(e) => tracing::debug!("on_enter_dir registry lookup error: {e}"),
            }
        }
    }

    /// Calls the registered handler for `action_name` with `arg`.
    ///
    /// Returns `true` if an action with that name was found and called,
    /// `false` if no plugin registered it.
    /// Handler errors are logged at `debug` level and do not propagate.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn fire_action(&self, action_name: &str, arg: &str) -> bool {
        for (name, key) in &self.registered_action_keys {
            if name == action_name {
                let func: Result<Function, _> = self.lua.registry_value(key);
                match func {
                    Ok(f) => {
                        if let Err(e) = f.call::<_, ()>(arg.to_owned()) {
                            tracing::debug!("action handler '{action_name}' error: {e}");
                        }
                    }
                    Err(e) => tracing::debug!("action registry lookup error: {e}"),
                }
                return true;
            }
        }
        false
    }

    /// Returns an iterator over the names of all registered custom actions.
    ///
    /// Used by the command parser to offer tab-completion for plugin actions.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn action_names(&self) -> impl Iterator<Item = &str> {
        self.registered_action_keys
            .iter()
            .map(|(name, _)| name.as_str())
    }

    /// Returns `true` if no plugins have registered any hooks or actions.
    // clippy: dead_code — API consumed in Phase 9 UI
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.on_select_keys.is_empty()
            && self.on_enter_dir_keys.is_empty()
            && self.registered_action_keys.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_engine_is_empty() {
        let engine = PluginEngine::new().expect("engine");
        assert!(engine.is_empty());
    }

    #[test]
    fn on_select_hook_fires() {
        let mut engine = PluginEngine::new().expect("engine");
        engine
            .load_plugin_str(
                "test_select",
                r#"
trail.on_select(function(path)
    trail.log("selected: " .. path)
end)
"#,
            )
            .expect("load");
        assert_eq!(engine.on_select_keys.len(), 1);
        // Fire should not panic.
        engine.fire_on_select(Path::new("/tmp/test.txt"));
    }

    #[test]
    fn on_enter_dir_hook_fires() {
        let mut engine = PluginEngine::new().expect("engine");
        engine
            .load_plugin_str(
                "test_enter",
                r#"
trail.on_enter_dir(function(dir)
    trail.log("entered: " .. dir)
end)
"#,
            )
            .expect("load");
        assert_eq!(engine.on_enter_dir_keys.len(), 1);
        engine.fire_on_enter_dir(Path::new("/tmp"));
    }

    #[test]
    fn register_action_hook_fires() {
        let mut engine = PluginEngine::new().expect("engine");
        engine
            .load_plugin_str(
                "test_action",
                r#"
trail.register_action("my_action", function(arg)
    trail.log("action with: " .. arg)
end)
"#,
            )
            .expect("load");
        assert_eq!(engine.registered_action_keys.len(), 1);
        assert!(engine.fire_action("my_action", "hello"));
        assert!(!engine.fire_action("nonexistent", ""));
    }

    #[test]
    fn action_names_iterator() {
        let mut engine = PluginEngine::new().expect("engine");
        engine
            .load_plugin_str(
                "test_names",
                r#"
trail.register_action("foo", function() end)
trail.register_action("bar", function() end)
"#,
            )
            .expect("load");
        let names: Vec<_> = engine.action_names().collect();
        assert_eq!(names, vec!["foo", "bar"]);
    }

    #[test]
    fn multiple_plugins_accumulate_hooks() {
        let mut engine = PluginEngine::new().expect("engine");
        engine
            .load_plugin_str("p1", r#"trail.on_select(function(p) end)"#)
            .expect("p1");
        engine
            .load_plugin_str("p2", r#"trail.on_select(function(p) end)"#)
            .expect("p2");
        assert_eq!(engine.on_select_keys.len(), 2);
    }

    #[test]
    fn hook_error_does_not_panic() {
        let mut engine = PluginEngine::new().expect("engine");
        engine
            .load_plugin_str(
                "bad_hook",
                r#"
trail.on_select(function(path)
    error("intentional test error")
end)
"#,
            )
            .expect("load");
        // Fire should not panic even when the Lua hook errors.
        engine.fire_on_select(Path::new("/tmp"));
    }

    #[test]
    fn register_action_unknown_returns_false() {
        let engine = PluginEngine::new().expect("engine");
        assert!(!engine.fire_action("ghost", "arg"));
    }

    #[test]
    fn trail_log_emits_info_log() {
        let mut engine = PluginEngine::new().expect("engine");
        let result = engine.load_plugin_str(
            "test_log",
            r#"
trail.log("hello from plugin log test")
"#,
        );
        assert!(result.is_ok());
    }
}
