# Trail User Guide

Trail is a terminal-first workspace for navigating, inspecting and acting on the filesystem without leaving the shell.

## 1. Getting Started

To launch trail, simply type `trail` in your terminal. You can optionally provide a starting path:
```bash
trail [<start-path>]
```
**Tip:** If you have configured shell integration, exiting Trail will automatically change your shell's current directory to the directory you were browsing when you exited.

## 2. Navigation Mode

Navigation Mode is the primary interface for browsing directories. It uses Vim-like bindings by default, but fallback arrows and standard keys are also supported.

### Movement & Traversal
- `j` or `↓`: Move selection down
- `k` or `↑`: Move selection up
- `l`, `Enter` or `→`: Enter directory or open file in your configured `$EDITOR`
- `h`, `Backspace` or `←`: Go to parent directory
- `gg`: Jump to top of the list
- `G`: Jump to bottom of the list
- `u`: Go back in navigation history
- `Ctrl-r`: Go forward in navigation history

### Tabs
Trail supports multiple tabs for multitasking.
- `Ctrl-t`: Open a new tab
- `Ctrl-w`: Close the current tab
- `Tab`: Switch to the next tab
- `Shift-Tab`: Switch to the previous tab

### File Operations
- `ya`: Copy absolute path of the selected item to clipboard
- `yr`: Copy path relative to the directory Trail was launched from
- `yn`: Copy filename to clipboard
- `yc`: Copy content to clipboard — file text, or directory listing
- `dd`: Delete the selected item (prompts for confirmation: `y`/`Enter` to confirm, `n`/`Esc` to cancel)
- `o`: Open the selected item with the OS default application

### Display Options
- `R`: Refresh the current directory view
- `.`: Toggle visibility of hidden files

### Mode Switching & Exit
- `/`: Enter Search Mode
- `:`: Enter Command Mode
- `q`: Quit Trail (if shell wrapper is sourced, your shell will cd to the last directory)
- `Ctrl-c`: Force quit without changing directory

## 3. Search Mode

Search Mode allows you to filter the current directory's contents. Type any characters to filter the list.

- `Enter` or `→`: Confirm search and return to Navigation Mode with the item selected
- `Esc`: Cancel search and exit Search Mode
- `j` / `↓`: Move selection down through search results
- `k` / `↑`: Move selection up through search results
- `Backspace` / `Ctrl-h`: Delete the last character of the search query

## 4. Command Mode

Command mode allows you to execute powerful filesystem operations and shell commands. Type `:` to enter Command Mode.

### Built-in Commands
- `:mkdir <name>`: Create a new directory inside the current directory
- `:touch <name>`: Create a new empty file
- `:rename <new_name>` (or `:ren`): Rename the currently selected item
- `:mv <dest>`: Move the selected item to a new destination (relative or absolute)
- `:cp <dest>`: Copy the selected item to a new destination
- `:git <subcommand>`: Run a git subcommand (e.g., `:git status --short`)
- `:set <key> <value>`: Set a runtime configuration value
- `:bookmark <name>` (or `:bm`): Bookmark the current directory (defaults to directory base name if no name provided)
- `:jump <name>` (or `:j`): Jump to a previously saved bookmark

### Shell Commands
You can run arbitrary shell commands by prefixing them with `!` instead of `:`.
- `!<command>`: Execute a shell command (e.g., `!ls -la`)

### Command Mode Features
- **History**: Use `↑` and `↓` arrows to scroll through previously executed commands.
- **Auto-completion**: Press `Tab` to cycle through command completions. Command verbs (e.g. `mkdir`, `mv`) and file paths (for `mv` and `cp` destinations) are auto-completed based on the current directory.

## 5. Configuration

You can customize Trail by supplying a configuration file. This allows overriding the default theme, keybindings, and general settings (like your preferred `$EDITOR`).

```bash
trail --config /path/to/trail.toml
```

Trail remembers that path, so a later plain `trail` reloads the same file — you only name it once. Use `trail --no-config` to run with the built-in defaults for a single run without forgetting it.

See the [configuration guide](configuration_guide.md) for the full schema.
