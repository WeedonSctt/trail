# Trail — Plugin API Plan

A design for widening the Lua plugin surface from "observe and log" to "read the
session and change it". **Nothing here is built.** This document decides the shape
so the implementation is a matter of typing, and records the two questions that
have to be answered before it can start.

Assessed against the code on 2026-09-29, at v1.8.3. Companion to
[`upcoming_features.md`](upcoming_features.md) §2.14, which is the triage entry;
this is the design behind it.

---

## 1. What exists today

Four functions, in `src/plugin/lua_api.rs`:

| Call | Receives | Returns | Fired from |
|---|---|---|---|
| `trail.on_select(fn)` | the selected path, as a string | ignored | `main.rs:322`, on every selection change |
| `trail.on_enter_dir(fn)` | the new directory, as a string | ignored | `state.rs:802`, on `enter_dir` |
| `trail.register_action(name, fn)` | one argument string | **discarded** | `actions/mod.rs:967`, via `:plugin <name> [arg]` |
| `trail.log(msg)` | a string | — | writes at `debug` level |

So a plugin can be told where the user went, and write a line to a log file the
user is not looking at. It cannot read the selection's size or kind, navigate
anywhere, put a message on screen, read a config key, or report that an action
failed. `PluginEngine`'s own comment says the surface is "intentionally minimal
for v1 — resist expanding it until" there is a reason. There is now.

The one shipped plugin, `example_bookmarks.lua`, works only because bookmarks
were *also* built as a core Rust module (`src/plugin/bookmarks.rs`) that the
command layer calls directly. It is a demonstration that the API exists, not
that it is sufficient.

---

## 2. The constraint that shapes everything

A plugin cannot be handed `&mut AppState`. This is not a preference — it does
not compile.

```rust
// actions/mod.rs:966
if let Some(engine) = &state.plugin_engine {
    if engine.fire_action(&name, &arg) { … }
}
```

The engine lives *in* `AppState`, so reaching it immutably borrows the whole
struct for as long as Lua is running. `fire_action` is `&self` for that reason.
Any design where a Lua callback calls `trail.navigate(...)` and something
mutates state underneath is a borrow error, and working around it with
`RefCell` or by moving the engine out and back buys a runtime panic instead of a
compile error.

**The answer is already in the codebase.** `Action::RunExternal` has the same
problem — it must leave the alternate screen, which `apply()` cannot do — and it
is solved with a deferral field:

```rust
/// Using a state field rather than a channel keeps `apply()` synchronous and
/// testable without a running terminal.
pub pending_external: Option<crate::actions::Action>,
```

`apply()` writes the request there; the event loop drains it afterwards, when
the borrow is gone.

### The design follows from it

A plugin does not mutate Trail. It **requests `Action`s**, which are drained and
applied after the hook returns.

```
Lua calls trail.navigate("/tmp")
    → pushes Action::Navigate("/tmp") onto the engine's own queue
    → hook returns, the &AppState borrow ends
    → caller drains the queue and runs actions::apply(action, state) for each
```

This is worth more than a workaround. `actions/mod.rs` already says *"Every
user-initiated mutation flows through an `Action` value, keeping the state
machine testable independently of input handling."* Routing plugins through the
same funnel means:

- a plugin can do exactly what a keybinding can do, and nothing else;
- every plugin effect is already covered by the `Action` tests;
- the notice, the dirty flag, the selection anchoring and the per-tab
  bookkeeping all happen for free, because `apply()` does them;
- nothing new can corrupt state, because nothing new writes to it.

The queue belongs on `PluginEngine` behind a `RefCell<Vec<Action>>` — interior
mutability on the *engine*, which is not `AppState`, so the borrow is legal and
its scope is one field rather than the world.

---

## 3. The read surface

Hooks currently receive a bare string. They should receive a **table**, built
once per call from state the UI thread already holds. No I/O: every field below
is already in memory.

### `trail.selection()` → table or `nil`

| Key | Type | Source | Note |
|---|---|---|---|
| `path` | string | `Entry::path` | absolute, plainly spelled (no `\\?\`) |
| `name` | string | `Entry::file_name` | |
| `kind` | string | `Entry::kind` | `"file"`, `"dir"`, `"symlink"` |
| `hidden` | boolean | `Entry::is_hidden` | |
| `size` | integer or `nil` | `Entry::metadata` | `nil` for an unstat-able entry |
| `modified` | integer or `nil` | `Entry::metadata` | Unix seconds, for Lua's `os.date` |
| `git` | string or `nil` | `Entry::git_status` | `nil` until the worker reports |
| `is_text` | boolean or `nil` | `Entry::is_text` | `nil` means "not yet classified" — see §7 |

### `trail.cwd()` → table

| Key | Type | Source |
|---|---|---|
| `path` | string | `AppState::cwd` |
| `entry_count` | integer | `StatusBarState::entry_count` |
| `show_hidden` | boolean | `AppState::show_hidden` |
| `sort` | table | `{ by, reverse, dirs_first }` from `SortSettings` |
| `git_branch` | string or `nil` | `GitDirState::branch` |
| `git_dirty` | boolean or `nil` | `GitDirState::is_dirty` |

### `trail.entries()` → array of selection tables

The whole visible listing, in display order, each entry shaped as
`trail.selection()`. Respects `show_hidden` and the active filter, so a plugin
sees what the user sees. This is the expensive one — a table per entry — so it
is a call rather than a hook argument, and a plugin that wants one field of one
entry does not pay for the directory.

### `trail.config(key)` → string, number, boolean or `nil`

Read against the same key vocabulary `:set` accepts, section-qualified or
aliased (`"general.editor"` or `"editor"`). One accessor rather than a dumped
table, so the config struct can change shape without breaking plugins.

### `trail.tabs()` → table

`{ count, active }`. Enough to write "open this in a new tab if more than one is
open"; not enough to inspect another tab's listing, which would mean exposing
`TabManager` internals for no use case yet named.

### `trail.version()` → string

`CARGO_PKG_VERSION`, so a plugin can refuse to load against an older Trail than
it needs.

---

## 4. The write surface — what a plugin may change

**This is the list the design turns on.** Every entry maps to an `Action`, and
anything not in this table cannot be changed by a plugin at all.

| Lua call | `Action` | What changes |
|---|---|---|
| `trail.navigate(path)` | `Navigate(PathBuf)` **(new)** | `cwd`, `entries`, `selected`, `history` |
| `trail.select(path)` | `SelectPath(PathBuf)` **(new)** | `selected` |
| `trail.move_selection(n)` | `MoveDown` / `MoveUp` | `selected` |
| `trail.go_parent()` | `GoParent` | `cwd` and everything it implies |
| `trail.back()` / `trail.forward()` | `HistoryBack` / `HistoryForward` | `cwd`, `history` |
| `trail.refresh()` | `Refresh` | `entries`, preserving the selection by path |
| `trail.set_sort{by, reverse, dirs_first}` | `SetSortBy` / `ToggleSortReverse` / `ToggleDirsFirst` | the **active tab's** `sort` |
| `trail.set_hidden(bool)` | `ToggleHidden` | `show_hidden` |
| `trail.set_filter(query)` | `SetFilter(String)` **(new)** | `filter`, `selected` |
| `trail.notify(msg)` / `trail.error(msg)` | — direct, see §5 | `notice` |
| `trail.yank(text)` | `CopyText(String)` **(new)** | the OS clipboard, `last_yank` |
| `trail.set_config(key, value)` | `ExecuteCommand(Set{…})` | one config key, for this session |
| `trail.open_tab(path)` / `trail.close_tab()` | `NewTab` / `CloseTab` | `tab_manager` |
| `trail.run(cmd)` | `RunExternal{…}` | suspends Trail, runs a command — **gated, see §6** |

Five new `Action` variants. Each is a thin wrapper over a method `AppState`
already has (`enter_dir`, `apply_filter`, `set_sort`, `restore_selection`), so
the cost is the enum arm and the dispatch, not new logic.

### What a plugin may *not* change, and why

| Field | Why not |
|---|---|
| `entries` directly | It is a projection of the filesystem. A plugin that could write it could make Trail show files that do not exist. Mutate the filesystem and `refresh()`. |
| `preview.*` | Owned by the generation guard (invariant 2). A plugin writing `content` without the matching `generation` reintroduces exactly the stale-preview bug the guard exists to stop. Scroll actions are fine; the content is not. |
| `git` | Worker-owned and invalidated on fs events. A plugin write would be overwritten unpredictably. |
| `dirty` | Render bookkeeping. `apply()` sets it. |
| `selection_memory` | Internal to `load_dir`'s re-anchoring. |
| `pending_delete` | A confirmation in flight. A plugin that could set it could put a "delete?" prompt on screen that the user did not ask for and that `y` would then confirm. |
| `pending_external`, `pending_nav_key`, `command_history`, `tab_state` | Input-layer mechanics; a plugin reaching them is a plugin injecting keystrokes. |
| `plugin_engine` | Reentrancy. A plugin loading a plugin during a hook is a borrow error and a support problem. |
| `launch_dir` | Fixed at startup; `yr` is defined relative to it. |
| `mode` | See §6 — this one is a judgement call rather than a flat no. |

---

## 5. Reporting, and the return value

`fire_action` currently returns `bool` meaning *"was an action of this name
registered"*, and throws the Lua return value away. A plugin action that fails
has nowhere to say so: `tracing::debug!` is off by default.

- **`trail.notify(msg)` and `trail.error(msg)`** write through `AppState::notify`
  and `set_error`, the channel built for exactly this in `upcoming_features.md`
  §2.1 — which means a plugin message also reaches the log, in the same format
  as everything else.
- **A handler's return value becomes the outcome.** Returning nothing or `true`
  is success; returning `false` or `false, "reason"` is a failure the status bar
  reports as `plugin <name>: reason`. A Lua runtime error is caught and reported
  the same way, rather than being swallowed.

These two are worth building first and shipping alone, because they turn the
existing API from unusable into merely narrow.

---

## 6. Two things that need a decision

### 6.1 May a plugin touch the filesystem?

Everything in §4 is navigation and display. `trail.move`, `trail.copy`,
`trail.delete`, `trail.mkdir` are a different class: the blast radius is the
user's disk, and a buggy loop in a Lua file the user copied off the internet is
not recoverable by pressing `Esc`.

If they are added, then:

- they must route through `actions::fs_ops`, so `[general] delete_mode` applies
  and a plugin delete goes to the recycle bin like `dd` does;
- `[plugins] allow_fs = false` should gate them, defaulting off, so enabling a
  plugin and granting it write access are two decisions rather than one;
- they belong behind the same confirmation the user gets, or the user finds out
  what happened afterwards.

**Recommendation: not in the first version.** `trail.run(cmd)` reaches the same
capability through a shell the user can see, with `shell_pause` already holding
the screen, and it is honest about what it is.

### 6.2 May hooks leave the UI thread?

Today they cannot. `fire_on_select` is called synchronously from the event loop,
so a hook that sleeps, reads a large file or makes a network call freezes the
frame. That is invariant 1 — "the UI thread never blocks" — and every other
variable-latency thing in Trail was moved off it for exactly this reason.

Three options:

1. **Leave it, and document a budget.** Hooks are for decisions, not work; a
   plugin that needs to do work calls `trail.run`. Cheapest, and honest, but
   nothing enforces it and the failure mode is a frozen file manager.
2. **A watchdog.** Run the hook with a Lua instruction-count hook that aborts
   past a threshold and reports it as a plugin error. Bounded damage, no
   threading change, and it makes the budget real instead of documented. This is
   the recommendation.
3. **Move hooks to the worker pool.** Correct, and the largest change: `mlua`'s
   `Lua` needs the `send` feature, plugin state stops being shareable with
   anything non-`Send`, and hook *ordering* stops being guaranteed — which
   changes what the API can promise and is what would make this MAJOR rather
   than MINOR.

Note that option 3 conflicts with §4: a hook running off the UI thread cannot
read `&AppState` at all, so the read surface would have to become an owned
snapshot taken before dispatch. That is a bigger design than it looks, and it is
why this question has to be answered *before* the read surface is built, not
after.

**`mode`, from §4, belongs to this decision too.** Letting a plugin push the user
into Search or Command mode means a hook can change where the next keystroke
goes — which is fine from a `:plugin` invocation the user typed, and hostile
from an `on_select` hook that fires as they arrow through a directory. If it is
allowed at all, it should be allowed only from a registered action.

---

## 7. Details that will bite

- **`is_text` is `nil` most of the time.** It is populated by the highlight
  worker, so it is unknown until something previews the file. A plugin that
  branches on it will behave differently on the second visit to a directory than
  the first. Either document it plainly or leave it out of v2.
- **Paths must go out through `pathfmt::display`.** A raw `cwd` on Windows can
  carry `\\?\`, and a plugin that pastes it into a shell command produces
  something the shell rejects. This is `pathfmt`'s whole reason for existing.
- **Hook order is load order** (`on_select_keys` is a `Vec`), which is the
  `[plugins] enabled` order. That is a promise worth stating, because plugins
  will come to depend on it.
- **A queued action can be stale.** If `on_select` queues `trail.select(path)`
  and the user has already moved on, the action applies to a listing that
  changed. `restore_selection` handles a missing path by falling back, so the
  failure is benign — but it should be a documented promise, not luck.
- **Queue ordering and re-entrancy.** Actions drain in the order they were
  queued; an action that itself fires a hook (`Navigate` → `on_enter_dir`) must
  append to the queue rather than recurse, or a plugin that navigates on
  `on_enter_dir` loops forever. A depth counter, or draining only what was
  queued before the drain started.

---

## 8. Phasing

Each step is independently shippable and MINOR.

| Step | Contents | Why here |
|---|---|---|
| 1 | `trail.notify`, `trail.error`, handler return values, Lua errors surfaced | Makes the *existing* API usable. No new architecture. |
| 2 | Structured `trail.selection()`, `trail.cwd()`, `trail.config()`, `trail.version()`; hooks receive the table | Pure reads, no queue needed, no decision blocked. |
| 3 | The action queue plus the five new `Action` variants; §4's write surface | The real change. Needs 6.2 answered first. |
| 4 | `trail.entries()`, `trail.tabs()`, the watchdog | Rounding out, once the shape is proven. |
| 5 | Filesystem access behind `[plugins] allow_fs` | Only if 6.1 is answered yes. |

Steps 1 and 2 need no decision and could start now.

## 9. Things this plan does not cover

- **Preview providers from Lua.** The architecture doc lists it under
  extensibility, and it is a different shape: a provider must answer
  `can_handle`/`preview` synchronously on the UI thread, or return `Deferred`
  and run in a worker — which lands straight back in 6.2, harder.
- **Keybindings from Lua.** `register_action` plus a `[keymap]` entry pointing
  at it would cover most of the want, and needs the keymap to accept plugin
  action names — a small change, but it belongs with the keymap fix in
  `upcoming_features.md` §2.15 rather than here.
- **A plugin manager.** Installing, versioning and updating plugins is a
  separate product question; today `[plugins] enabled` plus a `.lua` file in the
  config directory is the whole story, and it is adequate until there are
  plugins to manage.
