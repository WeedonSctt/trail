# Trail User Guide

Trail is a terminal-first workspace for navigating, inspecting and acting on the filesystem without leaving the shell.

## 1. Getting Started

To launch trail, simply type `trail` in your terminal. You can optionally provide a starting path:
```bash
trail [<start-path>]
```
**Tip:** If you have configured shell integration, exiting Trail will automatically change your shell's current directory to the directory you were browsing when you exited.

### Reading the screen

Trail is three panels, and the panel **borders** carry most of what you need to know. A
border is drawn whether or not anything is written on it, so nothing here costs you a row
of listing:

```
      directory name            sort order    selected file        version (optional)
             │                       │              │                       │
             ▼                       ▼              ▼                       ▼
     ╭ my-project ─────────── size↓ ╮╭ README.md ─────────────────── v1.11.0 ╮
     │>  src/                      —││    1  # Trail                         │
     │   README.md          17.06 kB││    2                                  │
     │   Cargo.toml          3.22 kB││    3  A terminal-first workspace…     │
     ╰ …\proj\it\util\trail ────────╯╰───────────────────────────── 1–24/312 ╯
                  ▲           ▲                                          ▲
                  │           │                                          │
            where you are   details column                     position in the file

      NORMAL   [2/3]                         main *          3 items
         ▲        ▲                            ▲                ▲
        mode    which tab                  git branch      entry count
```

- **Top-left of the listing** — the directory you are in, by name.
- **Top-right of the listing** — the sort order in use (`size↓`), per tab. See
  [Sorting](#sorting).
- **Bottom of the listing** — the full path. Cut from the front (`…\util\trail`) when it
  does not fit, because the end is the part that answers "where am I".
- **Right of each row** — optionally the size or modification time. Off by default; see
  [Display Options](#display-options).
- **Top-left of the preview** — the selected entry's name.
- **Top-right of the preview** — Trail's version, if `[general] show_version` is on.
- **Bottom-right of the preview** — how far through the file you are, with a trailing `+`
  when the file continues past what was loaded.
- **The bottom row** — mode, which tab has focus, the git branch, the entry count, and any
  message Trail has for you.

The one thing worth knowing about that last row: **Command Mode takes all of it**, so
while you are typing a `:` command it is the command line and nothing else. That is why
the path lives on a border rather than down there — you can read where you are while
typing a destination relative to it.

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
- `U`: Return from a jump. After `:jump` takes you to a bookmark, wander as far as you like;
  `U` goes straight back to where you jumped from, where `u` would step back one directory
  at a time. Press it again to return from the jump before that. Each tab keeps its own jumps

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
- `dd`: Delete the selected item (prompts for confirmation: `y`/`Enter` to confirm, `n`/`Esc` to cancel).
  It goes to your recycle bin, so it can be restored from there — the prompt reads
  `Recycle 'name'?`. Set `[general] delete_mode = "permanent"` to unlink it instead, after
  which the prompt reads `Delete 'name'?` and nothing can be recovered. Either way, on a
  directory this takes everything inside it.
- `o`: Open the selected item with the OS default application

### Marking, and Pasting Somewhere Else
Pick files in one place and copy or move them to another — even another tab.

- `Space`: Mark the selected item (or unmark it). The selection stays where it is; to
  mark a run of items, use visual mode (`v`, below). A marked item has a `*` in front of
  its name.
- `Esc`: Clear every mark. `:unmark` does the same.
- `pp`: Copy every marked item into the directory you are looking at. The marks stay, so
  you can paste the same set again somewhere else.
- `pm`: Move every marked item here. Moved items leave the mark set.
- `v`: Visual mode, for marking a range. `v` anchors it on the selection; `j`/`k`, the
  arrows, `G` and `gg` extend it, and it is highlighted as you go (the status bar reads
  `VISUAL` and `4 selected`). Then:
  - `Space` or `v` adds the range to the marks;
  - `h` or `l` adds it and then goes up or in, so selecting and leaving is one key;
  - `dd` deletes the range — only the range, not other marks — after one confirmation;
  - `Esc` abandons it and leaves the marks as they were.

Marks survive navigation and are shared by every tab; the status bar shows `3 marked` for
as long as any exist, because they are usually out of sight. A name already taken where you
paste is skipped and reported (`paste: 2 copied, 1 skipped: exists`) — nothing is ever
overwritten — and a marked file deleted in the meantime is dropped and counted. Pasting runs
in the background, like `:cp`. Marks are not saved: they end when Trail does.

**While anything is marked, `:mv`, `:cp` and `dd` act on the marks, not the selection.**
`:mv ..\archive\` moves every marked entry there (with several marked, the destination has to
be an existing directory), and `dd` asks once for the whole set — `Recycle 4 marked?`.
`:mv *.md docs\` still names its own files, marks or not.

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

A preview cut short also says so in words, on the bottom-left of its border, at any
scroll position: `preview limit: first 2000 lines · max_lines` when the line limit
stopped it, or `preview limit: first 256 KB · text_sync_threshold_kb` when the byte
limit did (a file of a few enormous lines, such as minified JavaScript). The key named
is the one to raise. On a narrow pane the notice shortens to `preview limit`.

A file with nothing in it shows `(empty file)`, so it cannot be mistaken for a preview
that failed. Text that is not UTF-8 is still shown: a byte-order mark selects UTF-16 or
UTF-8, and anything else that is not valid UTF-8 is read as Windows-1252, the "ANSI"
encoding Windows editors save Western European text in, so `café` reads as `café`.

Moving to a different entry resets its preview to the top. Re-previewing the *same*
entry does not: if the file changes on disk while you are reading it, you stay where
you were.

Scrolling applies to text, binary metadata and directory previews. An image preview
has no lines to scroll, so the keys do nothing there.

### Display Options
- `R`: Refresh the current directory view
- `.`: Toggle visibility of hidden files
- `m`: Step the details column through none → size → modified → both → none

The details column shows each entry's size, modification time, or both, right-aligned
beside the name. A directory shows `—` rather than a size, because what the filesystem
reports for a directory is the size of its own record and not of what is in it. On a
narrow panel the column stands down entirely rather than crush the names — widen the
terminal and it comes back. Set `[navigation] entry_details` in `trail.toml` for the
setting to persist across sessions.

In a listing longer than the panel, the selection stays a few rows away from the top and
bottom edges while you move, so you can see what is coming — vim calls this `scrolloff`.
The margin gives way at the ends of the listing, so the first and last entries still reach
the edges. `[navigation] scroll_margin` sets it (default `3`); `0` lets the selection ride
the edge, and `:set scroll_margin 5` tries a value without restarting.

**Which build am I running?** `[general] show_version = true` puts Trail's version on the
preview pane's top border, or `:set show_version true` for the current session. It is off
by default. This is worth turning on if you upgrade often: on Windows an upgrade cannot
overwrite a running `trail.exe` — the installer renames the old one aside — so sessions
you left open keep running the previous build and are otherwise indistinguishable from
new ones.

**Which tab is Trail in?** While it runs, Trail titles the terminal tab `trail <directory>`
— in WezTerm, Windows Terminal and most others that is the text on the tab — and puts the
shell's title back when it exits. `[general] terminal_title` chooses the wording: `"dir"`
(the default) names the directory, `"path"` gives its full path, and `"off"` leaves the
title alone.

The label also tells a release from a **dev build**. A release reads `v1.11.0`. A binary
built from anything other than the tagged, unmodified release commit reads like
`v1.11.0-dev.3+a1b2c3d`: three commits past v1.11.0, built from commit `a1b2c3d`. A trailing
`.dirty` means it was built with uncommitted changes. On a dev build the badge is drawn in
the `[theme] git_dirty` colour, so it stands out before you read it. `trail --version`
prints the same label, and for a dev build adds a sentence saying what it is:

```
trail 1.11.0-dev.3+a1b2c3d (dev build — 3 commits past v1.11.0)
```

So when something unreleased needs testing, the badge says which build you are in and
that it is not a release; once the release is installed it goes back to a plain `v1.10.2`.

### Sorting

`s` is a prefix; the second key picks what the listing is ordered by.

- `sn`: Sort by name (A–Z)
- `ss`: Sort by size, largest first
- `st`: Sort by modification time, most recent first
- `se`: Sort by extension, then name
- `sr`: Flip the current order
- `sd`: Toggle whether directories are grouped ahead of files

The order in use is always shown on the right-hand end of the navigation panel's top
border, so you never have to press a key to find out what you are looking at:

```
╭ my-project ─────────── size↓ ╮
│>  src/                      —│
│   README.md          17.06 kB│
│   Cargo.toml          3.22 kB│
╰──────────────────────────────╯
```

The arrow is the direction values run as you read *down* the list — `↓` for largest,
newest or Z-first, `↑` for the other end. `mixed` is added when directories are not
grouped ahead of files. The badge is per tab, like the sort itself, so it changes as you
switch tabs; on a very narrow panel it gives up its room to the directory name.

Size and time run largest-first and newest-first because that is the end of the range you
usually went looking for — `ls -S` and `ls -t` read the same way — and `sr` flips whichever
order is active. Directory grouping is separate: `sr` does not move directories to the
bottom, `sd` is what controls that.

**Sorting is per tab.** A downloads tab can sit in `st` while the tab next to it keeps a
source tree in `sn`, and each comes back in the order you left it. A new tab inherits the
order of the tab it was opened from. None of this is written to `trail.toml`; set
`[navigation] sort_by` there for the order every session should start in.

Two things are worth knowing. A directory has no meaningful size — what the filesystem
reports is the size of its directory record — so directories keep name order inside a size
sort, and the details column shows `—` rather than a misleading number. And under `st`, the
listing re-orders when a file is saved, because the filesystem watcher refreshes the view;
the selection follows the file it was on rather than staying on the row.

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
- `Ctrl-.`: Show or hide the terminal panel — see [Terminal Panel](#5-terminal-panel)
- `F12`: Move the keyboard between the file list and the terminal panel

## 3. Search Mode

Search Mode allows you to filter the current directory's contents. Type any characters to filter the list.

Every unmodified character you press is typed into the query - `j` and `k` included.
Movement therefore uses keys that are not text:

- `↓` / `Ctrl-n`: Move selection down through search results
- `↑` / `Ctrl-p`: Move selection up through search results
- `Enter` or `→`: Open the match — enter it if it is a directory, or open a file in your editor — and return to Navigation Mode
- `Esc`: Exit Search Mode, leaving the cursor on the selected match — so `dd`, `o`, `:` and
  the rest act on what you found
- `Backspace` / `Ctrl-h`: Delete the last character of the search query

## 4. Command Mode

Command mode allows you to execute powerful filesystem operations and shell commands. Type `:` to enter Command Mode.

### Built-in Commands
- `:mkdir <name>`: Create a new directory inside the current directory
- `:touch <name>`: Create a new empty file
- `:rename <new_name>` (or `:ren`): Rename the currently selected item
- `:mv <dest>`: Move the selected item to a new destination (relative or absolute)
- `:cp <dest>`: Copy the selected item to a new destination
- `:mv <pattern> <dir>` / `:cp <pattern> <dir>`: Move or copy every entry in the current
  directory whose name matches `pattern` into `dir`, e.g. `:mv *.md notes`. `*` stands for
  any run of characters and `?` for exactly one; a wildcard in the first word is what tells
  Trail you mean several files, so a destination containing a space still works as before.
  The destination has to be an existing directory, hidden entries are skipped unless the
  pattern itself starts with a dot, and the status bar reports how many were moved
- `@name` in a `:mv` or `:cp` destination is a bookmark: `:mv @docs` moves the selection into
  the directory bookmarked as `docs`, and `:mv @docs\next\route\` into a route under it.
  `Tab` after `@` offers your bookmark names, and then the folders inside the one you picked.
  An unknown name is an error rather than a folder called `@docs`; to reach a real folder
  whose name starts with `@`, write `.\@folder`
- `:git <subcommand>`: Run a git subcommand (e.g., `:git status --short`)
- `:set <key> <value>`: Set a runtime configuration value, for this session
- `:config write`: Save this session's `:set` changes into your `trail.toml`, keeping its comments
- `:sort <key> [reverse]`: Re-order the active tab's listing. `<key>` is `name`, `size`,
  `modified` or `extension`; the two words may be given in either order, and `:sort reverse`
  on its own flips whichever order is already in use
- `:bookmark <name>` (or `:bm`): Bookmark the current directory (defaults to directory base name if no name provided)
- `:jump <name>` (or `:j`): Jump to a previously saved bookmark
- `:unmark`: Clear every mark (the same as `Esc` in Navigation Mode)
- `:term`, `:term new [profile]`, `:term close`, `:term <n>`, `:term max`: Drive the
  terminal panel — see [Terminal Panel](#5-terminal-panel)

`:mv` and `:cp` run in the background, so a large copy never freezes Trail: the status bar
shows `copying 3/10` while it works, and reports what happened when it is done. They never
overwrite: an entry whose name is already taken at the destination is skipped and counted
(`cp: 4 copied, 1 skipped: exists`). Moving to another drive works too — Trail copies and
then removes the original, and only once the copy has fully succeeded.

### Shell Commands
You can run arbitrary shell commands by prefixing them with `!` instead of `:`.
- `!<command>`: Execute a shell command (e.g., `!ls -la`)

The command runs in the directory the nav panel is showing, and the whole string is handed
to a shell to interpret, so pipes, redirection, `&&` and shell builtins all work. That
shell is `cmd.exe /C` on Windows and `sh -c` elsewhere, and `:git` uses the same one.

While the command runs, Trail steps aside and gives it the whole terminal. When it
finishes, Trail waits for you to press Enter before taking the screen back — otherwise its
output would be erased the instant it appeared. Set `[general] shell_pause` to `on_error` to
be held up only when a command fails, or to `never` if you would rather not press a key.
Opening a file in the editor never waits.

Change it with `[general] shell` — see
[the configuration guide](configuration_guide.md#configuring-the-shell). On Windows the
usual reason to is that `cmd.exe` rejects a leading `./`: `!./gradlew runClient` fails with
`'.' is not recognized`, where `!.\gradlew runClient` and `!gradlew.bat runClient` both
work. Setting `shell = "pwsh -NoProfile -Command"` makes the `./` spelling work too.

### Command Mode Features
- **Editing**: The command line takes the whole status bar and shows the cursor where your
  next keystroke will land. `←`/`→` move it, `Home`/`End` jump to either end, `Backspace`
  and `Delete` remove either side of it. A command longer than the terminal scrolls to keep
  the cursor in view. The current directory stays visible on the navigation panel's bottom
  border throughout, so a relative `:mv` or `:cp` destination can be typed against
  something you can still see.
- **History**: Use `↑` and `↓` arrows to scroll through previously executed commands.
- **Repeat**: Press `@` in Navigation Mode to do the last file operation again — the last
  `:` or `!` command, or `dd`, `pp` or `pm`, whichever came last. Like Vim's `.`, it acts on
  what is selected *now*: `:mv archive`, `j`, `@` files the next entry too, `dd`, `j`, `@`
  deletes the next one (still asking first), and with marks it acts on the marks. Moving,
  marking and yanking are not operations, so they never replace what `@` will repeat. `@` works however
  your keyboard types it — `Shift`+`2` on US English, `AltGr`+`Q` on Spanish, Latin American
  or German layouts.
- **Auto-completion**: Press `Tab` to cycle through command completions. Command verbs (e.g. `mkdir`, `mv`) and file paths (for `mv` and `cp` destinations) are auto-completed. A destination that is a whole route is completed against the directory that route names, not against the current directory, so `Tab` walks into `nested/`, `../dst/` or an absolute path you pasted in. Directory candidates come back with a trailing separator, so you can keep pressing `Tab` to descend.

### Paths on Windows

`:mv` and `:cp` take either separator: `:mv ..\backup`, `:mv ../backup` and `:mv C:\Users\me\backup` all work, and completion answers in whichever one you typed.

Trail shows and copies paths in the plain `C:\Users\me` form rather than the extended-length `\\?\C:\Users\me` form Windows returns internally — on the nav panel's borders, in `ya` (yank absolute path) and in the directory handed back to your shell on exit. The one exception is a path the plain form cannot address, such as one longer than 260 characters: there the `\\?\` prefix is kept, because dropping it would produce a path Windows rejects.

## 5. Terminal Panel

The terminal panel is a real shell inside Trail: a panel under the file list and the preview
where you can type commands, see their output, and run anything you would run in a terminal —
`git commit` without `-m`, `vim`, `ssh`, a build. Unlike `!`, Trail does not step aside while
it runs: you can go back to browsing with the shell still open, and come back to it.

```
┌ Trail ──────────────┐┌ preview ─────────────────────┐
│> src/               ││ ...                          │
└─────────────────────┘└──────────────────────────────┘
┌ 1:pwsh │ 2:gitbash ─────────────────────────────────┐
│C:\proj\trail> cargo test                             │
│C:\proj\trail> _                                      │
└─────────────────────────────────────────────────────┘
 TERMINAL  2/2 gitbash
```

### Keys

| Key | Does |
|---|---|
| `Ctrl-.` | Show the panel and give it the keyboard — starting a shell if there is none — or hide it. Hiding never stops a shell. |
| `F12` | Move the keyboard to the other side: from the file list to the shell, or back. The panel stays open. With the panel hidden, it opens it. |
| `Alt-Shift-PageDown` / `Alt-Shift-PageUp` | Next / previous shell |
| `Alt-PageUp` / `Alt-PageDown` | Scroll back through the shell's output. Typing anything returns to the bottom. |

While the shell has the keyboard the status bar says **TERMINAL**, and **every other key
goes to the shell** — `q`, `Esc`, `Ctrl-c` and all of Trail's own keys included. That is
what makes it a real terminal; `F12` is the way back. All six keys can be changed under
[`[keymap.terminal]`](configuration_guide.md#keymapterminal).

`F12` was chosen because no shell, editor or common terminal program uses it, and every
terminal sends it the same way. `Ctrl-.` needs your terminal to report the `Ctrl` with the
`.` — Windows Terminal does. On a terminal that does not, the key arrives as a plain `.`,
which toggles hidden files; bind `toggle` to something else, such as `f11` or `alt-t`.

### Commands

| Command | Does |
|---|---|
| `:term` | Same as `Ctrl-.` |
| `:term new [profile]` | Start another shell in the folder Trail is showing, from the default profile or the one named |
| `:term close` | End the current shell — asks first if it is running a command |
| `:term <n>` | Switch to shell number *n*, as the tabs are numbered |
| `:term max` | Make the panel fill the screen, for `vim` or long output; again to restore |

### How it behaves

- **Each shell starts in the folder Trail is showing.** After that the two are independent:
  `cd` in the shell does not move Trail, and browsing in Trail does not move the shell.
  Quitting Trail hands your shell wrapper *Trail's* folder, as always.
- **Shells keep running while the panel is hidden**, and are there as you left them when you
  show it again. A shell that exits (`exit`, `Ctrl-d`) closes its tab; when the last one does,
  the panel hides.
- **Quitting Trail ends the panel's shells.** If one is running a command, Trail asks first.
  `[terminal] confirm_quit` and `confirm_close` choose when it asks: `when_busy` (the
  default), `always` or `never`.
- **Shells come from profiles.** With none configured you get PowerShell 7 if it is installed,
  else Windows PowerShell (`$SHELL` on Linux and macOS). Add `[[terminal.profile]]` entries
  for Git Bash, cmd, WSL or anything else — see the
  [configuration guide](configuration_guide.md#terminal).
- **The panel's height** is `[terminal] height`, 35% of the screen by default. On a screen too
  short for both, the panel takes all of it.
- **Copying text with the mouse** is done by your terminal, not by Trail, so a selection that
  runs beside the file list picks that up too. Pasting works as usual.
- `!command` is unchanged: it still hands the whole terminal to the command and comes back.
- **Known issue:** making the window *narrower* can leave the screen drawn as a staircase of
  broken rows. Any redraw — moving the selection, for instance — puts it right. Widening the
  window and resizing it vertically are not affected.

## 6. Configuration

You can customize Trail by supplying a configuration file. This allows overriding the default theme, keybindings, and general settings (like your preferred `$EDITOR`).

```bash
trail --config /path/to/trail.toml
```

Trail remembers that path, so a later plain `trail` reloads the same file — you only name it once. Use `trail --no-config` to run with the built-in defaults for a single run without forgetting it.

See the [configuration guide](configuration_guide.md) for the full schema.
