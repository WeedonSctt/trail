# Trail Plugin Guide

Trail includes a lightweight, event-driven Lua scripting engine (v1) powered by `mlua` and Lua 5.4. This allows you to extend Trail's functionality, register custom Command Mode actions, and automate workflows in response to user navigation without compiling or modifying Trail's Rust source code.

This guide covers:
1. **Where Trail loads plugins from**
2. **How to enable a plugin**
3. **What a plugin can do**
4. **How to use a plugin**
5. **A complete end-to-end example (`activity_logger`) followed step-by-step**

---

## 1. Where Trail Loads Plugins From

When Trail initializes during application startup (Phase 8 of startup in `main.rs`), it resolves each entry in the configured `enabled` plugin list using a strict lookup order:

```
[plugins.enabled list in trail.toml]
                 │
                 ▼
     ┌───────────────────────┐
     │ Is it "bookmarks"?    │──Yes──► Load embedded Lua source code
     └───────────────────────┘         (compiled into binary via include_str!)
                 │
                 │ No
                 ▼
     ┌────────────────────────────────────────────────────────┐
     │ Look in OS-specific user configuration directory:      │
     │ - Linux/macOS: ~/.config/trail/<name>.lua              │
     │ - Windows:     %APPDATA%\trail\config\<name>.lua       │
     └────────────────────────────────────────────────────────┘
                 │
                 │ Fallback (if OS config dir is unavailable)
                 ▼
     ┌────────────────────────────────────────────────────────┐
     │ Look relative to current working directory:            │
     │ - ./<name>.lua                                         │
     └────────────────────────────────────────────────────────┘
```

### OS-Specific Load Directories
To load a custom plugin named `my_plugin`, Trail searches for a file named `my_plugin.lua` inside the OS config folder:
- **Windows**: `%APPDATA%\trail\config\my_plugin.lua`  
  *(e.g., `C:\Users\<Username>\AppData\Roaming\trail\config\my_plugin.lua`)*
- **Linux**: `~/.config/trail/my_plugin.lua` *(or `$XDG_CONFIG_HOME/trail/my_plugin.lua`)*
- **macOS**: `~/Library/Application Support/trail/my_plugin.lua` *(or `~/.config/trail/my_plugin.lua` depending on system standard directories)*
- **Relative Fallback**: `./my_plugin.lua` *(relative to the working directory where Trail was launched)*

> [!NOTE]
> The filename extension must be strictly `.lua`. In `trail.toml`, you only specify the plugin basename (e.g., `"my_plugin"` without `.lua`).

### Built-in Embedded Plugins
Trail also supports built-in plugins compiled directly into the binary. Currently, Trail includes the `"bookmarks"` plugin as an embedded plugin string. When `"bookmarks"` is listed in `enabled`, Trail loads it directly from binary memory without reading any external file from disk.

### Loading & Fault Tolerance
Plugin scripts are evaluated top-to-bottom exactly once during Trail startup. If a plugin file is missing, cannot be read, or contains Lua syntax/compile errors:
- Trail catches the error, logs a debug diagnostic trace (`tracing::debug!`), and skips that single plugin.
- Trail **will not crash** and will continue loading any remaining enabled plugins normally.

---

## 2. How to Enable a Plugin

Plugins are enabled by adding their name (the file basename without `.lua`) to the `[plugins]` section in your `trail.toml` configuration file.

### Step 1: Edit `trail.toml`
Add the `[plugins]` block with the `enabled` array containing string names:

```toml
# trail.toml

[general]
editor = "nvim"

[plugins]
# Enable built-in plugins or custom Lua scripts placed in the config folder
enabled = [
    "bookmarks",        # Built-in embedded bookmarks plugin
    "activity_logger"   # Loads activity_logger.lua from config dir
]
```

### Step 2: Launch Trail with `--config`
Because Trail requires an explicit configuration path flag to apply settings from disk, launch Trail using the `--config` option:

```bash
trail --config /path/to/trail.toml
```

When Trail boots up, it reads `[plugins].enabled` from the TOML file and initializes each specified plugin.

---

## 3. What a Plugin Can Do

Plugins run inside an embedded Lua 5.4 interpreter state (`mlua`) shared across all plugins. Trail exposes a global table named `trail` to all plugin scripts during evaluation.

### Available Lua API (`trail` Table)

| Function | Signature | Description |
| :--- | :--- | :--- |
| `trail.log` | `trail.log(message: string)` | Writes an info log message to Trail's internal logging framework (`tracing::info!`). |
| `trail.on_select` | `trail.on_select(fn: function(path: string))` | Registers a callback that fires whenever the user highlights a new file or directory entry in the navigation tree. |
| `trail.on_enter_dir` | `trail.on_enter_dir(fn: function(dir: string))` | Registers a callback that fires whenever the user enters a new directory. |
| `trail.register_action` | `trail.register_action(name: string, fn: function(arg: string))` | Registers a custom Command Mode action. Users invoke it via `:plugin <name> [arg]`. |

### Plugin Capabilities & Use Cases
Plugins can:
- **Track Navigation State**: React automatically to directory changes and cursor movement.
- **Extend Command Mode**: Add custom interactive commands (`:plugin <action_name> [arg]`) for user-triggered workflows.
- **Perform Custom File/System Operations**: Standard Lua 5.4 functions (e.g., `io.open`, `os.date`, string manipulation, math) are available to write logs, process paths, or store custom state.
- **Provide Auto-Completion**: Actions registered via `trail.register_action` automatically register their names in Command Mode tab-completion.

### Fault Isolation & Safety
Trail guarantees application stability when running third-party Lua code:
- **Sandbox Safety**: Callbacks are executed within Rust error-handling wrappers (`fire_on_select`, `fire_on_enter_dir`, `fire_action`).
- **No Crash Guarantee**: If a callback throws a Lua runtime error (or calls `error()`), Trail catches the exception, logs the error details, and continues running the TUI seamlessly.

---

## 4. How to Use a Plugin

Plugins operate in two distinct modes once loaded:

### A. Automatic / Background Event Hooks
Hooks registered using `trail.on_select` and `trail.on_enter_dir` run **automatically** in the background:
- **Selection Event (`on_select`)**: Fires instantly whenever the user presses `j`, `k`, `Up`, `Down`, or uses jump keys to change the highlighted row. The callback receives the absolute path of the selected item.
- **Directory Change Event (`on_enter_dir`)**: Fires whenever the user presses `Enter` or `l` to navigate inside a directory. The callback receives the absolute path of the entered directory.

### B. Interactive Command Mode Actions (`:plugin`)
Actions registered using `trail.register_action` are invoked manually by the user in **Command Mode**:
1. Press `:` to enter Command Mode.
2. Type `:plugin <action_name> [argument]` and press `Enter`.

#### Command Mode Features for Plugins:
- **Tab Completion**: When you type `:plugin ` and press `Tab`, Trail automatically suggests all action names registered by loaded plugins.
- **Arguments**: Any text typed after `:plugin <action_name> ` is passed as a single string argument `arg` to your Lua callback function (or an empty string `""` if no argument was provided).

---

## 5. End-to-End Example: Building & Using `activity_logger`

Let's build a complete custom plugin named `activity_logger` and follow it through creation, installation, configuration, and execution.

### Objective
We want a plugin that:
1. Logs file selections and directory entries to a local log file `activity.log`.
2. Registers a custom command `:plugin log_note [text]` allowing the user to append custom notes to `activity.log` directly from Trail's Command Mode.

---

### Step 1: Write the Lua Script (`activity_logger.lua`)

Create a file named `activity_logger.lua` with the following code:

```lua
-- activity_logger.lua
-- A Trail plugin that records navigation activity and custom notes to a log file.

trail.log("Initializing activity_logger plugin...")

-- Helper function to append text to activity.log in the user's home/temp folder
local function append_log(line)
    local log_path = "activity.log"
    local f = io.open(log_path, "a")
    if f then
        local timestamp = os.date("%Y-%m-%d %H:%M:%S")
        f:write("[" .. timestamp .. "] " .. line .. "\n")
        f:close()
    end
end

-- 1. Hook into directory entrance events
trail.on_enter_dir(function(dir)
    trail.log("activity_logger: Entered directory -> " .. dir)
    append_log("ENTERED DIR: " .. dir)
end)

-- 2. Hook into item selection events
trail.on_select(function(path)
    trail.log("activity_logger: Selected item -> " .. path)
    append_log("SELECTED: " .. path)
end)

-- 3. Register custom action: :plugin log_note <text>
trail.register_action("log_note", function(arg)
    if arg == "" then
        trail.log("activity_logger: Note creation skipped (empty argument)")
        append_log("NOTE: [Empty note]")
    else
        trail.log("activity_logger: Custom note recorded -> " .. arg)
        append_log("NOTE: " .. arg)
    end
end)

trail.log("activity_logger plugin successfully loaded.")
```

---

### Step 2: Install the Plugin File

Move `activity_logger.lua` into your platform's Trail configuration directory:

- **Windows**: Copy to `%APPDATA%\trail\config\activity_logger.lua`  
  *(e.g., `C:\Users\Alice\AppData\Roaming\trail\config\activity_logger.lua`)*
- **Linux**: Copy to `~/.config/trail/activity_logger.lua`
- **macOS**: Copy to `~/Library/Application Support/trail/activity_logger.lua`

---

### Step 3: Enable `activity_logger` in `trail.toml`

Open your `trail.toml` configuration file and add `"activity_logger"` to `[plugins].enabled`:

```toml
# trail.toml

[general]
editor = "nvim"

[plugins]
enabled = [
    "bookmarks",
    "activity_logger"
]
```

---

### Step 4: Launch Trail and Use the Plugin

Launch Trail with your configuration file:

```bash
trail --config trail.toml
```

#### What Happens at Startup:
1. Trail initializes the embedded Lua 5.4 engine.
2. Trail loads `"bookmarks"` (built-in).
3. Trail locates `activity_logger.lua` in your config directory and executes it.
4. `activity_logger.lua` calls `trail.on_enter_dir`, `trail.on_select`, and `trail.register_action("log_note", ...)` to register callbacks.

#### Interacting with the Plugin at Runtime:

1. **Automatic Logging via Navigation**:
   - Move the selection cursor down with `j` or `Down`:  
     `trail.on_select` fires automatically for each selected file.
   - Enter a subfolder by pressing `Enter` or `l`:  
     `trail.on_enter_dir` fires automatically for the new folder path.

2. **Using the Interactive Command Mode Action**:
   - Press `:` to open Command Mode.
   - Type `:plugin log_` and press `Tab`:  
     Trail's autocompletion fills in `:plugin log_note `.
   - Type `Finished reviewing project documentation` and press `Enter`:
     ```text
     :plugin log_note Finished reviewing project documentation
     ```
   - Trail executes the registered `log_note` callback in Lua, writing `[2026-08-01 12:00:00] NOTE: Finished reviewing project documentation` into `activity.log`.

---

### Step 5: Check Log Output

If you open `activity.log`, you will see entries generated by both automatic hooks and manual commands:

```text
[2026-08-01 12:00:01] ENTERED DIR: /home/alice/projects/trail
[2026-08-01 12:00:03] SELECTED: /home/alice/projects/trail/Cargo.toml
[2026-08-01 12:00:05] SELECTED: /home/alice/projects/trail/docs
[2026-08-01 12:00:06] ENTERED DIR: /home/alice/projects/trail/docs
[2026-08-01 12:00:10] NOTE: Finished reviewing project documentation
```

---

## 6. Built-in Example: The `bookmarks` Plugin

Trail includes an embedded `"bookmarks"` plugin. To use it, simply include `"bookmarks"` in `[plugins].enabled`:

```toml
[plugins]
enabled = ["bookmarks"]
```

### Registered Actions:
- `:plugin bookmark [name]` — Saves the current directory as a named bookmark.
- `:plugin jump <name>` — Navigates directly to a previously saved bookmark.

The `bookmarks` plugin demonstrates how Lua plugins interact with Trail: it registers `bookmark` and `jump` actions via `trail.register_action` and hooks into `on_enter_dir` to track directory state.

