# Trail Configuration Guide

Trail provides strict, type-checked configuration options to customize your workspace, theme, keybindings, and editor preferences.

## 1. The Configuration File (`trail.toml`)

By default, Trail runs with built-in settings. To customize Trail, create a `trail.toml` configuration file and provide it at launch using the `--config` flag:

```bash
trail --config /path/to/trail.toml
```

*(Note: Trail does not scan `~/.config/trail/` or other standard directories for a config file. The only way a config file is ever loaded is by passing `--config` at least once — see below.)*

### Trail Remembers Your Config File

Since v1.0.1, the path you pass to `--config` is remembered. A later `trail` with no flags reloads the same file, so you only have to name it once:

```bash
trail --config ~/dotfiles/trail.toml   # loads it, and remembers the path
trail                                  # reloads ~/dotfiles/trail.toml
trail --config ~/other.toml            # switches, and remembers the new path
```

The path is stored as an **absolute** path in `last_config.toml` inside Trail's data directory, alongside `bookmarks.toml` and `recent_dirs.toml`:

| Platform | Location |
|---|---|
| Linux | `~/.local/share/trail/` |
| macOS | `~/Library/Application Support/trail/` |
| Windows | `%APPDATA%\trail\data\` |

An explicit `--config` always wins over the remembered path.

**If the remembered file is later moved, deleted, or broken**, Trail does not refuse to start. It falls back to the built-in defaults, shows the reason in the status bar, and keeps the remembered path — so fixing the file is enough to get your settings back, with no need to pass `--config` again.

By contrast, a file you name explicitly with `--config` that fails to load **is** a hard error: you asked for that file by name, so a silent fallback would hide a typo.

### Ignoring the Remembered Config

To run once with the built-in defaults, bypassing whatever is remembered:

```bash
trail --no-config
```

This affects only that run. The remembered path stays in place, so the next plain `trail` picks it up again. (`--no-config` and `--config` cannot be combined.) To forget the path permanently, delete `last_config.toml` from the data directory above.

### Configuring the Editor

Unlike some tools that implicitly read the `$EDITOR` environment variable, Trail explicitly relies on its own configuration property to determine which application opens files when you trigger the `enter_or_open` action (default: `Enter` or `l`). By default, this is set to `"vi"`.

You can configure your preferred editor in the `[general]` section of your TOML file:

```toml
[general]
editor = "nvim"
```

### Complete Configuration Schema

The TOML configuration is strictly validated. Unknown keys will cause Trail to fail to load the config. The file is divided into four main sections:

#### `[general]`
Controls overall application behavior.
- `editor` (String): Command used to open files. Must not be empty. (Default: `"vi"`)
- `text_sync_threshold_kb` (Positive Integer): Maximum file size in KiB to preview synchronously on the UI thread. Larger files skip synchronous preview. Must be > 0. (Default: `256`)
- `git_status_enabled` (Boolean): Enable or disable background git status workers. (Default: `true`)
- `fs_watch_debounce_ms` (Non-negative Integer): Debounce delay for filesystem watching in milliseconds. (Default: `200`)

#### `[theme]`
Customizes the UI colors.
**Valid Color Values:**
- Hex codes (must be exactly 7 characters starting with `#`, e.g., `"#112233"`).
- Named colors: `"black"`, `"red"`, `"green"`, `"yellow"`, `"blue"`, `"magenta"`, `"cyan"`, `"gray"`, `"grey"`, `"dark_gray"`, `"dark_grey"`, `"darkgray"`, `"darkgrey"`, `"white"`, `"reset"`.

**Available Properties:**
- `foreground`: Default text color.
- `background`: Default background color.
- `border`: Panel border color.
- `selection_fg`: Foreground color of the currently selected row.
- `selection_bg`: Background color of the currently selected row.
- `directory`: Color for directory entries.
- `symlink`: Color for symlink entries.
- `hidden`: Color for dotfiles/hidden entries.
- `status_fg`: Status bar text color.
- `error`: Error message text color.
- `search`: Search mode accent color.
- `command`: Command mode accent color.
- `git_clean`: Indicator color for a clean git repository.
- `git_dirty`: Indicator color for a dirty git repository.

#### `[keymap.navigation]`
Overrides keybindings for Navigation Mode. 
**Valid Key Formats:** 
- Single characters (`"j"`, `"/"`).
- Named keys: `"enter"`, `"esc"`, `"backspace"`, `"tab"`, `"left"`, `"right"`, `"up"`, `"down"`.
- Control chords: `"ctrl-r"`, `"ctrl-w"`.

**Allowed Action Names:**
- `move_down`: Move selection down
- `move_up`: Move selection up
- `jump_top`: Jump to first item
- `jump_bottom`: Jump to last item
- `enter_or_open`: Enter directory or open file
- `go_parent`: Navigate to parent directory
- `history_back`: Go backward in navigation history
- `history_forward`: Go forward in navigation history
- `refresh`: Manually refresh directory
- `toggle_hidden`: Toggle visibility of hidden files
- `copy_absolute_path`: Copy absolute path of selection
- `copy_relative_path`: Copy relative path of selection
- `copy_filename`: Copy filename of selection
- `delete`: Prompt to delete selection
- `enter_search`: Enter Search Mode
- `enter_command`: Enter Command Mode
- `quit`: Exit Trail
- `open_with_os`: Open selection with OS default handler
- `new_tab`: Open a new tab
- `close_tab`: Close current tab
- `switch_tab_next`: Switch to next tab
- `switch_tab_prev`: Switch to previous tab

#### `[keymap.search]`
Overrides keybindings for Search Mode (using the same valid key formats).
**Allowed Action Names:**
- `exit`: Leave Search Mode
- `confirm`: Select the currently matched item
- `move_down`: Scroll down in search results
- `move_up`: Scroll up in search results
- `delete_char`: Delete the last typed character in the search query

#### `[plugins]`
Enables specific Lua plugins to load at startup.
- `enabled` (Array of Strings): Names of the plugins to load. (e.g., `enabled = ["example_bookmarks"]`).

## 2. Runtime Configuration (`:set`)

You can modify settings dynamically while Trail is running by using the `:set` command in Command Mode (press `:`). 
Changes made via `:set` are validated exactly like the TOML config and take effect immediately, but **they are not saved** back to your `trail.toml` file.

### Syntax
```
:set <key> <value>
```

### Allowed Keys & Examples
You must use the section-qualified key (e.g., `theme.directory`), with the exception of `[general]` properties which have convenient short aliases.

**General Properties (Aliases supported):**
- `:set editor nvim` (or `:set general.editor nvim`)
- `:set text_sync_threshold_kb 512`
- `:set git_status_enabled false` (Accepts `true`, `yes`, `on`, `1` / `false`, `no`, `off`, `0`)
- `:set fs_watch_debounce_ms 500`

**Theme Properties (Requires `theme.` prefix):**
- `:set theme.background #1a1b26`
- `:set theme.directory blue`

**Keymap Properties (Requires `keymap.navigation.` or `keymap.search.` prefix):**
- `:set keymap.navigation.move_down n`
- `:set keymap.navigation.quit ctrl-q`
- `:set keymap.search.confirm enter`
