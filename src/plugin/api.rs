//! The static half of the `trail` Lua table: registration, writes, utilities.
//!
//! Every function here is `'static` — it closes over nothing but the Lua state
//! and talks to the engine through Lua app data: registrations land in
//! `Registry`, write requests in `Queue`, job callbacks in `Jobs`. The
//! read functions (`trail.selection` and friends) need `&AppState` and are
//! installed per call by the engine instead; outside a call they are the stubs
//! `install_read_stubs` puts back. See `docs/plugin_api_plan.md` §3 and §4.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use mlua::{Function, Lua, RegistryKey, Table, Value, Variadic};

use crate::plugin::previewer::{PreviewerRule, DEFAULT_TIMEOUT_MS};
use crate::plugin::request::{CommandSpec, PluginRequest};

/// A registered callback, attributed to the plugin that registered it.
pub(crate) struct Hook {
    /// The registering plugin's name, for error messages.
    pub plugin: String,
    /// The Lua function, kept alive in the Lua registry.
    pub key: RegistryKey,
}

/// A key or sequence a plugin bound to one of its actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The plugin that asked for it.
    pub plugin: String,
    /// The key or sequence, in `[keymap]` spelling: `gv`, `ctrl-g`.
    pub keys: String,
    /// The registered action it runs.
    pub action: String,
    /// The argument passed to the action.
    pub arg: String,
}

/// Everything plugins have registered, in load order.
#[derive(Default)]
pub(crate) struct Registry {
    /// The plugin whose code is running now: set by the engine before every
    /// load and every call, so a registration made at runtime is attributed
    /// to the plugin that made it.
    pub current: String,
    pub on_select: Vec<Hook>,
    pub on_enter_dir: Vec<Hook>,
    pub on_fs_change: Vec<Hook>,
    pub actions: Vec<(String, Hook)>,
    pub bindings: Vec<Binding>,
    pub previewers: Vec<PreviewerRule>,
}

/// Requests queued since the host last drained.
#[derive(Default)]
pub(crate) struct Queue(pub Vec<PluginRequest>);

/// `on_exit` callbacks of jobs still running, by job id.
#[derive(Default)]
pub(crate) struct Jobs {
    pub next_id: u64,
    pub callbacks: HashMap<u64, Hook>,
}

/// The message a read function raises when called outside a hook.
const OUTSIDE_CALL: &str =
    "is only available inside a hook, an action or a job callback, not at load time";

/// The read functions, by name. Kept in one list so the engine installs and
/// removes exactly the same set.
pub(crate) const READ_FUNCTIONS: &[&str] =
    &["selection", "entries", "cwd", "config", "tabs", "mode"];

/// Builds the `trail` global and the app data the engine relies on.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    lua.set_app_data(Registry::default());
    lua.set_app_data(Queue::default());
    lua.set_app_data(Jobs::default());

    let trail = lua.create_table()?;
    install_registration(lua, &trail)?;
    install_writes(lua, &trail)?;
    install_utilities(lua, &trail)?;
    install_read_stubs(lua, &trail)?;
    lua.globals().set("trail", trail)?;
    Ok(())
}

/// Replaces the read functions with stubs that raise a clear error.
///
/// The engine installs scoped versions for the length of one call; after it,
/// a plugin that stashed one would otherwise get mlua's "callback destructed",
/// which says nothing about what the plugin did wrong.
pub(crate) fn install_read_stubs(lua: &Lua, trail: &Table) -> mlua::Result<()> {
    for name in READ_FUNCTIONS {
        let message = format!("trail.{name}() {OUTSIDE_CALL}");
        trail.set(
            *name,
            lua.create_function(move |_, _: Variadic<Value>| -> mlua::Result<()> {
                Err(mlua::Error::RuntimeError(message.clone()))
            })?,
        )?;
    }
    Ok(())
}

fn registry_mut(lua: &Lua) -> mlua::Result<mlua::AppDataRefMut<'_, Registry>> {
    lua.app_data_mut::<Registry>()
        .ok_or_else(|| mlua::Error::RuntimeError("plugin registry missing".to_owned()))
}

/// Queues `request` for the host to apply after the current call returns.
fn push(lua: &Lua, request: PluginRequest) -> mlua::Result<()> {
    lua.app_data_mut::<Queue>()
        .ok_or_else(|| mlua::Error::RuntimeError("plugin queue missing".to_owned()))?
        .0
        .push(request);
    Ok(())
}

fn hook(lua: &Lua, f: Function) -> mlua::Result<Hook> {
    let plugin = registry_mut(lua)?.current.clone();
    Ok(Hook {
        plugin,
        key: lua.create_registry_value(f)?,
    })
}

fn install_registration(lua: &Lua, trail: &Table) -> mlua::Result<()> {
    trail.set(
        "on_select",
        lua.create_function(|lua, f: Function| {
            let h = hook(lua, f)?;
            registry_mut(lua)?.on_select.push(h);
            Ok(())
        })?,
    )?;
    trail.set(
        "on_enter_dir",
        lua.create_function(|lua, f: Function| {
            let h = hook(lua, f)?;
            registry_mut(lua)?.on_enter_dir.push(h);
            Ok(())
        })?,
    )?;
    trail.set(
        "on_fs_change",
        lua.create_function(|lua, f: Function| {
            let h = hook(lua, f)?;
            registry_mut(lua)?.on_fs_change.push(h);
            Ok(())
        })?,
    )?;
    trail.set(
        "on",
        lua.create_function(|lua, (event, f): (String, Function)| {
            let h = hook(lua, f)?;
            let mut registry = registry_mut(lua)?;
            match event.as_str() {
                "select" => registry.on_select.push(h),
                "enter_dir" => registry.on_enter_dir.push(h),
                "fs_change" => registry.on_fs_change.push(h),
                other => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "unknown event '{other}' — expected select, enter_dir or fs_change"
                    )))
                }
            }
            Ok(())
        })?,
    )?;
    trail.set(
        "register_action",
        lua.create_function(|lua, (name, f): (String, Function)| {
            if name.trim().is_empty() || name.contains(char::is_whitespace) {
                return Err(mlua::Error::RuntimeError(format!(
                    "action name '{name}' must be one word"
                )));
            }
            let h = hook(lua, f)?;
            registry_mut(lua)?.actions.push((name, h));
            Ok(())
        })?,
    )?;
    trail.set(
        "bind",
        lua.create_function(
            |lua, (keys, action, arg): (String, String, Option<String>)| {
                validate_keys(&keys)?;
                let mut registry = registry_mut(lua)?;
                let plugin = registry.current.clone();
                registry.bindings.push(Binding {
                    plugin,
                    keys,
                    action,
                    arg: arg.unwrap_or_default(),
                });
                Ok(())
            },
        )?,
    )?;
    trail.set(
        "register_previewer",
        lua.create_function(|lua, spec: Table| {
            let rule = previewer_rule(lua, &spec)?;
            registry_mut(lua)?.previewers.push(rule);
            Ok(())
        })?,
    )?;
    Ok(())
}

/// Rejects a key spelling that could never match a keystroke.
///
/// A key name (`ctrl-g`, `enter`) is one key. Anything else is typed
/// characters, and Navigation Mode's sequences are at most two keys long —
/// the prefix waits for exactly one more — so a longer string could never
/// complete.
fn validate_keys(keys: &str) -> mlua::Result<()> {
    let typed = keys.chars().count();
    let ok = crate::input::keymap::is_named_key(keys)
        || ((1..=2).contains(&typed) && !keys.contains(char::is_whitespace));
    if ok {
        Ok(())
    } else {
        Err(mlua::Error::RuntimeError(format!(
            "'{keys}' is not a key — use one or two characters like \"gv\", or a key name like \"ctrl-g\""
        )))
    }
}

fn previewer_rule(lua: &Lua, spec: &Table) -> mlua::Result<PreviewerRule> {
    let plugin = registry_mut(lua)?.current.clone();
    let name: Option<String> = spec.get("name")?;
    let strings = |field: &str| -> mlua::Result<Vec<String>> {
        let list: Option<Vec<String>> = spec.get(field)?;
        Ok(list.unwrap_or_default())
    };
    let extensions: Vec<String> = strings("extensions")?
        .into_iter()
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
        .collect();
    let names = strings("names")?;
    if extensions.is_empty() && names.is_empty() {
        return Err(mlua::Error::RuntimeError(
            "register_previewer needs `extensions` or `names`".to_owned(),
        ));
    }
    let command = command_spec(spec.get("command")?, "register_previewer `command`")?;
    let timeout_ms: Option<u64> = spec.get("timeout_ms")?;
    Ok(PreviewerRule {
        name: name.unwrap_or_else(|| plugin.clone()),
        plugin,
        extensions,
        names,
        command,
        timeout: Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)),
    })
}

/// Reads a command: a string runs through the shell, a table is an argv.
fn command_spec(value: Value, what: &str) -> mlua::Result<CommandSpec> {
    match value {
        Value::String(s) => Ok(CommandSpec::Shell(s.to_str()?.to_owned())),
        Value::Table(t) => {
            let argv: Vec<String> = t.sequence_values::<String>().collect::<Result<_, _>>()?;
            if argv.is_empty() {
                return Err(mlua::Error::RuntimeError(format!("{what} is empty")));
            }
            Ok(CommandSpec::Argv(argv))
        }
        _ => Err(mlua::Error::RuntimeError(format!(
            "{what} must be a string or a table of strings"
        ))),
    }
}

/// Coerces a config value a plugin passed to the string `:set` parses.
fn value_text(value: Value) -> mlua::Result<String> {
    match value {
        Value::String(s) => Ok(s.to_str()?.to_owned()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        other => Err(mlua::Error::RuntimeError(format!(
            "a config value must be a string, number or boolean, not {}",
            other.type_name()
        ))),
    }
}

fn install_writes(lua: &Lua, trail: &Table) -> mlua::Result<()> {
    // The simple ones: one argument, one request.
    trail.set(
        "navigate",
        lua.create_function(|lua, path: String| {
            push(lua, PluginRequest::Navigate(PathBuf::from(path)))
        })?,
    )?;
    trail.set(
        "select",
        lua.create_function(|lua, target: String| push(lua, PluginRequest::Select(target)))?,
    )?;
    trail.set(
        "move",
        lua.create_function(|lua, n: i64| push(lua, PluginRequest::Move(n)))?,
    )?;
    trail.set(
        "go_parent",
        lua.create_function(|lua, ()| push(lua, PluginRequest::GoParent))?,
    )?;
    trail.set(
        "back",
        lua.create_function(|lua, ()| push(lua, PluginRequest::Back))?,
    )?;
    trail.set(
        "forward",
        lua.create_function(|lua, ()| push(lua, PluginRequest::Forward))?,
    )?;
    trail.set(
        "refresh",
        lua.create_function(|lua, ()| push(lua, PluginRequest::Refresh))?,
    )?;
    trail.set(
        "set_hidden",
        lua.create_function(|lua, show: bool| push(lua, PluginRequest::SetHidden(show)))?,
    )?;
    trail.set(
        "yank",
        lua.create_function(|lua, text: String| push(lua, PluginRequest::Yank(text)))?,
    )?;
    trail.set(
        "command",
        lua.create_function(|lua, line: String| {
            let line = line.trim().trim_start_matches(':').to_owned();
            push(lua, PluginRequest::Command(line))
        })?,
    )?;
    trail.set(
        "open_tab",
        lua.create_function(|lua, path: Option<String>| {
            push(lua, PluginRequest::OpenTab(path.map(PathBuf::from)))
        })?,
    )?;
    trail.set(
        "close_tab",
        lua.create_function(|lua, ()| push(lua, PluginRequest::CloseTab))?,
    )?;
    trail.set(
        "notify",
        lua.create_function(|lua, msg: String| push(lua, PluginRequest::Notify(msg)))?,
    )?;
    trail.set(
        "error",
        lua.create_function(|lua, msg: String| push(lua, PluginRequest::Error(msg)))?,
    )?;
    trail.set(
        "set_status",
        lua.create_function(|lua, text: Option<String>| {
            let text = text.filter(|t| !t.trim().is_empty());
            push(lua, PluginRequest::SetStatus(text))
        })?,
    )?;

    // The ones with structure, validated here so a mistake is an error at the
    // call site rather than a silent no-op later.
    trail.set(
        "set_sort",
        lua.create_function(|lua, spec: Table| {
            let by: Option<String> = spec.get("by")?;
            if let Some(by) = &by {
                if crate::app::sort::SortBy::parse(by).is_none() {
                    return Err(mlua::Error::RuntimeError(format!(
                        "set_sort: '{by}' {}",
                        crate::app::sort::SORT_BY_REASON
                    )));
                }
            }
            push(
                lua,
                PluginRequest::SetSort {
                    by,
                    reverse: spec.get("reverse")?,
                    dirs_first: spec.get("dirs_first")?,
                },
            )
        })?,
    )?;
    trail.set(
        "set_config",
        lua.create_function(|lua, (key, value): (String, Value)| {
            let value = value_text(value)?;
            push(lua, PluginRequest::Command(format!("set {key} {value}")))
        })?,
    )?;
    trail.set(
        "run",
        lua.create_function(|lua, (cmd, opts): (Value, Option<Table>)| {
            let cmd = command_spec(cmd, "run `cmd`")?;
            let pause: Option<String> = match opts {
                Some(o) => o.get("pause")?,
                None => None,
            };
            if let Some(p) = &pause {
                if crate::actions::shell_exec::ShellPause::parse(p).is_none() {
                    return Err(mlua::Error::RuntimeError(format!(
                        "run: pause '{p}' must be always, on_error or never"
                    )));
                }
            }
            push(lua, PluginRequest::Run { cmd, pause })
        })?,
    )?;
    trail.set(
        "spawn",
        lua.create_function(|lua, spec: Table| {
            let cmd = command_spec(spec.get("cmd")?, "spawn `cmd`")?;
            let cwd: Option<String> = spec.get("cwd")?;
            let on_exit: Option<Function> = spec.get("on_exit")?;
            let callback = on_exit.map(|f| hook(lua, f)).transpose()?;
            let id = {
                let mut jobs = lua
                    .app_data_mut::<Jobs>()
                    .ok_or_else(|| mlua::Error::RuntimeError("job table missing".to_owned()))?;
                jobs.next_id += 1;
                let id = jobs.next_id;
                if let Some(cb) = callback {
                    jobs.callbacks.insert(id, cb);
                }
                id
            };
            push(
                lua,
                PluginRequest::Spawn {
                    id,
                    cmd,
                    cwd: cwd.map(PathBuf::from),
                },
            )?;
            Ok(id)
        })?,
    )?;
    Ok(())
}

fn install_utilities(lua: &Lua, trail: &Table) -> mlua::Result<()> {
    trail.set(
        "log",
        lua.create_function(|lua, msg: String| {
            let plugin = registry_mut(lua)?.current.clone();
            tracing::info!(plugin = %plugin, "{msg}");
            Ok(())
        })?,
    )?;
    trail.set(
        "version",
        lua.create_function(|_, ()| Ok(env!("CARGO_PKG_VERSION")))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_spellings_are_checked() {
        assert!(validate_keys("gv").is_ok());
        assert!(validate_keys("ctrl-g").is_ok());
        assert!(validate_keys("shift-end").is_ok());
        assert!(validate_keys("-").is_ok());
        assert!(validate_keys("g-").is_ok());
        assert!(validate_keys("abc").is_err());
        assert!(validate_keys("").is_err());
        assert!(validate_keys("g v").is_err());
        assert!(validate_keys("hyper-x").is_err());
    }

    #[test]
    fn commands_are_strings_or_argv_tables() {
        let lua = Lua::new();
        let s = lua.create_string("echo hi").unwrap();
        assert_eq!(
            command_spec(Value::String(s), "x").unwrap(),
            CommandSpec::Shell("echo hi".to_owned())
        );
        let t = lua.create_sequence_from(["jq", "."]).unwrap();
        assert_eq!(
            command_spec(Value::Table(t), "x").unwrap(),
            CommandSpec::Argv(vec!["jq".to_owned(), ".".to_owned()])
        );
        let empty = lua.create_table().unwrap();
        assert!(command_spec(Value::Table(empty), "x").is_err());
        assert!(command_spec(Value::Integer(1), "x").is_err());
    }
}
