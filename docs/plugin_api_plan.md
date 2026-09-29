# Trail — Plugin API Plan

The design for widening the Lua plugin surface from "observe and log" to "read the
session, change it, and extend it". The earlier version of this document (at v1.8.3)
settled the core mechanism — plugins *request* `Action`s rather than mutate state — and
left two questions open. This version answers both, adds five capabilities the first
draft did not cover, and is what the `feat/plugin-api` branch implements.

User-facing reference: [`plugin_guide.md`](plugin_guide.md). Triage entry:
[`upcoming_features.md`](upcoming_features.md) §2.14.

---

## 1. Where it started

Four functions: `trail.on_select(fn)`, `trail.on_enter_dir(fn)`,
`trail.register_action(name, fn)` and `trail.log(msg)`. Hooks received a path string
and returned nothing; an action's return value was discarded; errors went to a `debug`
log line below the default filter. A plugin could be told where the user went and do
nothing about it.

---

## 2. Principles

Every decision below follows from these, so they come first.

1. **A plugin can do what the user can do, through the paths the user's keys take.**
   Writes become `Action`s (or Trail commands) applied by `actions::apply`, so the
   notice channel, the dirty flag, the selection anchoring and the per-tab bookkeeping
   happen for free, and there is no second code path to keep correct.
2. **Invariant 1 holds for plugins too.** The UI thread never blocks. Lua runs on it —
   that is unavoidable while hooks can read `&AppState` — so Lua runs under a
   **budget**, and anything slow has a way off the thread that is *easier* than doing it
   inline.
3. **Nothing a plugin does is invisible.** A failed load, a runtime error, a blown
   budget and a returned failure all reach the status bar and the log, attributed to the
   plugin.
4. **Additive only.** Every existing plugin keeps working unchanged. Hooks gain a second
   argument rather than changing the first.
5. **Honest about trust.** See §6.1.

---

## 3. The mechanism

### 3.1 Reads: scoped functions over `&AppState`

A plugin cannot be handed `&mut AppState` — the engine lives inside it. It *can* be
handed `&AppState`: firing a hook borrows the engine immutably from the state, and a
second immutable borrow of the same state is legal.

`mlua::Lua::scope` makes that borrow available to Lua without copying anything. For the
duration of one hook call, `trail.selection`, `trail.cwd`, `trail.entries`,
`trail.config`, `trail.tabs` and `trail.mode` are scoped functions that close over
`&AppState`; when the hook returns, the scope ends and they are replaced by stubs that
raise a clear error ("only available inside a hook, action or job callback"). A plugin
that stashes one in a global and calls it later gets that error rather than a dangling
reference — which is what `scope` guarantees.

This is lazy: `on_select` fires on every keystroke, and a hook that never calls
`trail.entries()` never pays for a table per entry.

### 3.2 Writes: a request queue on the engine

Write functions push a `PluginRequest` onto a queue in the Lua state's app data. After
the hook returns and the borrow ends, the host drains the queue and applies each request
in order:

```
Lua calls trail.navigate("/tmp")
    → PluginRequest::Action(Action::Navigate("/tmp")) is queued
    → the hook returns; the &AppState borrow ends
    → plugin::host::drain applies it through actions::apply
```

Requests that fire more hooks (`navigate` → `on_enter_dir`, a selection change →
`on_select`) queue more requests, which the event loop drains in further rounds —
**at most four per input event**. A plugin that navigates on every `on_enter_dir` stops
after four hops with an error naming the loop, instead of freezing Trail.

### 3.3 The budget

Every call into Lua runs under an instruction-count hook that checks a deadline every
1 000 instructions and aborts past `[plugins] budget_ms` (default 50 ms; loading a
plugin gets ten times that). The abort is an ordinary Lua error, so it is caught and
reported like any other.

What the budget **cannot** bound is a single blocking C call — `os.execute("sleep 10")`
or `io.popen(...)` blocks inside one instruction. That is documented rather than
prevented, and it is why `trail.spawn` exists: the non-blocking way is also the easier
one.

---

## 4. The API

### 4.1 Events

| Call | Fires | Arguments |
|---|---|---|
| `trail.on_select(fn)` | the selection changes | `path` (string), `entry` (table, §4.2) |
| `trail.on_enter_dir(fn)` | a directory is entered | `path`, `dir` (the `trail.cwd()` table) |
| `trail.on_fs_change(fn)` **(new)** | the watcher reports a change in the current directory | `path` |
| `trail.on(event, fn)` **(new)** | alias: `"select"`, `"enter_dir"`, `"fs_change"` | as above |

Hooks run in load order (`[plugins] enabled` order), which is a promise.

### 4.2 Reads — inside a hook, action, or job callback

| Call | Returns |
|---|---|
| `trail.selection()` | entry table or `nil` |
| `trail.entries()` | array of entry tables — the visible listing, in display order, respecting hidden files and an active search |
| `trail.cwd()` | `{ path, entry_count, show_hidden, sort = {by, reverse, dirs_first}, git_branch, git_dirty }` |
| `trail.config(key)` | the value of any key `:set` accepts (`"editor"` or `"general.editor"`), or `nil` |
| `trail.tabs()` | `{ count, active }` (1-based) |
| `trail.mode()` | `"navigation"`, `"search"` or `"command"` |
| `trail.version()` | Trail's version — available everywhere, including at load time |

An **entry table** is `{ path, name, kind, hidden, size, modified, git, is_text }`:
`kind` is `"file"`, `"dir"` or `"symlink"`; `size` is `nil` for a directory (its byte
length describes its record, not its contents — the same rule as the details column);
`modified` is Unix seconds, for `os.date`; `git` is `nil` until the git worker reports;
`is_text` is `nil` until something has previewed the file. Paths are spelled through
`pathfmt::display`, so no `\\?\` reaches a plugin on Windows.

### 4.3 Writes — queued, applied after the call returns

| Call | Effect |
|---|---|
| `trail.navigate(path)` | enter a directory (a relative path resolves against the current one) |
| `trail.select(path_or_name)` | move the selection to an entry in the current listing |
| `trail.move(n)` | move the selection `n` rows (negative is up) |
| `trail.go_parent()`, `trail.back()`, `trail.forward()` | as `h`, `u`, `Ctrl-r` |
| `trail.refresh()` | re-read the listing, keeping the selection |
| `trail.set_sort{by=, reverse=, dirs_first=}` | the active tab's order; omitted fields are kept |
| `trail.set_hidden(bool)` | show or hide hidden files |
| `trail.yank(text)` | put text on the clipboard |
| `trail.set_config(key, value)` | as `:set key value`, for this session |
| `trail.command(line)` | run a Trail command line, as if typed after `:` — `mkdir`, `mv`, `bookmark`, `jump`, `git`, `sort`, … |
| `trail.open_tab(path?)`, `trail.close_tab()` | as `Ctrl-t`, `Ctrl-w` |
| `trail.run(cmd, opts?)` | suspend Trail and run a command in the terminal, as `!`; `cmd` is a string (through `[general] shell`) or an argv table; `opts.pause` overrides `shell_pause` |
| `trail.notify(msg)`, `trail.error(msg)` | the status bar and the log |
| `trail.set_status(text)` **(new)** | a persistent segment in the status bar; `nil` clears it |

### 4.4 Returning from an action

`trail.register_action(name, fn)` handlers receive the argument string. What they return
is the outcome:

| Return | Result |
|---|---|
| nothing, `nil`, `true` | success, silent |
| a string | success, shown as a notice |
| `false` or `false, "reason"` | failure, shown as `plugin <name>: reason` |
| a Lua error | failure, shown the same way |

### 4.5 Keys — `trail.bind(keys, action, arg?)` **(new)**

Binds a Navigation Mode key or sequence to a registered action: `trail.bind("gv",
"open_in_code")`, `trail.bind("ctrl-g", "git_log")`. Keys use the `[keymap]` spelling.

**A plugin can claim keys Trail does not use; it cannot take one over.** The configured
keymap (defaults included) is consulted first, so `trail.bind("j", …)` never fires.
Multi-character bindings join the prefix machinery, so `gv` makes `g` wait for a second
key exactly as `gg` does. A binding that can never fire is reported at startup.

### 4.6 Previewers — `trail.register_previewer{...}` **(new)**

```lua
trail.register_previewer{
  name = "json",
  extensions = { "json" },
  command = { "jq", "-C", ".", "{path}" },
}
```

A previewer is **declarative**: a match (`extensions`, and/or exact `names`) and a
command, where `{path}` is replaced by the selected file's path. It never runs Lua on the
preview path. Trail runs the command on the worker pool, captures stdout, strips ANSI
colour, bounds it by `[preview] max_lines`, and delivers it through the generation guard
like every other preview — so a slow command shows `Loading…` and a stale result is
dropped, and invariant 1 and invariant 2 both hold by construction. A command that fails
or times out (`timeout_ms`, default 5 000) shows its error in the pane instead.

Plugin previewers are consulted before the built-in ones, in load order.

The earlier draft deferred Lua previewers because a provider must answer synchronously
or defer to a worker, and Lua can do neither safely. A declarative previewer sidesteps
the question: Lua runs once, at load time, to *describe* the previewer, and Rust runs it.

### 4.7 Jobs — `trail.spawn{...}` **(new)**

```lua
trail.spawn{
  cmd = { "git", "log", "-1", "--format=%s", "--", path },
  cwd = trail.cwd().path,        -- optional
  on_exit = function(result)     -- { ok, code, stdout, stderr }
    trail.set_status(result.stdout)
  end,
}
```

Runs a command on the worker pool and calls `on_exit` back on the UI thread, with the
read and write surface available, when it finishes. This is the non-blocking way to do
anything slow, and the answer to "may hooks leave the UI thread?" (§6.2): the hook
doesn't — the *work* does. `trail.spawn` returns a job id. A result for a selection the
user has already left is the plugin's to check (`trail.selection()` is right there),
and the shipped examples show how.

### 4.8 Utilities

`trail.log(msg)` (now at `info`, attributed), `trail.version()`.

---

## 5. What a plugin may *not* change, and why

Unchanged from the first draft, and enforced by construction — there is no API that
reaches these:

| State | Why not |
|---|---|
| `entries` directly | A projection of the filesystem. Change the filesystem and `refresh()`. |
| `preview.*` | Owned by the generation guard (invariant 2). Previewers go through the guard. |
| `git` | Worker-owned; a write would be overwritten unpredictably. |
| `pending_delete` | A plugin that could raise the delete prompt could get `y` to confirm a delete the user never asked for. |
| `mode` | A hook that moved the user into Search or Command Mode would change where their next keystroke lands. |
| `pending_*`, `command_history`, `tab_state` | Input-layer mechanics; reaching them is injecting keystrokes. |
| `plugin_engine` | Re-entrancy. |

---

## 6. The two decisions

### 6.1 May a plugin touch the filesystem? — **Yes, and it always could.**

The first draft proposed an `allow_fs` gate. It would have been theatre: plugins run in
a full Lua with `io` and `os`, so `os.remove` and `io.open(path, "w")` already exist.
Trail loads only plugins the user names in `[plugins] enabled`, from their own config
directory — the same trust model as vim, Neovim and every shell's rc file. A gate on
Trail's own helpers would restrict the polite path and leave the direct one open.

So: **plugins are trusted code**, and the documentation says so on its first page. What
Trail adds is the *integrated* path — `trail.command("mkdir build")`, `trail.command("mv
...")` — which goes through `fs_ops`, reports through the notice channel, and refreshes
the listing. There is deliberately no `trail.delete`: deleting through Trail means the
confirmation prompt, and a plugin must not be able to raise it (§5).

### 6.2 May hooks leave the UI thread? — **No. Work may.**

Of the three options — document a budget, a watchdog, move hooks to the worker pool —
this takes the second and adds the escape hatch the third was reaching for:

- **Hooks stay on the UI thread, under the budget** (§3.3). They keep their ordering
  guarantee and their read access to `&AppState`, which moving them off-thread would
  have cost (it would have needed an owned snapshot per call and `mlua`'s `send`
  feature, and turned this into a MAJOR release).
- **Slow work moves off-thread through `trail.spawn`** and declarative previewers, both
  of which run on the existing worker pool and come back over the existing channel.

---

## 7. Details that bite, and what was done

- **Hook arguments.** Hooks keep the path string as the first argument; the entry table
  is the second. Changing the first would have broken every `"x: " .. path` in the wild.
- **Stale queued actions.** `select(path)` on a listing that changed falls back through
  `restore_selection`, so it is benign. Documented.
- **Re-entrancy.** Four drain rounds per input event, then an error (§3.2).
- **Load errors.** Collected at startup and shown in the status bar, not just logged.
- **Key conflicts.** Reported at startup (§4.5).
- **`is_text` is usually `nil`.** Documented in the entry-table reference.

---

## 8. What this does not do

- **Mode changes and search filters.** §5.
- **Persistent plugin storage.** A plugin can write its own file with `io`; a Trail-owned
  store is a product decision (where it lives, when it is written) for when two plugins
  need one.
- **Plugin actions in `[keymap]`.** `trail.bind` covers it from the plugin side. Letting
  the user rebind a plugin's action from `trail.toml` means teaching the keymap validator
  about names that only exist after plugins load.
- **A plugin manager.** `[plugins] enabled` and a `.lua` file remain the whole story.
- **Other scripting languages.** Lua only; WASM stays deferred in the Decision Log.

---

## 9. Version impact

MINOR throughout. Every function is new, hook signatures only gain arguments, and the one
new config key (`[plugins] budget_ms`) defaults to a value no existing plugin plausibly
exceeds. The behaviour change worth a line in the notes: plugin errors that used to be
silent now reach the status bar.
