//! The Lua plugin engine: loading, firing hooks and actions, and the budget.
//!
//! One `mlua::Lua` state is shared by every plugin. The `trail` table is
//! built by [`crate::plugin::api`]; this module owns the calls *into* Lua.
//! Every call goes through `PluginEngine::invoke`, which does three things
//! around it:
//!
//! 1. **Attributes it.** The running plugin's name is recorded, so anything it
//!    registers or reports carries its name.
//! 2. **Lends it `&AppState`.** The read functions (`trail.selection()` and
//!    friends) are installed as `mlua` scoped functions that borrow the state
//!    for exactly the length of the call, then replaced by stubs. Nothing is
//!    copied until a plugin asks for it.
//! 3. **Budgets it.** An instruction-count hook aborts a call that runs past
//!    `[plugins] budget_ms`, because the call is on the UI thread and
//!    invariant 1 does not stop applying to code Trail did not write.
//!
//! Nothing here mutates `AppState`. Writes are queued (see
//! [`crate::plugin::request`]) and applied by [`crate::plugin::host`] once the
//! call has returned. Design: `docs/plugin_api_plan.md` §3.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Function, HookTriggers, IntoLuaMulti, Lua, MultiValue, Table, Value};
use thiserror::Error;

use crate::app::state::{AppState, Entry};
use crate::plugin::api::{self, Binding, Hook, Jobs, Queue, Registry};
use crate::plugin::previewer::PreviewerRule;
use crate::plugin::read;
use crate::plugin::request::{JobResult, PluginRequest};

/// How often, in Lua VM instructions, the budget is checked.
///
/// Checking reads the clock, so it is not free; every thousand instructions
/// bounds an overrun to well under a millisecond on any machine Trail runs on.
const BUDGET_CHECK_INTERVAL: u32 = 1_000;

/// How many budgets a plugin's top-level chunk gets when it loads.
///
/// Loading happens once, before the first frame, and may legitimately build
/// tables a hook never would.
const LOAD_BUDGET_MULTIPLIER: u32 = 10;

/// The budget used until [`PluginEngine::set_budget`] is called.
pub const DEFAULT_BUDGET: Duration = Duration::from_millis(50);

/// Errors that can arise when loading or running Lua plugins.
#[derive(Debug, Error)]
pub enum PluginError {
    /// A Lua runtime error occurred while loading or executing a plugin.
    #[error("{}", brief(.0))]
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

/// What running a registered action came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionOutcome {
    /// No plugin registered an action by that name.
    NotFound,
    /// The handler succeeded, optionally with a message to show.
    Done(Option<String>),
    /// The handler returned `false` or raised an error.
    Failed(String),
}

/// The plugin engine: an embedded Lua interpreter with Trail's API.
pub struct PluginEngine {
    lua: Lua,
    budget: Duration,
    /// When the running call must finish by; `None` between calls. Shared with
    /// the instruction hook, which is the only thing that reads it.
    deadline: Rc<Cell<Option<Instant>>>,
}

impl std::fmt::Debug for PluginEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("PluginEngine");
        if let Some(r) = self.lua.app_data_ref::<Registry>() {
            s.field("on_select", &r.on_select.len())
                .field("on_enter_dir", &r.on_enter_dir.len())
                .field("on_fs_change", &r.on_fs_change.len())
                .field("actions", &r.actions.len())
                .field("bindings", &r.bindings.len())
                .field("previewers", &r.previewers.len());
        }
        s.field("budget", &self.budget).finish()
    }
}

impl PluginEngine {
    /// Creates an engine with the `trail` API installed and no plugins loaded.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::Lua`] if the API table cannot be installed.
    pub fn new() -> Result<Self, PluginError> {
        let lua = Lua::new();
        api::install(&lua)?;

        let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let watched = Rc::clone(&deadline);
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(BUDGET_CHECK_INTERVAL),
            move |_, _| match watched.get() {
                Some(limit) if Instant::now() > limit => Err(mlua::Error::RuntimeError(
                    "ran past its time budget and was stopped".to_owned(),
                )),
                _ => Ok(()),
            },
        );

        Ok(Self {
            lua,
            budget: DEFAULT_BUDGET,
            deadline,
        })
    }

    /// Sets how long one call into a plugin may run: `[plugins] budget_ms`.
    pub fn set_budget(&mut self, budget: Duration) {
        self.budget = budget;
    }

    /// Loads and runs a plugin file, named after its file stem.
    ///
    /// # Errors
    ///
    /// [`PluginError::Io`] if it cannot be read, [`PluginError::Lua`] if it
    /// fails to compile or run. A plugin that fails part-way through loading
    /// keeps none of what it registered, so a broken plugin is absent rather
    /// than half-present.
    pub fn load_plugin(&mut self, path: &Path) -> Result<(), PluginError> {
        let source = std::fs::read_to_string(path).map_err(|e| PluginError::Io {
            path: path.to_owned(),
            source: e,
        })?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("plugin")
            .to_owned();
        self.load_named(&name, &source, &path.display().to_string())
    }

    /// Loads and runs a plugin from source, under `name`.
    ///
    /// # Errors
    ///
    /// [`PluginError::Lua`] if the chunk fails to compile or run; as with
    /// [`PluginEngine::load_plugin`], nothing it registered is kept.
    pub fn load_plugin_str(&mut self, name: &str, source: &str) -> Result<(), PluginError> {
        self.load_named(name, source, name)
    }

    fn load_named(&mut self, name: &str, source: &str, chunk: &str) -> Result<(), PluginError> {
        self.set_current(name)?;
        self.deadline
            .set(Some(Instant::now() + self.budget * LOAD_BUDGET_MULTIPLIER));
        let result = self.lua.load(source).set_name(chunk).exec();
        self.deadline.set(None);
        if let Err(e) = result {
            self.forget(name);
            return Err(e.into());
        }
        Ok(())
    }

    /// Drops everything `plugin` registered.
    fn forget(&self, plugin: &str) {
        if let Some(mut r) = self.lua.app_data_mut::<Registry>() {
            r.on_select.retain(|h| h.plugin != plugin);
            r.on_enter_dir.retain(|h| h.plugin != plugin);
            r.on_fs_change.retain(|h| h.plugin != plugin);
            r.actions.retain(|(_, h)| h.plugin != plugin);
            r.bindings.retain(|b| b.plugin != plugin);
            r.previewers.retain(|p| p.plugin != plugin);
        }
    }

    fn set_current(&self, plugin: &str) -> mlua::Result<()> {
        let mut r = self
            .lua
            .app_data_mut::<Registry>()
            .ok_or_else(|| mlua::Error::RuntimeError("plugin registry missing".to_owned()))?;
        plugin.clone_into(&mut r.current);
        Ok(())
    }

    // ── Calling into Lua ────────────────────────────────────────────────────

    /// Runs `func` as `plugin`, with the read surface lent over `state` and the
    /// budget armed. The heart of the engine; every call below goes through it.
    fn invoke<'lua>(
        &'lua self,
        state: &AppState,
        plugin: &str,
        func: Function<'lua>,
        args: MultiValue<'lua>,
    ) -> mlua::Result<MultiValue<'lua>> {
        self.set_current(plugin)?;
        let lua = &self.lua;
        let trail: Table = lua.globals().get("trail")?;
        lua.scope(|scope| {
            trail.set(
                "selection",
                scope.create_function(|lua, ()| read::selection(lua, state))?,
            )?;
            trail.set(
                "entries",
                scope.create_function(|lua, ()| read::entries(lua, state))?,
            )?;
            trail.set(
                "cwd",
                scope.create_function(|lua, ()| read::cwd(lua, state))?,
            )?;
            trail.set(
                "config",
                scope.create_function(|lua, key: String| read::config(lua, state, &key))?,
            )?;
            trail.set(
                "tabs",
                scope.create_function(|lua, ()| read::tabs(lua, state))?,
            )?;
            trail.set(
                "mode",
                scope.create_function(|_, ()| Ok(read::mode(state)))?,
            )?;

            self.deadline.set(Some(Instant::now() + self.budget));
            let result = func.call::<_, MultiValue>(args);
            self.deadline.set(None);
            api::install_read_stubs(lua, &trail)?;
            result
        })
    }

    /// Collects the functions of a hook list, releasing the registry borrow
    /// before any of them runs — a hook may register another, which needs it.
    fn hooks(&self, pick: impl Fn(&Registry) -> &Vec<Hook>) -> Vec<(String, Function<'_>)> {
        let Some(r) = self.lua.app_data_ref::<Registry>() else {
            return Vec::new();
        };
        pick(&r)
            .iter()
            .filter_map(|h| {
                self.lua
                    .registry_value::<Function>(&h.key)
                    .ok()
                    .map(|f| (h.plugin.clone(), f))
            })
            .collect()
    }

    fn fire<'lua>(
        &'lua self,
        state: &AppState,
        event: &str,
        hooks: Vec<(String, Function<'lua>)>,
        args: impl Fn(&'lua Lua) -> mlua::Result<MultiValue<'lua>>,
    ) {
        for (plugin, func) in hooks {
            let result = args(&self.lua).and_then(|a| self.invoke(state, &plugin, func, a));
            if let Err(e) = result {
                self.report(&plugin, event, &e);
            }
        }
    }

    /// Queues an error for the status bar, attributed to `plugin`.
    fn report(&self, plugin: &str, what: &str, error: &mlua::Error) {
        let message = format!("plugin {plugin}: {what}: {}", brief(error));
        tracing::warn!("{message}");
        self.queue(PluginRequest::Error(message));
    }

    fn queue(&self, request: PluginRequest) {
        if let Some(mut q) = self.lua.app_data_mut::<Queue>() {
            q.0.push(request);
        }
    }

    /// Fires every `on_select` hook for `entry`.
    ///
    /// Hooks receive the path string, as they always have, and the entry
    /// table as a second argument. Errors are queued, never propagated.
    pub fn fire_on_select(&self, state: &AppState, entry: &Entry) {
        let hooks = self.hooks(|r| &r.on_select);
        if hooks.is_empty() {
            return;
        }
        let path = crate::pathfmt::display(&entry.path);
        self.fire(state, "on_select", hooks, |lua| {
            (path.as_str(), read::entry_table(lua, entry)?).into_lua_multi(lua)
        });
    }

    /// Fires every `on_enter_dir` hook for `dir`, with the `trail.cwd()` table
    /// as the second argument.
    pub fn fire_on_enter_dir(&self, state: &AppState, dir: &Path) {
        let hooks = self.hooks(|r| &r.on_enter_dir);
        if hooks.is_empty() {
            return;
        }
        let path = crate::pathfmt::display(dir);
        self.fire(state, "on_enter_dir", hooks, |lua| {
            (path.as_str(), read::cwd(lua, state)?).into_lua_multi(lua)
        });
    }

    /// Fires every `on_fs_change` hook for `dir`.
    pub fn fire_on_fs_change(&self, state: &AppState, dir: &Path) {
        let hooks = self.hooks(|r| &r.on_fs_change);
        if hooks.is_empty() {
            return;
        }
        let path = crate::pathfmt::display(dir);
        self.fire(state, "on_fs_change", hooks, |lua| {
            path.as_str().into_lua_multi(lua)
        });
    }

    /// Runs the action registered as `name` with `arg`, and says how it went.
    ///
    /// The handler's return value is the outcome: nothing or `true` succeeds
    /// silently, a string succeeds with a notice, `false` (optionally followed
    /// by a reason) fails, and so does a Lua error.
    pub fn fire_action(&self, state: &AppState, name: &str, arg: &str) -> ActionOutcome {
        let found = {
            let Some(r) = self.lua.app_data_ref::<Registry>() else {
                return ActionOutcome::NotFound;
            };
            r.actions.iter().find(|(n, _)| n == name).map(|(_, h)| {
                (
                    h.plugin.clone(),
                    self.lua.registry_value::<Function>(&h.key),
                )
            })
        };
        let Some((plugin, func)) = found else {
            return ActionOutcome::NotFound;
        };
        let result = func
            .and_then(|f| {
                let args = arg.into_lua_multi(&self.lua)?;
                self.invoke(state, &plugin, f, args)
            })
            .map(|values| outcome(values.into_vec()));
        match result {
            Ok(outcome) => outcome,
            Err(e) => ActionOutcome::Failed(brief(&e)),
        }
    }

    /// Calls the `on_exit` callback of job `id` with its result, if it had one.
    pub fn fire_job(&self, state: &AppState, id: u64, result: &JobResult) {
        let hook = self
            .lua
            .app_data_mut::<Jobs>()
            .and_then(|mut jobs| jobs.callbacks.remove(&id));
        let Some(hook) = hook else {
            return;
        };
        let call = self
            .lua
            .registry_value::<Function>(&hook.key)
            .and_then(|f| {
                let t = self.lua.create_table()?;
                t.set("ok", result.ok)?;
                t.set("code", result.code)?;
                t.set("stdout", result.stdout.as_str())?;
                t.set("stderr", result.stderr.as_str())?;
                t.set("id", id)?;
                let args = t.into_lua_multi(&self.lua)?;
                self.invoke(state, &hook.plugin, f, args)
            });
        if let Err(e) = call {
            self.report(&hook.plugin, "job callback", &e);
        }
        let _ = self.lua.remove_registry_value(hook.key);
    }

    // ── What the host reads back ────────────────────────────────────────────

    /// Takes every request queued since the last call, in order.
    pub fn take_requests(&self) -> Vec<PluginRequest> {
        self.lua
            .app_data_mut::<Queue>()
            .map(|mut q| std::mem::take(&mut q.0))
            .unwrap_or_default()
    }

    /// Names of all registered actions, for `:plugin` completion.
    pub fn action_names(&self) -> Vec<String> {
        self.lua
            .app_data_ref::<Registry>()
            .map(|r| r.actions.iter().map(|(n, _)| n.clone()).collect())
            .unwrap_or_default()
    }

    /// Every key binding plugins asked for, in load order.
    pub fn bindings(&self) -> Vec<Binding> {
        self.lua
            .app_data_ref::<Registry>()
            .map(|r| r.bindings.clone())
            .unwrap_or_default()
    }

    /// The action and argument bound to `keys`, if a plugin bound it.
    pub fn binding_for(&self, keys: &str) -> Option<(String, String)> {
        let r = self.lua.app_data_ref::<Registry>()?;
        r.bindings
            .iter()
            .find(|b| b.keys == keys)
            .map(|b| (b.action.clone(), b.arg.clone()))
    }

    /// Whether `ch` starts a multi-key sequence a plugin bound.
    pub fn is_binding_prefix(&self, ch: char) -> bool {
        self.lua.app_data_ref::<Registry>().is_some_and(|r| {
            r.bindings.iter().any(|b| {
                b.keys.len() > ch.len_utf8()
                    && b.keys.starts_with(ch)
                    && !crate::input::keymap::is_named_key(&b.keys)
            })
        })
    }

    /// Every previewer plugins registered, in load order.
    pub fn previewer_rules(&self) -> Vec<PreviewerRule> {
        self.lua
            .app_data_ref::<Registry>()
            .map(|r| r.previewers.clone())
            .unwrap_or_default()
    }
}

/// Reads an action handler's return values as its outcome.
fn outcome(values: Vec<Value>) -> ActionOutcome {
    let mut it = values.into_iter();
    match it.next() {
        None | Some(Value::Nil) | Some(Value::Boolean(true)) => ActionOutcome::Done(None),
        Some(Value::String(s)) => ActionOutcome::Done(Some(s.to_string_lossy().into_owned())),
        Some(Value::Boolean(false)) => ActionOutcome::Failed(match it.next() {
            Some(Value::String(s)) => s.to_string_lossy().into_owned(),
            _ => "failed".to_owned(),
        }),
        Some(_) => ActionOutcome::Done(None),
    }
}

/// One line saying what went wrong, without mlua's traceback.
///
/// Callback errors wrap the error the Rust side raised in layers of context;
/// the status bar has room for the innermost message and nothing else.
pub fn brief(error: &mlua::Error) -> String {
    match error {
        mlua::Error::CallbackError { cause, .. } => brief(cause),
        mlua::Error::RuntimeError(message) | mlua::Error::SyntaxError { message, .. } => {
            message.lines().next().unwrap_or("").trim().to_owned()
        }
        other => other.to_string().lines().next().unwrap_or("").to_owned(),
    }
}
