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

Once a second tab is open, the status bar shows which one has focus — `[2/3]`, just left of
the path. With a single tab there is no indicator, and `Tab` says so rather than appearing
to do nothing.

### File Operations
- `ya`: Copy absolute path of the selected item to clipboard
- `yr`: Copy path relative to the directory Trail was launched from
- `yn`: Copy filename to clipboard
- `yc`: Copy content to clipboard — file text, or directory listing
- `dd`: Delete the selected item (prompts for confirmation: `y`/`Enter` to confirm, `n`/`Esc` to cancel)
- `o`: Open the selected item with the OS default application

### Preview Pane
The preview scrolls independently of the selection, so you can read through a long
file without opening it and without losing your place in the listing.

- `J` or `Shift-↓`: Scroll the preview down one line
- `K` or `Shift-↑`: Scroll the preview up one line
- `Ctrl-f`: Scroll the preview down one page (a screenful, less two lines of overlap)
- `Ctrl-b`: Scroll the preview up one page
- `Shift-Home`: Jump back to the start of the preview
- `Shift-End`: Jump to the end of the loaded preview

When there is more content than fits, the bottom-right of the pane shows which
lines you are looking at, as `41–61/200`. A trailing `+` — `41–61/2000+` — means
the file carries on past the part Trail loaded; raise `[preview] max_lines` in your
config (or `:set max_lines 8000` for this session) to load more of it.

Moving to a different entry resets its preview to the top. Re-previewing the *same*
entry does not: if the file changes on disk while you are reading it, you stay where
you were.

Scrolling applies to text, binary metadata and directory previews. An image preview
has no lines to scroll, so the keys do nothing there.

### Display Options
- `R`: Refresh the current directory view
- `.`: Toggle visibility of hidden files

### Status Bar Messages

The middle of the status bar carries one message at a time. A failure is shown in the
theme's error colour and prefixed with `Error:`; an outcome worth confirming — a yank, a
saved bookmark, a count of files moved — is shown in the "clean" colour with no prefix.

A message lasts until your next keystroke, so it never follows you into a directory it has
nothing to do with. Everything shown there is also written to the log file (`trail --paths`
prints its location), which is where to look for the full text of a message that was too
long for the bar.

### Mode Switching & Exit
- `/`: Enter Search Mode
- `:`: Enter Command Mode
- `q`: Quit Trail (if shell wrapper is sourced, your shell will cd to the last directory)
- `Ctrl-c`: Force quit without changing directory

## 3. Search Mode

Search Mode allows you to filter the current directory's contents. Type any characters to filter the list.

Every unmodified character you press is typed into the query - `j` and `k` included.
Movement therefore uses keys that are not text:

- `↓` / `Ctrl-n`: Move selection down through search results
- `↑` / `Ctrl-p`: Move selection up through search results
- `Enter` or `→`: Confirm search and return to Navigation Mode with the item selected
- `Esc`: Cancel search and exit Search Mode
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

The command runs in the directory the nav panel is showing, and the whole string is handed
to a shell to interpret, so pipes, redirection, `&&` and shell builtins all work. That
shell is `cmd.exe /C` on Windows and `sh -c` elsewhere, and `:git` uses the same one.

Change it with `[general] shell` — see
[the configuration guide](configuration_guide.md#configuring-the-shell). On Windows the
usual reason to is that `cmd.exe` rejects a leading `./`: `!./gradlew runClient` fails with
`'.' is not recognized`, where `!.\gradlew runClient` and `!gradlew.bat runClient` both
work. Setting `shell = "pwsh -NoProfile -Command"` makes the `./` spelling work too.

### Command Mode Features
- **Editing**: The command line takes the whole status bar and shows the cursor where your
  next keystroke will land. `←`/`→` move it, `Home`/`End` jump to either end, `Backspace`
  and `Delete` remove either side of it. A command longer than the terminal scrolls to keep
  the cursor in view.
- **History**: Use `↑` and `↓` arrows to scroll through previously executed commands.
- **Auto-completion**: Press `Tab` to cycle through command completions. Command verbs (e.g. `mkdir`, `mv`) and file paths (for `mv` and `cp` destinations) are auto-completed. A destination that is a whole route is completed against the directory that route names, not against the current directory, so `Tab` walks into `nested/`, `../dst/` or an absolute path you pasted in. Directory candidates come back with a trailing separator, so you can keep pressing `Tab` to descend.

### Paths on Windows

`:mv` and `:cp` take either separator: `:mv ..\backup`, `:mv ../backup` and `:mv C:\Users\me\backup` all work, and completion answers in whichever one you typed.

Trail shows and copies paths in the plain `C:\Users\me` form rather than the extended-length `\\?\C:\Users\me` form Windows returns internally — in the nav panel title, in the status bar, in `ya` (yank absolute path) and in the directory handed back to your shell on exit. The one exception is a path the plain form cannot address, such as one longer than 260 characters: there the `\\?\` prefix is kept, because dropping it would produce a path Windows rejects.

## 5. Configuration

You can customize Trail by supplying a configuration file. This allows overriding the default theme, keybindings, and general settings (like your preferred `$EDITOR`).

```bash
trail --config /path/to/trail.toml
```

Trail remembers that path, so a later plain `trail` reloads the same file — you only name it once. Use `trail --no-config` to run with the built-in defaults for a single run without forgetting it.

See the [configuration guide](configuration_guide.md) for the full schema.
