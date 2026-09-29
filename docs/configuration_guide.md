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

### Configuring the Shell

Two commands hand a string to a shell to interpret rather than spawning a program
directly: `!<command>` and `:git <subcommand>`. That is deliberate — it is what makes
pipelines, redirection, quoting and shell builtins work inside `!`.

By default the shell is `cmd.exe /C` on Windows and `sh -c` everywhere else. Note that
this is `sh`, not `$SHELL`: the command you type is interpreted as POSIX shell syntax, so
your interactive shell's aliases and functions are not in scope, and a shell with
different syntax (fish) would not be a safe substitute without asking.

The `shell` key changes that choice:

```toml
[general]
shell = "pwsh -NoProfile -Command"
```

The value is a program followed by the flags that make it read a command string as one
argument — the command is always appended as the final argument. Whitespace separates
tokens, and a double-quoted run is a single token, which is how you write a program path
containing spaces.

**Windows users:** the default `cmd.exe` treats a leading `/` as the start of a switch, so
`:!./program` fails with `'.' is not recognized as an internal or external command`. Three
ways out: spell it `:!.\program`, drop the prefix entirely (`:!program.exe` — `cmd.exe`
searches the current directory), or set `shell` to PowerShell, which resolves `./program`
the way you would expect:

```toml
[general]
# A TOML *literal* string (single quotes) — in a normal double-quoted TOML string
# every backslash would have to be doubled, and `\P` is a TOML parse error.
shell = '"C:\Program Files\PowerShell\7\pwsh.exe" -NoProfile -Command'
```

The trade-offs in picking a non-default shell are startup cost — PowerShell takes
noticeably longer to launch than `cmd.exe` per command, and `-NoProfile` is what stops
your profile script adding to that — and that the builtins available to `!` change with it.

Two things the key deliberately does not touch: the editor (`enter_or_open` spawns
`editor` directly, with no shell in between) and `open_with_os` (`o`), which must keep
using the platform's own handler — `cmd.exe /C start` on Windows, `open` on macOS,
`xdg-open` on Linux.

### Complete Configuration Schema

The TOML configuration is strictly validated. Unknown keys will cause Trail to fail to load the config. The file is divided into five main sections:

#### `[general]`
Controls overall application behavior.
- `editor` (String): Command used to open files. Must not be empty. (Default: `"nvim"`)
- `shell` (String): Shell that interprets `!<command>` and `:git`, written as a program plus the flags that make it read a command string — e.g. `"pwsh -NoProfile -Command"`. Empty means the platform default: `cmd.exe /C` on Windows, `sh -c` elsewhere. Rejected if it leaves a quote unterminated or names an empty program. (Default: `""`)
- `shell_pause` (String): When to wait for Enter after a `!<command>` or `:git` finishes, before Trail takes the screen back — `"always"`, `"on_error"` or `"never"`. A command prints on the normal screen, which Trail covers again as soon as the command exits, so without a pause the output of `!ls` is erased in the same breath it was written. `"on_error"` holds the screen only when the command failed (including when it could not be started at all). Opening a file in the editor never pauses, whatever this says: an editor owns the screen while it runs and leaves nothing behind to read. (Default: `"always"`)
- `text_sync_threshold_kb` (Positive Integer): How much of a file, in KiB, the preview reads. Together with `[preview] max_lines` this bounds one preview: `max_lines` stops an ordinary file, this stops one whose lines are enormous (minified JavaScript, a single-line JSON blob) and which would otherwise be loaded whole however few lines it has. A preview cut short by either is marked with a `+` in the pane's footer. The name is historical - it once chose which files were previewed on the UI thread, which nothing is any more. Must be > 0. (Default: `256`)
- `delete_mode` (String): Where `dd` sends the selected entry — `"trash"` or `"permanent"`. `"trash"` hands it to the platform's recycle bin, so a mistake can be undone from the desktop without Trail's help; this matters most because `dd` on a directory takes everything under it. A path the recycle bin will not take (some network shares and removable volumes) is reported as an error rather than deleted anyway. `"permanent"` unlinks it, recovering nothing. The confirmation prompt says which one is about to happen. (Default: `"trash"`)
- `show_version` (Boolean): Show Trail's version on the preview pane's top border. A diagnostic rather than decoration: `trail --version` cannot be asked of a session that is already *running*, and on Windows an upgrade cannot overwrite a running `trail.exe` — it renames the old one aside — so several open windows can be on different builds and look identical. The badge stands down when the selected file's name needs the whole border, because the name is what the pane is about. (Default: `false`)
- `git_status_enabled` (Boolean): Enable or disable background git status workers. (Default: `true`)
- `fs_watch_debounce_ms` (Non-negative Integer): Debounce delay for filesystem watching in milliseconds. (Default: `200`)

#### `[navigation]`
Controls how the navigation panel orders the listing and what it shows beside each name.

- `sort_by` (String): Which property the listing is ordered by — `"name"`, `"size"`, `"modified"` or `"extension"`. `"size"` puts the largest first and `"modified"` the most recent first, the way `ls -S` and `ls -t` do; `"name"` and `"extension"` run A–Z, and `"extension"` falls back to the name within each group. Ties are always broken by name, case-insensitively, so an order does not shuffle between refreshes. Note that this key **seeds the first tab only**: each tab then owns its order, so `ss` in a downloads tab leaves the source tree in the next tab alone, and a new tab inherits the order of the tab it was opened from. There is deliberately no `"created"` — many Unix filesystems do not record a creation time, so that sort would work on Windows and silently tie on Linux. (Default: `"name"`)
- `sort_reverse` (Boolean): Flip whichever order `sort_by` selected. (Default: `false`)
- `dirs_first` (Boolean): Group directories ahead of files whatever `sort_by` says. This is an axis of its own — `sort_reverse` does *not* flip it, because reversing a listing is asking for the files in the other order, not for the directories to move to the bottom. A directory has no meaningful size, so directories keep name order inside a size sort whether or not they are grouped. (Default: `true`)
- `entry_details` (String): What each row shows beside the name — `"none"`, `"size"`, `"modified"` or `"both"`. The navigation panel is 40% of the screen, which is 28 usable columns on an 80-column terminal, and `"both"` wants 26 of them; rather than crushing names, the column **stands down entirely** when it cannot leave at least 12 columns for a name, and `"both"` first drops the clock time and keeps the date. A directory shows `—` for its size, because a directory's byte length describes its directory record rather than its contents. (Default: `"none"`)
- `scroll_margin` (Non-negative Integer): Rows kept between the selection and the top or bottom of the panel while you move through a listing, like vim's `scrolloff`. The margin gives way at the true ends of the list, so the first entry still reaches the top row and the last entry the bottom row. `0` lets the selection ride the edge of the panel; a value larger than half the panel keeps the selection centred. (Default: `3`)

The default reproduces Trail's ordering before v1.8.0 exactly: directories first, then name,
case-insensitive, with no details column.

#### `[preview]`
Controls the preview pane, including inline image rendering.

- `image_protocol` (String): Which terminal graphics protocol to draw images
  with. (Default: `"auto"`)

  | Value | Meaning |
  |---|---|
  | `"auto"` | Detect from the environment (see below). |
  | `"kitty"` | Kitty graphics protocol — Kitty, Ghostty, Konsole, WezTerm. |
  | `"iterm2"` | iTerm2 inline images — iTerm2, WezTerm, mintty, Tabby, Hyper, VS Code. |
  | `"sixel"` | Sixel graphics — mlterm, foot, xterm built with sixel support. |
  | `"halfblocks"` | Unicode half-blocks. Works in every terminal, at half the vertical resolution. |
  | `"none"` | Disable image previews; show metadata only. |

- `image_cell_width` (Integer): Width, in pixels, of one terminal character
  cell, or `0` to work it out. (Default: `0`)
- `image_cell_height` (Integer): Height, in pixels, of one terminal character
  cell, or `0` to work it out. (Default: `0`)
- `max_lines` (Integer, `1`–`100000`): How many lines of a text file the preview
  loads. (Default: `2000`)

  The preview scrolls within this window (`J`/`K`, `Shift`+arrows, `Ctrl-f`/
  `Ctrl-b`), and the pane's footer marks a file that continues past it with a
  `+` — so a truncated preview says so rather than looking like the whole file.
  Raising the value lets you scroll further into long files; each preview then
  costs the highlight worker proportionally more time and memory. The work happens
  off the UI thread either way, so navigation stays responsive.

  Read when a preview is requested, not at render time: `:set max_lines 8000`
  therefore applies from the next preview onwards. Move off the entry and back to
  reload the one on screen.

##### How detection works

No portable way exists to ask a terminal what it supports — the query requires a
raw read on the same stdin the event loop owns, and is unavailable on Windows
without unsafe code, which Trail forbids. Detection therefore reads environment
variables, in this order:

1. `KITTY_WINDOW_ID`, or `TERM` containing `kitty` → **kitty**
2. `ITERM_SESSION_ID`, or `TERM_PROGRAM`/`LC_TERMINAL` containing `iterm` → **iterm2**
3. `WEZTERM_EXECUTABLE`, or `TERM_PROGRAM=WezTerm` → **iterm2**
4. `TERM_PROGRAM` naming mintty, VS Code, Tabby, Hyper, Rio or Warp → **iterm2**
5. `TERM` naming sixel, mlterm, yaft or foot → **sixel**
6. Anything else → **halfblocks**

The last step matters: an unrecognised terminal still draws the image, just at
lower fidelity. Image previews are never silently skipped — only
`image_protocol = "none"` turns them off.

If detection picks wrong for your terminal, name the protocol explicitly rather
than working around it.

##### Tuning the cell size

A pixel protocol is told how large the image is *in pixels*, and the terminal
draws it across however many cells those pixels cover. Trail therefore has to
know the terminal's character cell size to fit an image to the preview pane.

At `0`, Trail asks the platform for it. That works on Linux and macOS when the
kernel reports the terminal's pixel size; it is **never** available on Windows,
where neither `crossterm` nor `ratatui-image` implements it. When it cannot be
measured, Trail assumes `7 × 14`.

That assumption deliberately errs *low*, because the two failure directions are
not symmetric:

- **Too small** → the image covers fewer cells than the pane, and renders
  correctly with a margin around it. Cosmetic.
- **Too large** → the image covers more cells than the pane reserved. Trail
  paints the rest of the interface over the overflow, and terminals respond by
  dropping the whole image rather than the covered part of it, so the preview
  disappears — typically the moment you resize the window.

So if images are missing or flicker away when you resize, **lower** these
numbers; if they render correctly but look smaller than the pane, raise them.
The preview caption shows the protocol and the cell size in use, and both keys
work with `:set` (see below), so you can tune them live and watch the result:

```
:set image_cell_width 8
:set image_cell_height 17
```

To find the real values, divide your terminal window's pixel size by its size in
columns and rows. In WezTerm, `wezterm ls-fonts` reports the cell metrics for
your configured font and size. Setting one axis and leaving the other at `0` is
fine — each is resolved independently.

##### Supported formats

PNG, JPEG, GIF, BMP, ICO, TIFF, WebP and AVIF are decoded. SVG is listed as an
image extension but is not rasterised — it falls back to a metadata preview
reporting the decode failure.

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
- `copy_relative_path`: Copy path of selection relative to the launch directory
- `copy_filename`: Copy filename of selection
- `copy_content`: Copy content of selection (file text, or directory listing)
- `delete`: Prompt to delete selection
- `enter_search`: Enter Search Mode
- `enter_command`: Enter Command Mode
- `quit`: Exit Trail
- `open_with_os`: Open selection with OS default handler
- `new_tab`: Open a new tab
- `close_tab`: Close current tab
- `switch_tab_next`: Switch to next tab
- `switch_tab_prev`: Switch to previous tab
- `preview_scroll_down`: Scroll the preview pane down one line (default `J`)
- `preview_scroll_up`: Scroll the preview pane up one line (default `K`)
- `preview_page_down`: Scroll the preview pane down one page (default `ctrl-f`)
- `preview_page_up`: Scroll the preview pane up one page (default `ctrl-b`)
- `preview_scroll_top`: Jump to the start of the preview (default `shift-home`)
- `preview_scroll_bottom`: Jump to the end of the loaded preview (default `shift-end`)

`Shift-↓` and `Shift-↑` are built-in aliases for `preview_scroll_down` and
`preview_scroll_up`, the same way the plain arrows alias `move_down`/`move_up`.
Rebinding those two actions does not remove the arrow aliases.

Shift is spelled out only on keys that are not text — `shift-down`, `shift-home`,
`shift-end`, `shift-tab`. A shifted character is written as the capital itself:
`G`, `J`, `K`.

#### `[keymap.search]`
Overrides keybindings for Search Mode.

> **Single characters are not valid here.** Search Mode is a typing mode: every
> unmodified character is appended to the query, so binding one to an action
> would make that character impossible to search for. Use a named key (`up`,
> `down`, `enter`, `esc`, `backspace`) or a chord (`ctrl-n`). A single-character
> binding is rejected at load and by `:set`, with a message saying why.
>
> This is why the defaults are `down` and `up` rather than `j` and `k`.
> `Ctrl-n` / `Ctrl-p` / `Ctrl-h` also work as built-in aliases that no config
> is needed to enable.

**Allowed Action Names:**
- `exit`: Leave Search Mode
- `confirm`: Select the currently matched item
- `move_down`: Scroll down in search results
- `move_up`: Scroll up in search results
- `delete_char`: Delete the last typed character in the search query

#### `[plugins]`
Enables specific Lua plugins to load at startup.
- `enabled` (Array of Strings): Names of the plugins to load, in order — hooks run in this order. A name is looked for as a built-in plugin (`"bookmarks"`), then as `<name>.lua` in Trail's config directory. A plugin that fails to load is named in the status bar at startup. See [`plugins.md`](plugins.md). (e.g., `enabled = ["bookmarks", "git_line"]`).
- `budget_ms` (Positive Integer): How long one call into a plugin may run before Trail stops it and reports the plugin. Plugin hooks run on the UI thread, so this bounds how long a plugin can hold up a keystroke. It cannot interrupt a single blocking call such as `os.execute` — use `trail.spawn` for slow work, which runs off the UI thread and is not budgeted. Loading a plugin gets ten times this. (Default: `50`)

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
- `:set shell pwsh -NoProfile -Command` (everything after the key is the value, so the flags come along; applies to the next `!` command)
- `:set shell_pause on_error` (applies to the next `!` command)
- `:set delete_mode permanent` (applies to the next `dd`)
- `:set show_version true` (read at render time, so it appears on the next frame)
- `:set text_sync_threshold_kb 512`
- `:set git_status_enabled false` (Accepts `true`, `yes`, `on`, `1` / `false`, `no`, `off`, `0`)
- `:set fs_watch_debounce_ms 500`

**Navigation Properties (Aliases supported):**
- `:set sort_by size` (or `:set navigation.sort_by size`) — re-orders the active tab immediately
- `:set sort_reverse true`
- `:set dirs_first false`
- `:set entry_details both`
- `:set scroll_margin 5` (read at render time, so it applies on the next frame)

**Plugin Properties:**
- `:set budget_ms 200` (or `:set plugins.budget_ms 200`) — applies to the next call into a plugin

The three sort keys change the **active tab only**, the same as `:sort` and the `s` bindings;
they do not reach the other tabs or the config file. `entry_details` applies to the whole
window, because it describes the panel rather than a place you are working.

**Preview Properties (Aliases supported):**
- `:set image_protocol halfblocks` (or `:set preview.image_protocol halfblocks`)
- `:set image_cell_width 8`
- `:set image_cell_height 17`

Preview changes apply to the next preview, so move the selection off the image
and back to see the effect.

**Theme Properties (Requires `theme.` prefix):**
- `:set theme.background #1a1b26`
- `:set theme.directory blue`

**Keymap Properties (Requires `keymap.navigation.` or `keymap.search.` prefix):**
- `:set keymap.navigation.move_down n`
- `:set keymap.navigation.quit ctrl-q`
- `:set keymap.search.confirm enter`
