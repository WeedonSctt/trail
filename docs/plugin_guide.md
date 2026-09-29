# Trail Plugin Guide

A Trail plugin is a Lua file. It can react to what you do, read what Trail is showing,
change it, bind keys, run commands in the background and add previewers.

```lua
-- hello.lua, in the config directory (§1)
trail.register_action("hello", function()
    local s = trail.selection()
    return "hello from " .. (s and s.name or "nowhere")
end)
trail.bind("gh", "hello")
```

```toml
# trail.toml
[plugins]
enabled = ["hello"]
```

Press `gh` and the status bar says `hello from README.md`.

---

## 1. Loading

`[plugins] enabled` lists plugin names, in order. Each is looked for as a built-in
plugin (only `"bookmarks"` today), then as `<name>.lua` in Trail's config directory:

| Platform | Directory |
|---|---|
| Windows | `%APPDATA%\trail\config\` |
| Linux | `~/.config/trail/` (or `$XDG_CONFIG_HOME/trail/`) |
| macOS | `~/Library/Application Support/trail/` |

If the platform names no home directory, `<name>.lua` is looked for in the directory
Trail was started from. Plugins load in the order listed, and their hooks run in that
order.

A plugin that fails to load is named in the status bar at startup, with the error. It
keeps nothing it registered before failing: a broken plugin is absent, not half there.

### Plugins are trusted code

Trail does not sandbox plugins. Lua's `io` and `os` libraries are available, so a
plugin can read and write any file you can. Enable only plugins you have read or whose
author you trust, as you would for a vim plugin or a line in your shell's rc file.

### The budget

Plugin code runs on the same thread that draws the screen, so a slow hook would freeze
Trail. Every call into a plugin is therefore given `[plugins] budget_ms` (default 50 ms)
and stopped with an error if it runs longer. Loading a plugin gets ten times that.

The budget can stop a Lua loop, but not a single blocking call — `os.execute("sleep
5")` or `io.popen(...)` blocks until it returns. **Use `trail.spawn` for anything that
waits on a process or the disk**: it runs off the UI thread and is not budgeted.

---

## 2. Events

```lua
trail.on_select(function(path, entry) ... end)     -- the selection changed
trail.on_enter_dir(function(path, dir) ... end)    -- the current directory changed
trail.on_fs_change(function(path) ... end)         -- files changed in the current directory
trail.on("select", fn)                             -- the same, by name
```

- `on_select` gets the selected path as a string and its [entry table](#entry-tables).
- `on_enter_dir` fires once for every change of directory, however it happened — `l`,
  `h`, `u`, `:jump`, a tab switch, a plugin's `navigate` — and once for the directory
  Trail starts in. `dir` is the [`trail.cwd()`](#reads) table.
- `on_fs_change` fires after Trail re-reads the listing because something on disk
  changed.

An error in a hook is shown in the status bar as `plugin <name>: <event>: <error>`.

---

## 3. Actions

```lua
trail.register_action("name", function(arg) ... end)
```

Runs on `:plugin name [arg]`, or from a key bound with `trail.bind`. What it returns is
the outcome:

| Return | Status bar |
|---|---|
| nothing, `nil`, `true` | nothing |
| `"text"` | `text` |
| `false` / `false, "why"` | `plugin name: why` (error) |
| an error | `plugin name: <error>` (error) |

`:plugin ` followed by Tab completes action names.

---

## 4. Keys

```lua
trail.bind("gv", "open_in_editor")          -- a two-key sequence
trail.bind("ctrl-g", "git_log", "--oneline") -- a named key, with an argument for the action
```

Keys use the `[keymap]` spelling: one or two characters, or a key name (`enter`,
`ctrl-x`, `shift-end`, …). Navigation Mode only.

**A plugin can claim keys Trail does not use; it cannot take one over.** Trail's own
keymap is consulted first, so `trail.bind("j", …)` never fires. A two-key binding joins
Trail's prefixes: `gv` works alongside `gg`. A binding that can never fire — because
Trail uses the key, or because the action does not exist — is reported at startup.

Free in the default keymap: `a`, `b`, `c`, `e`, `f`, `i`, `n`, `p`, `r`, `t`, `v`, `w`,
`x`, `z`, every capital except `G`, `J`, `K` and `R`, and any `g` sequence other than `gg`.
(The built-in `bookmarks` plugin takes `b`; the examples take `gs`, `gv`, `gV` and `g-`.)

### Plugins written for Trail before 1.10

Keep working unchanged. Hooks still receive the path string first — the entry table is a
new second argument — and an action that returns nothing still succeeds silently. What
changes is that their errors, which used to go only to a debug log, now appear in the
status bar.

---

## 5. Reads

Available inside a hook, an action or a job callback — not at the top level of the
file, where there is nothing to read yet.

| Call | Returns |
|---|---|
| `trail.selection()` | the selected [entry](#entry-tables), or `nil` in an empty directory |
| `trail.entries()` | every entry on screen, in display order — hidden files only while shown, only the matches during a search |
| `trail.cwd()` | `{ path, entry_count, show_hidden, sort = { by, reverse, dirs_first }, git_branch, git_dirty }` — the `git_` fields are `nil` outside a repository |
| `trail.config(key)` | any key `:set` accepts, as `"editor"` or `"general.editor"`; `nil` if there is no such key |
| `trail.tabs()` | `{ count, active }`, 1-based |
| `trail.mode()` | `"navigation"`, `"search"` or `"command"` |
| `trail.version()` | Trail's version, e.g. `"1.10.0"` — available everywhere |

### Entry tables

| Field | |
|---|---|
| `path` | absolute path |
| `name` | file name |
| `kind` | `"file"`, `"dir"` or `"symlink"` |
| `hidden` | boolean |
| `size` | bytes; `nil` for a directory, whose size on disk says nothing about its contents |
| `modified` | Unix seconds, for `os.date` |
| `git` | `"modified"`, `"added"`, `"deleted"`, `"renamed"`, `"untracked"`, or `nil` for clean or not yet known |
| `is_text` | `true`/`false` once Trail has previewed the file, `nil` before |

---

## 6. Writes

Writes are **requests**: they take effect after your function returns, in the order you
made them. Reading after writing in the same call sees the old state.

| Call | Does |
|---|---|
| `trail.navigate(path)` | go to a directory; relative paths resolve against the current one |
| `trail.select(name_or_path)` | select an entry in the current listing |
| `trail.move(n)` | move the selection `n` rows; negative is up |
| `trail.go_parent()`, `trail.back()`, `trail.forward()` | as `h`, `u`, `Ctrl-r` |
| `trail.refresh()` | re-read the listing |
| `trail.set_sort{ by = "size", reverse = true, dirs_first = false }` | the current tab's order; leave out what should not change |
| `trail.set_hidden(true)` | show or hide hidden files |
| `trail.yank(text)` | copy to the clipboard |
| `trail.set_config(key, value)` | as `:set key value`, for this session |
| `trail.command("mkdir build")` | run a Trail command, as if typed after `:` |
| `trail.open_tab(path)`, `trail.close_tab()` | as `Ctrl-t`, `Ctrl-w`; `path` is optional |
| `trail.run(cmd, { pause = "never" })` | leave the screen and run a command in the terminal, as `!` does |
| `trail.notify(text)`, `trail.error(text)` | a message in the status bar |
| `trail.set_status(text)` | a lasting segment in the middle of the status bar; `nil` clears it |
| `trail.log(text)` | a line in Trail's log file |

A command (`cmd` in `run` and `spawn`) is either a **string**, run through
`[general] shell` like `!`, or a **table** `{ "program", "arg", … }`, run directly —
safer whenever an argument is a path that may contain spaces.

`trail.command` is how a plugin changes files through Trail: `mkdir`, `touch`,
`rename`, `mv`, `cp` behave exactly as typed, report the same way, and refresh the
listing. There is no `trail.delete` — deleting through Trail means its confirmation
prompt, and a plugin cannot raise that on your behalf.

A plugin cannot change the mode, the search query or the preview contents, and cannot
press keys for you.

If hooks keep triggering each other — `on_enter_dir` navigating, which fires
`on_enter_dir` — Trail stops after four rounds and says so.

---

## 7. Background jobs

```lua
local id = trail.spawn{
    cmd = { "git", "log", "-1", "--format=%s" },
    cwd = trail.cwd().path,          -- optional; the current directory by default
    on_exit = function(result)       -- optional
        -- result = { ok, code, stdout, stderr, id }
        trail.set_status(result.stdout)
    end,
}
```

The command runs off the UI thread; `on_exit` runs back on it when the command finishes,
with the reads and writes available. By then the user may have moved on — if the result
is about the selection, check the selection is still the one you asked about. See
`examples/plugins/git_line.lua`.

---

## 8. Previewers

```lua
trail.register_previewer{
    name = "jq",                          -- optional; shown in errors
    extensions = { "json" },              -- and/or
    names = { "Pipfile.lock" },           -- exact file names, case-insensitive
    command = { "jq", ".", "{path}" },    -- {path} is the selected file
    timeout_ms = 3000,                    -- optional; default 5000
}
```

A previewer is a *description*: Trail runs the command itself, off the UI thread, and
shows its output in the preview pane, numbered like any text file. `Loading…` shows
while it runs; if you move on first, its result is thrown away. Colour escape codes are
stripped. If the command fails or times out, the pane says why.

Plugin previewers take precedence over Trail's own, in load order.

---

## 9. Examples

In [`examples/plugins/`](../examples/plugins). Copy one into your config directory and
add its name to `[plugins] enabled`.

| Plugin | Shows |
|---|---|
| `dir_summary.lua` | `gs` counts files and adds up sizes — an action that returns its message |
| `git_line.lua` | the last commit for the selected file in the status bar — `spawn` with a stale-result check |
| `open_in_editor.lua` | `gv` / `gV` open the selection or directory in VS Code — binding keys, background launch |
| `json_preview.lua` | pretty-printed JSON — a previewer |
| `jump_back.lua` | `g-` toggles between the last two directories — plugin state across events |

The built-in `bookmarks` plugin binds `b` to bookmark the current directory.
