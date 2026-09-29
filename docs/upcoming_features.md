# Trail — Upcoming Features

Assessed improvements, in the order they are worth doing. Each entry says what the
defect or gap actually is (with the code that proves it), the shape of the fix, and
what it costs.

Source: field notes kept while using Trail, triaged against the code on 2026-09-28 and
again on 2026-09-29. Items already shipped are recorded in [§6](#6-already-resolved)
rather than deleted, so a note that resurfaces can be checked against what was done
about it.

**First round (2026-09-28, items 1–12).** Ten of the twelve were built in the pass that
followed; each has a commit of its own, and the entries below are kept as the reasoning
behind them.

**Second round (2026-09-29, items 13–16).** Two were built and shipped in v1.8.0 — the
listing's sort order and its details column. Two are assessed and not yet started: the
scroll margin, which is a defect with a provable cause, and the plugin API, which is a
scope decision rather than a fix. A third note from this round turned out to be
[§5.2](#52-a-second-window-for-commands) resurfacing unchanged, and is recorded there
rather than re-triaged.

Status values: **planned** (agreed, not started), **in progress**, **done** (shipped,
kept here for provenance), **deferred** (needs a decision or a terminal Trail cannot
test itself).

---

## 1. Rank

| # | Improvement | Fixes | Status |
|---|---|---|---|
| 1 | One notification channel, separate from errors | success reported as an error, sticky `yanked:`, errors missing from the log | **done** |
| 2 | Selection identity by path, not index | toggling hidden files moves the selection | **done** |
| 3 | Responsiveness: stop the per-frame clear and the per-preview rebuilds | the "flash" and the late reaction under load | **done** |
| 4 | Tab completion that actually cycles | only ever the first candidate | **done** |
| 5 | Command line with a cursor and room to read | blind mid-string editing, truncated messages | **done** |
| 6 | Active-tab indicator | no way to tell which tab is focused | **done** |
| 7 | Pause before returning from `!command` | command output wiped on exit | **done** — `[general] shell_pause` |
| 8 | One path on screen, not two | redundant path top and bottom | **done** |
| 9 | Recycle bin instead of permanent delete | `dd` is irreversible | **done** — `[general] delete_mode` |
| 10 | Globs for `:mv` and `:cp` | `:mv *.md` unsupported | **done** — patterns only; marking entries is still open, §5.3 |
| 11 | `:set` persistence | `:set` is session-only | deferred — §5 |
| 12 | Run a command without leaving the view | wanted a second window | deferred — §5 |
| 13 | Sort the listing by size, time or extension | no way to find the biggest or newest file | **done** — v1.8.0, `[navigation] sort_by` |
| 14 | Size and modification time in the listing | both visible only one file at a time, in the preview | **done** — v1.8.0, `[navigation] entry_details` |
| 15 | A scroll margin, so the selection leaves the last row | the selection is pinned to the bottom for the whole lower part of a listing | **planned** — §2.13 |
| 16 | A plugin API that can act, not just observe | a plugin can watch and log and do nothing else | **planned** — §2.14 |

---

## 2. The defects behind them

### 2.1 Success is reported as an error

`actions/mod.rs` sets `error_message` on the **success** branch of `:bookmark`, and the
status bar renders `error_message` as `Error: …` in the error colour. A bookmark that
saved correctly says `Error: bookmark added: name`. Success has no channel of its own,
so it borrows the failure one.

`last_yank` has the mirror problem: it is only ever assigned, never cleared, so one `ya`
keeps printing `yanked: …` in directories the user never yanked from.

And `state.error_message = Some(…)` never logs. Level counts across `src/` are 53
`debug!` against 6 `info!`, 1 `warn!` and 2 `error!`, while the subscriber's default
filter is INFO — so every failure the user sees is absent from `trail.log` unless
`RUST_LOG` is set.

**Fix.** One pair of helpers on `AppState` — `notify(msg)` and `set_error(msg)` — each
carrying a level and an expiry, each logging on the way through. The status bar renders
info in the normal colour and errors in the error colour, and a notice ages out instead
of pinning the section. Every current `error_message = Some(…)` site becomes one of the
two, which is what gets errors into the log in one pass.

### 2.2 Toggling hidden files moves the selection

`toggle_hidden` keeps the selection **index** and clamps it to the new count, but hidden
entries sort in among the visible ones, so index N addresses a different file afterwards.
`refresh()` has the same bug: a file created above the selection shifts it.

**Fix.** Resolve the selected entry's path before the listing changes and re-find it
after, falling back to a clamped index when it is gone. `selection_memory` is already
keyed by path, so the pattern exists in the codebase.

### 2.3 The flash, and the late reaction under load

Four independent causes, all in code rather than in the terminal:

1. `ui::render` calls `terminal.clear()` on **every** frame — a physical clear-screen and
   a full repaint of every cell, per keystroke. This is the flicker.
2. The highlight worker rebuilds `SyntaxSet::load_defaults_newlines()` and
   `ThemeSet::load_defaults()` **per preview**, i.e. once per row scrolled past.
3. `load_dir` opens every regular file and reads 8 KB of it to classify text vs binary,
   on the UI thread. 500 files is 500 opens per navigation.
4. Every `FsChanged` drops and recreates the `notify` watcher, re-lists the directory
   (cause 3 again) and respawns the git worker.

**Fix.** 2 and 3 and 4 are independent and safe: hoist the syntect sets into a
`OnceLock`, classify lazily off the UI thread, and re-subscribe the watcher only when the
watched directory actually changed.

Cause 1 is load-bearing and must go **last**: the per-frame clear is the mitigation for
the stray-character artifacts (§5.4), so the artifacts need their own fix first.
Preview text is currently rendered raw — the highlight worker trims only trailing
`\n`/`\r`, and `content_inspector` calls a file text as long as it has no NULs, so an ESC
or BEL byte in a "text" file goes straight to the terminal. Sanitizing control characters
where the preview is built removes the reason for the blanket clear; then ratatui's own
buffer diff can do its job and the flicker goes with it.

### 2.4 Tab completion never reaches the second candidate

`apply_completion` overwrites the buffer with the candidate, and the next `Tab` recomputes
candidates from that mutated buffer. `:m` + Tab becomes `mkdir ` — which contains a
space, so the second Tab takes the path branch, finds `mkdir` is not `mv`/`cp`, and
returns nothing. `mv` is unreachable. Paths behave the same way: after `mv foo.txt`, the
only candidate matching prefix `foo.txt` is itself. `TabState::advance` is correct; it is
simply never given the same list twice.

**Fix.** Keep the prefix that opened the cycle in `TabState` and compute candidates from
it until a non-Tab keystroke resets it.

### 2.5 The command line is one line, 30 % wide, with no cursor

`frame.set_cursor_position` is called nowhere. Command mode tracks a byte offset and
`Left`/`Right`/`Home`/`End`/`Delete` all move it, so editing mid-string is blind. The same
30 % centre section holds error messages, so a long parse error is cut mid-word with no
ellipsis, and a long `:mv` is invisible past the cut.

**Fix.** These are one job. Give Command mode the full bar width, scroll the buffer
horizontally around the cursor, and place a real cursor with `set_cursor_position`
(shown only in Command mode). Errors get the room in the same change, with an ellipsis
when they still do not fit.

### 2.6 No active-tab indicator

Nothing under `src/ui/` mentions tabs. `TabManager::is_single` is documented as the thing
"the UI uses to decide whether to render the tab bar"; nothing calls it. Shift-Tab
reaching `switch_tab_prev` is therefore invisible, and a no-op with one tab open.

**Fix.** A `[2/3]` badge in the status bar's left section — no row lost, no layout
change. A full tab bar can follow if tabs get heavier use; it costs a screen row.

### 2.7 Command output is wiped before it can be read

`run_external` re-enters raw mode and the alternate screen immediately after
`child.wait()`, so `!ls` paints its output and erases it in the same breath.

**Fix.** Pause between the wait and the restore, while the terminal is still in cooked
mode with the alternate screen inactive — writing a prompt there is legal, which it would
not be a moment later. The design point is that the same path opens the editor, where a
pause would be a nuisance: `Action::RunExternal` needs a `pause_after` flag, true for `!`
and `:git`, false for editor and OS-open. Behaviour belongs in
`[general] shell_pause = "always" | "on_error" | "never"`.

### 2.8 The path is drawn twice

The nav panel puts the full `cwd` in its border title and the status bar prints it again
bottom-left.

**Fix.** Title shows the directory's own name; the status bar keeps the full path. It
recovers title width rather than a screen row — the border is drawn either way.

### 2.9 Delete is irreversible

`fs_ops::delete` is `remove_file` / `remove_dir_all` after a single `y`, and on a
directory it is recursive.

**Fix.** The `trash` crate, not a hand-rolled folder: a home-made bin has to solve name
collisions, cross-device moves, growth and a purge policy, and the platform recycle bin
has already solved all four. `[general] delete_mode = "trash" | "permanent"`, defaulting
to `trash`. New dependency, so it needs a Decision Log entry.

### 2.10 `:mv` takes no glob

`:mv` treats everything after the verb as one destination string, and `fs_ops::mv` acts
on the single selected entry, so `:mv *.md docs/` resolves a destination literally named
`*.md docs/`. Related: `mv` uses `fs::rename`, which fails across drives on Windows.

**Fix.** Expand a pattern against `cwd` and apply the move to each match, requiring the
destination to be a directory when more than one file matches, and reporting the count
through the notification channel. The deeper feature — marking entries with `Space` and
letting `:mv`/`:cp`/`dd` act on the mark set — is the file-manager-idiomatic answer and
would make globs optional sugar; it is a larger change and is not in this pass.

### 2.11 One sort order, and it was the only one

`load_dir` sorted directories-first then name, case-insensitive, in a closure written
inline — the only entry sort in the codebase. There was no way to ask which file in a
directory was the biggest or which had changed most recently, which is most of what a
downloads folder or a build output directory is browsed *for*.

The cost turned out to be close to zero, which is what settled it. `Entry::from_dir_entry`
already calls `de.metadata().ok()` for every entry, unconditionally, because the kind
(file / dir / symlink) has to come from somewhere — so size and mtime were already in
hand. Sorting by them adds no syscall, which is why a re-sort can happen on the UI thread
without touching invariant 1.

**Fix, shipped in v1.8.0.** `src/app/sort.rs` owns `SortBy`, `SortSettings` and
`sort_entries`; `[navigation] sort_by`, `sort_reverse` and `dirs_first` configure it;
`:sort` and the `sn`/`ss`/`st`/`se`/`sr`/`sd` bindings change it at runtime. Held **per
tab**, on `TabState`, so a downloads tab in `st` does not re-sort the source tree in the
tab beside it.

Three traps were worth the time they took to find:

- `FilterState::matches` holds **indices** into `entries`. A re-sort that did not re-run
  the filter would leave every index naming a different file — silently, and only while a
  search was active. `set_sort` re-applies the filter rather than re-indexing it.
- The selection is an index too, so it is resolved to a path and looked up again after the
  re-sort. This is §2.2's bug in a new place; the fix is §2.2's `restore_selection`.
- `[navigation] sort_by` feeds the listing as it is *built*, not at render time, so the
  `set_value` caller in `actions/mod.rs` has to re-sort. Without it `:set sort_by size`
  appears to do nothing until the next navigation — exactly the failure `CLAUDE.md` §5
  warns about.

### 2.12 Size and modification time, one file at a time

Both were already displayed — in the preview pane, for the selected entry only. Comparing
four files meant selecting each in turn and remembering the last three.

The blocker was never the data, it was the width. The nav panel is 40% of the screen: 28
usable columns on an 80-column terminal, after the border and the `"> "` highlight symbol.
A size is 9 of those and a full timestamp 16, so `both` wants 26 of 28 and would leave two
columns of file name.

**Fix, shipped in v1.8.0.** `[navigation] entry_details = none | size | modified | both`,
defaulting to `none` so the release changes nothing for anyone who does not ask. The
column is budgeted rather than assumed: `EntryDetails::fit` degrades `both` to a bare date
before it degrades to nothing, and stands down entirely rather than leave a name fewer
than 12 columns. A directory shows `—` rather than `metadata.len()`, which describes its
directory record and not its contents. Formatting moved to `src/metafmt.rs`, shared with
the binary and image previews so the same file cannot be described two ways.

Known limitation, stated in the code: padding counts `char`s, not display width, so a name
containing double-width characters leaves that one row's column a little ragged. Fixing it
means taking `unicode-width` as a direct dependency; the same trade-off is already
documented in `status_bar::draw_command_line`, and it is not worth a dependency yet.

### 2.13 The selection is pinned to the last row

> "When navigating in the same directory the selection is ALWAYS at the bottom. When it's
> already at the bottom, the selector keeps being at the bottom, even if I scroll up."

Provable, and the cause is two lines. `nav_panel::draw` builds a **fresh**
`ListState::default()` on every frame and sets only `select(...)` — the offset is never
carried over, so it is `0` at the start of every render.

ratatui's `List` computes its window from that offset: it fills forward from `offset` for
as many rows as fit, and then, while `selected >= last_visible`, scrolls down one row at a
time until the selection is on screen. Starting from `0`, the cheapest window that
contains selection *N* always ends at *N* — so for every selection index at or past the
viewport height, the selection renders on the **bottom row**. Moving up from index 40 to
39 re-runs the same arithmetic and lands on the bottom row again, which is precisely the
report: the list scrolls under a selection that never moves.

It stops only once the index falls below the viewport height, which is why the top of a
listing behaves normally and the rest does not.

**Fix.** Two parts, and they have to go together:

1. **Persist the offset.** It belongs on `TabState` next to `selected`, for the reason the
   preview's scroll lives on `PreviewSlot`: only the renderer knows the pane height, so
   the renderer records it and the movement actions clamp against it. A `ListState` rebuilt
   per frame can hold no scroll position by construction.
2. **A scroll margin.** Keep 2–3 rows between the selection and either edge, collapsing to
   zero within that distance of the true top or bottom of the list — so the selection
   reaches row 0 for the first entry and the last row for the last entry, and floats in
   between. This is vim's `scrolloff`; the note asks for exactly it.

Worth a `[navigation] scroll_margin` key, defaulting to 3, with 0 restoring today's
behaviour for anyone who prefers it. Both parts are on the UI thread and cost nothing;
the render tests can assert the selected row's position at a given viewport height, which
makes this one of the more testable items in this file.

### 2.14 A plugin can observe and nothing else

> "for minor release, expand the trail API for plugins"

The current surface is four functions — `trail.on_select(fn)`, `trail.on_enter_dir(fn)`,
`trail.register_action(name, fn)`, `trail.log(msg)` — and the two hooks receive a path
string and return nothing. `fire_action` discards its callback's return value entirely.

So a plugin can be told where the user went and write a line to the log. It cannot read
the selection's size or kind, navigate anywhere, report a result to the status bar, read a
config key, register a preview provider, or tell Trail that an action failed. The engine's
own comment says the API is "intentionally minimal for v1 — resist expanding it until"
there is a reason; bookmarks, the one example plugin, works only because it was also built
as a core module.

**Fix.** The smallest set that makes a plugin able to do a job, in the order they are worth
adding:

- **Read the world.** `trail.selection()` and `trail.cwd()` returning a table
  (`path`, `name`, `kind`, `size`, `modified`, `is_hidden`) rather than a bare string.
  Pure reads of state the UI thread already holds.
- **Report.** `trail.notify(msg)` and `trail.error(msg)` onto the notice channel built in
  §2.1, so a plugin action can say what it did in the place every other outcome is
  reported.
- **Act.** `trail.navigate(path)`, `trail.refresh()`, and letting a `register_action`
  handler return `false`/`nil` plus a message so a failure reaches the user instead of the
  debug log.
- **Read config.** `trail.config(key)` against the same key vocabulary `:set` uses, so a
  plugin can respect the user's theme and editor instead of hard-coding.

Each is additive, so this is MINOR. Two things need deciding before it is built, which is
why it is *planned* rather than started: whether a plugin may **mutate** the filesystem
(a `trail.move`/`trail.delete` pair is a much larger blast radius than everything above,
and the recycle-bin routing of §2.9 would have to apply to it), and whether hooks may run
anywhere but the UI thread — today they are called synchronously from it, so a slow hook
blocks rendering, which invariant 1 forbids for everything else. A plugin that does real
work will hit that ceiling, and moving hooks to the worker pool changes what the API can
promise about ordering.

---

## 3. Sequencing

This was the order used, and it held up: 1 and 2 first, both small, and 1 a prerequisite
for reporting anything else sensibly. Then 4, 5, 6 and 8 — independent UI work. Then the
control-character fix, which is what made it safe to drop the per-frame clear, and only
then the rest of 3. Finally 7, 9 and 10, each of which adds a config key or a dependency.

Every config key added here touches all six places in `CLAUDE.md` §4, and every new
binding or command touches `keymap.rs`, `docs/user_guide.md` and the `README.md` key
table.

---

## 4. Version impact

New config keys with behaviour-preserving defaults are MINOR under `docs/release_process.md`
§7. Two of these defaults do **not** preserve behaviour — `delete_mode = "trash"` and a
non-`never` `shell_pause` — so the release that carries them is a MINOR at least, and the
notes have to say that `dd` now goes to the recycle bin.

The second round is MINOR throughout. v1.8.0's four `[navigation]` keys all default to
what v1.7.2 did (`sort_by = "name"`, not reversed, directories first, no details column),
and its seven new bindings are additions rather than changes — `s` was unbound, and so was
`m`. The one thing worth a line in the release notes is that `s` is now a **prefix**: a
user who had bound `s` to something of their own in `[keymap.navigation]` keeps it, since
a configured single-key binding is resolved before the prefix check, but a binding that
*starts* with `s` and is longer now shares the prefix and should be checked.

Of the two planned items, the scroll margin is MINOR (`scroll_margin` defaults to a value
that changes the current behaviour, so it leads the notes, with `0` documented as the way
back). The plugin API is MINOR as long as it only adds functions; letting hooks run off
the UI thread would change what the API promises about ordering and is the part that could
turn it MAJOR, which is why §2.14 keeps it as a separate decision.

---

## 5. Deferred, and why

### 5.1 `:set` persistence

`set_value` only mutates the in-memory config; nothing writes `trail.toml`, and
`configuration_guide.md` already documents that. Auto-writing on every `:set` would
serialize the config back and erase the comments in it, which here are the documentation
of each trade-off. If persistence is wanted, it should be an explicit `:set!` or
`:config write` built on `toml_edit` so comments survive. Needs a decision on which.

### 5.2 A second window for commands

> **Resurfaced 2026-09-29**, in the same words: *"could it be possible to start a new
> window where to input commands instead of exiting trail? just as vscode"*. Re-checked
> against the code and nothing below has changed — the three options are still the three
> options, and the decision the second one needs has still not been made. The `vscode`
> comparison does sharpen it: what VS Code has is a **dockable panel**, which is the
> second option, not a detached window. If this is wanted, the thing to settle is where
> that panel lives — a third pane splitting the preview, or a mode that takes the whole
> screen below the nav panel — and everything after that is ordinary work.


Trail does not exit to run `!` — it suspends, swapping out of the alternate screen and
back. Three ways to get the effect asked for:

- Spawn a detached terminal window (`wt.exe`, `start`, `open -a Terminal`): platform
  specific, loses stdin, and Trail can never learn that the command finished.
- Capture stdout/stderr in a worker and show it in a pane: works within the current
  architecture and keeps the UI thread free, but only for non-interactive commands, and
  it needs a home in the UI that the preview pane cannot provide (the next selection
  change would overwrite it).
- Embed a PTY pane (`portable-pty` plus a VT parser): this is writing a terminal emulator
  inside Trail.

The second is the realistic one, and it needs a decision about a new pane or mode before
it can be built. §2.7's pause covers the immediate need for interactive commands.

### 5.3 Marking entries for bulk operations

`:mv *.md notes` now covers the common case, but the general feature is a mark set:
`Space` to mark, marks shown in the listing, and `:mv`/`:cp`/`dd` acting on them. That
makes globs optional sugar and covers the cases a pattern cannot express — "these four,
not those two". It needs decisions about the marking key, how marks survive navigation and
whether a marked delete confirms once or per entry, so it is a feature rather than a fix.

### 5.4 Stray characters on `.png` and `.bbmodel`

Reported as characters appearing outside the preview, cleared by any redraw, reproducing
on `.png` and `.bbmodel` but not `.jpg`. One cause was provable and is fixed: preview text
was rendered without stripping control characters, and `preview::provider::sanitize` now
replaces them. That covers `.bbmodel`, which is text. The `.png` half is not covered,
because it depends on which terminal was in use — inline-image protocol detection
floors at Halfblocks in Windows Terminal, which is safe, but resolves to iTerm2 in the
VS Code terminal, whose base64 payload prints as garbage where it is unsupported. Settling
it needs the terminal the artifacts were seen in; the image matrix is listed as
untestable in `CLAUDE.md` §7 for exactly this reason.

---

## 6. Already resolved

Kept so a resurfacing note can be checked rather than re-triaged.

| Note | Resolution |
|---|---|
| `j`/`k` moved the selection instead of typing into a search | Search mode types every bare character; arrows and `Ctrl-n`/`Ctrl-p` move (`14b4658`) |
| `Shift-Tab` did nothing | Windows reports it as `BackTab`, which matched no binding; both spellings now resolve to `shift-tab` (`2875c74`). Invisible until §2.6 lands |
| `yr` and `yn` copied the same string | `yr` is relative to the launch directory, not the browsed one (`e399247`) |
| No way to yank a file's content | `yc`, natively — no plugin needed (`e399247`) |
| Windows paths carried `\\?\` | `src/pathfmt.rs` decides the spelling once; `cwd` holds the plain form |
| Syntax highlighting held the selection back; large HTML loaded while large Markdown froze | Every text preview is deferred to the worker with a `Loading` placeholder and a generation guard |
| Does `:set` rewrite the config file? | No. It is session-only, and documented as such |
