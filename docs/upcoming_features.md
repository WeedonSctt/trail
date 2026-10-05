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

**Second round (2026-09-29, items 13–17).** Two were built and shipped in v1.8.0 — the
listing's sort order and its details column. Two are assessed and not yet started: the
scroll margin, which is a defect with a provable cause (since shipped in v1.9.0), and the plugin API, which is a
scope decision rather than a fix. Item 17 was found while building the sort keys rather
than reported. A further note from this round turned out to be
[§5.2](#52-a-second-window-for-commands) resurfacing unchanged, and is recorded there
rather than re-triaged.

**Third round (2026-09-29, items 18–19).** Both are about what the frame tells you rather
than what it does: which build you are looking at, and where you are while you type a
command. Both shipped, in v1.8.2 and v1.8.3. They share a question — which surface carries standing
information — and §2.17 is where the answer has to be decided, because it revisits
[§2.8](#28-the-path-is-drawn-twice).

**Fourth round (2026-10-05, item 21).** A feature request rather than a defect: external
previewer tools per file type, switchable with a key. The design decisions were put to the
maintainer and answered before this entry was written; §2.19 records the answers alongside
the plan, and the release gate it carries is stricter than the others' — a branch, a manual
test by the maintainer, and performance budgets, before anything reaches `main`.

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
| 15 | A scroll margin, so the selection leaves the last row | the selection is pinned to the bottom for the whole lower part of a listing | **done** — v1.9.0, `[navigation] scroll_margin` |
| 16 | A plugin API that can act, not just observe | a plugin can watch and log and do nothing else | **planned** — §2.14 |
| 17 | `t` and `c` silently swallow the next keystroke | a named-key binding makes its first letter a prefix | **done** — v1.9.1; the pending-key indicator is still open, §2.15 |
| 18 | An optional version indicator in the view | no way to tell which build a running session is | **done** — v1.8.2, `[general] show_version` |
| 19 | Keep the path on screen during Command Mode | the command line covers the only copy of it | **done** — v1.8.3, the nav panel's bottom border |
| 20 | `\` missing from a pasted path | AltGr characters dropped from the command line and search | **done** — v1.9.2, §2.18 |
| 21 | External previewer tools per file type, toggled with `P` | PDFs show only metadata; Markdown only as source; no way to use `pdftotext`, `glow`, `chafa`, `ffprobe` | **planned** — §2.19 |

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

**Shipped in v1.9.0**, with two departures from the plan above. The offset lives on
`AppState` rather than `TabState`, keyed by the directory it was computed in
(`src/app/scroll.rs`): comparing directories at render time catches navigation, tab
switches and tab closes without each of them having to reset it, and a directory without
a recorded offset centres its selection, which puts a remembered entry in context rather
than on an edge. And `0` does not restore the old behaviour — the old behaviour was the
bug. It means no margin: the selection may reach either edge, and stays where it is when
you reverse direction.

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

**Fix.** Designed in full in [`plugin_api_plan.md`](plugin_api_plan.md), which settles the
shape: a plugin cannot be handed `&mut AppState` — the engine lives *in* it, so reaching
it borrows the whole struct — and so plugins request `Action`s onto a queue that is
drained after the hook returns, the same deferral `pending_external` already uses for
`RunExternal`. That makes the plugin write surface exactly the `Action` enum: a plugin can
do what a keybinding can do and nothing else.

The smallest set that makes a plugin able to do a job, in the order they are worth adding:

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

### 2.15 `t` and `c` swallow the next keystroke

Found while building §2.11's `s` prefix, and confirmed against the running
binary rather than by reading: pressing `t` or `c` in Navigation Mode returns
`SetPendingNavKey`, so Trail waits for a second key that can never complete a
sequence, and the keystroke after it is discarded.

`configured_nav_prefix` decides a character is a prefix like this:

```rust
binding.len() > ch.len_utf8() && binding.starts_with(ch)
```

which is right for `gg`, `ya` and `dd` and wrong for every **named-key**
binding. The shipped defaults include `tab`, `ctrl-r`, `ctrl-t`, `ctrl-w`,
`shift-tab`, `shift-home` and `shift-end` — so `t` is "the start of `tab`", `c`
is "the start of `ctrl-r`", and both become prefixes. A probe over the default
keymap returns:

```text
't' -> Some(SetPendingNavKey('t'))     # from "tab"
'c' -> Some(SetPendingNavKey('c'))     # from "ctrl-r" / "ctrl-t" / "ctrl-w"
's' -> Some(SetPendingNavKey('s'))     # from "shift-tab" before v1.8.0; now the sort prefix
'g' -> Some(SetPendingNavKey('g'))     # correct: "gg"
```

This predates v1.8.0 — `s` behaved the same way because of `shift-tab`, which is
why giving `s` a real meaning cost no keymap plumbing and broke nothing. What it
means today is that two ordinary letters are dead keys that also eat the letter
after them, with nothing on screen to say so.

**Fix.** A binding is a multi-key sequence only when it is a run of single
characters — i.e. it contains no `-` and is not one of the named keys
`key_to_config_string` can produce. Reject the named ones before the prefix test
rather than after. Cheap, and `keymap.rs` already has the list of names to
check against.

Worth pairing with the other half of the same gap: there is no visual sign that
a prefix is pending. Vim shows the partial sequence in the corner; Trail shows
nothing, so a swallowed keystroke and a genuine `g`-waiting-for-`g` look
identical. `state.pending_nav_key` is already on `AppState`, so the status bar
could render it in the space the tab indicator uses.

**Fixed in v1.9.1**, as described: `configured_nav_prefix` skips any binding
`is_named_key` recognises — a bare key name from `NAMED_KEYS`, or one prefixed with
`ctrl-` or `shift-` — before the prefix test. A unit test walks every key
`key_to_config_string` can name and asserts `is_named_key` accepts it, so a named key
added later cannot quietly reopen the bug. The pending-prefix indicator was not part of
the patch; it adds something to the frame rather than fixing it, and is still open.

### 2.16 No way to tell which build you are looking at

> "add an optional indicator to show the trail version inside the tool view. (it'd be
> like a dev option, available at the config toml)"

`trail --version` answers the question from outside, but a running session cannot be
asked: nothing under `src/ui/` mentions the version, so two Trail windows side by side are
indistinguishable however far apart their builds are.

That is not a hypothetical. During the v1.8.1 session the maintainer had four Trail
processes running **three different binaries** at once — 1.7.2, 1.8.0 and 1.8.1 — because
Windows will not let a running `trail.exe` be overwritten, so each upgrade renames the old
image aside and the open sessions keep it until they exit (see `CLAUDE.md` §8). Every one
of those windows looked identical. A window that did not have the sort badge was on an old
build, which is a diagnosis by absence, and only works while a feature is new.

**Cost: almost none.** `src/cli.rs` already declares `#[command(version)]`, which clap
fills from `CARGO_PKG_VERSION`, so the string is a compile-time constant already in the
binary. There is nothing to plumb, no state to hold and nothing to recompute per frame.

**Fix.** `[general] show_version = false`, a dev-facing diagnostic rather than chrome, so
the default leaves the frame exactly as it is.

Where it goes matters more than whether it exists, because the surfaces are full:

- The **status bar's right section** is 20% of the row and already overflows at 80
  columns — `  branch*  12 items ` is 20 characters in 16 columns. Adding to it pushes
  the entry count off.
- The **navigation panel's top border** now carries the directory name on the left and
  the sort badge on the right (v1.8.1).
- The **preview panel's top border** carries the selected file's name on the left and
  **nothing on the right**. Its bottom-right holds the scroll footer; its top-right is
  the one unoccupied surface in the frame.

So: `title_top` on the preview panel, right-aligned, in the dim status style — the same
call the scroll footer uses one border away. Costs no screen row and nothing at render
time.

Worth deciding at the same time: whether the indicator shows the version alone (`v1.8.1`)
or is a general diagnostics slot that could later carry a frame counter or the resolved
image protocol, which is the other thing that is invisible from inside and hard to
diagnose from outside (§5.4). If the second, name the key for the slot rather than for the
version.

### 2.17 The command line covers the only copy of the path

> "maybe the absolute path of trail may be shown on top, since the bottom part is undrew
> when the command mode is used"

Exactly right, and the code says why. `status_bar::draw` opens with:

```rust
if let Mode::Command { buffer, cursor, .. } = &state.mode {
    draw_command_line(frame, area, buffer, *cursor, styles.command);
    return;
}
```

Command Mode returns before the three sections are laid out, so the left section — which
is the only place the full `cwd` is drawn — is not rendered at all while a command is
being typed. The navigation panel's border still shows the directory's *name*, so the leaf
survives; the full path does not.

This is [§2.8](#28-the-path-is-drawn-twice) coming back around, and worth stating plainly
rather than treating as a new bug. §2.8 removed the path from the nav panel title because
it was drawn twice, leaving the status bar as the single copy. §2.5 then gave Command Mode
the whole status bar, because a `:mv` destination is longer than 30% of a terminal. Each
decision was right on its own; together they put the only copy of the path on the one
surface that Command Mode covers. The path is least visible exactly when a relative
destination is being typed against it.

**The fix is a choice about which surface carries standing information**, and it should be
made once rather than per-feature:

- **The navigation panel's bottom border.** Free today — `╰────╯` and nothing else — and
  Command Mode does not touch it, so the path survives. `title_bottom` with a right- or
  left-aligned `Line`, exactly as the preview panel's scroll footer already does. Costs no
  screen row. The trade-off is width: the nav panel is 40% of the screen against the
  status bar's 50%, so a deep path elides sooner. This is the recommendation.
- **A dedicated row at the top**, as the note suggests. Full width, always legible, and
  the only option that never elides. It costs a screen row permanently, which this project
  has refused twice — §2.6 chose a `[2/3]` badge over a tab bar, §2.8 chose to recover
  title width rather than a row. Refusing it a third time should be a decision, not a
  reflex, but the reasoning has not changed.
- **Re-split the status bar during Command Mode.** Cheapest to write and wrong: it undoes
  §2.5 and puts a long `:mv` back into a third of a row.

Whichever is chosen, the path should move rather than be duplicated — §2.8's point stands.
If it moves to the nav panel's bottom border, the status bar's left section is left with
the mode badge and the tab indicator, which frees room the right section currently does
not have (§2.16).

### 2.18 A pasted path loses every `\`

> "Every time i want to copy an absolute path with 'ya' in windows and i have to paste it
> in the command line, every single time i found the path is pasted without the '\'."

The clipboard was never the problem; `ya` copies the backslashes. The keystrokes drop them.
Without bracketed paste, the Windows console replays a paste as one key event per
character, and for each one it sets the modifiers the active layout would need to type
it. On Spanish (Latin America), `\` is AltGr plus the key left of `1`, and Windows reports
AltGr as Ctrl+Alt. Command Mode inserted a character only when Ctrl was **not** held, and
Search Mode only when neither Ctrl nor Alt was, so both threw away `\`, along with `@`,
`|`, `~` and anything else that layout puts behind AltGr. Typing `\` by hand failed the
same way.

**Fixed in v1.9.2.** `input::is_text_modifiers` treats Ctrl+Alt as text and a bare Ctrl or
Alt as a chord, and both typing modes use it. Command Mode still accepts Alt+char as text,
as it always did. Navigation Mode is unchanged: it binds keys, not text.

### 2.19 External previewer tools per file type

> "add an optional previewer tool for various type of files (pdf, png, jpg, md, etc.).
> this can be switchable from default in config (for each type of file) and live with a
> key that toggles the preview of the selected item."

**The gap.** The preview pane knows four kinds of thing: directories, images (by
extension, `preview/image.rs`), text (`workers/highlight.rs`) and binary metadata
(`preview/binary.rs`). A PDF is binary, so it gets size and timestamp and nothing about
its content; Markdown is text, so it gets highlighted source rather than a rendering; a
video gets the same metadata block as a PDF. Tools that do each of these well already
exist — `pdftotext`, `glow`, `chafa`, `ffprobe`, `7z l` — and the usual way a terminal
file manager reaches them (lf, yazi, ranger) is to run them and show their output in the
preview pane. On the maintainer's machine `pdftotext`, `bat` and `ffprobe` are installed;
`glow` and `chafa` are not.

This is not `o` (`open_with_os`), which hands the file to a GUI application and stays as
it is.

#### Decisions

Put to the maintainer on 2026-10-05; these are settled, not open.

| Question | Answer |
|---|---|
| What the toggle key switches | **The whole file type, for the session.** `P` on one `.md` switches every `.md` until `P` again or quit |
| How a command is written | **Both forms.** A list runs directly; a string runs through `[general] shell` |
| What Trail ships with | **Commented examples only.** No rule is active until the user writes one |
| What a failed tool shows | **The error and the file's metadata**, since the preview did not load |
| The toggle key | **`P`** (Shift-p), leaving `p` free |
| Whether toggles survive a restart | **Session only**, the same as `:set` (§5.1) |
| How it ships | **Branch → maintainer tests → performance budgets pass → `main` → release** |

#### Configuration

One rule per tool, as an array of tables, so one image tool can cover several
extensions:

```toml
[[preview.tool]]
extensions = ["pdf"]
command    = ["pdftotext", "-l", "10", "-layout", "{path}", "-"]
default    = "external"          # external | builtin; omitted means external

[[preview.tool]]
extensions = ["png", "jpg", "jpeg", "webp"]
command    = ["chafa", "-f", "symbols", "--size", "{width}x{height}", "{path}"]
default    = "builtin"           # keep the inline image; P switches to chafa

[[preview.tool]]
extensions = ["md"]
command    = "glow -s dark -w $TRAIL_PREVIEW_WIDTH \"$TRAIL_PREVIEW_PATH\""

[preview]
external_timeout_ms = 3000       # 100..=60000
```

- **List form** is spawned directly, no shell. Placeholders `{path}`, `{width}`,
  `{height}` are substituted per argument, so a file name can never become a second
  argument or a second command. Any other `{…}` is a config error. A `.cmd`/`.bat` tool
  (npm shims) cannot be spawned without an interpreter and is written as
  `["cmd", "/C", "tool", …]`; the configuration guide says so.
- **String form** runs through `shell_exec::shell_argv`, i.e. the `[general] shell` the
  user already configured, so pipes work. Placeholders are **a config error** in the
  string form: substituting a file name into shell text is injection (a file called
  `x & del /s *.pdf`), and quoting rules differ between cmd, PowerShell and sh. The string
  reads the path from the environment instead — `TRAIL_PREVIEW_PATH`,
  `TRAIL_PREVIEW_WIDTH`, `TRAIL_PREVIEW_HEIGHT` — written `"$TRAIL_PREVIEW_PATH"` in sh
  and PowerShell, `"%TRAIL_PREVIEW_PATH%"` in cmd. Both forms get the variables.
- **Validation** (`TrailConfig::validate`): `deny_unknown_fields` on the rule; empty
  command rejected; an extension in two rules rejected; extensions lowercased and a
  leading `.` stripped; `default` is `external` or `builtin`.
- **Shipped default:** `tool = []`, with the four examples above commented out in
  `default.toml`. Trail never probes `PATH` — not at startup, not at config load — so
  startup is unaffected whether or not rules exist.
- **`:set`.** `preview.external_timeout_ms` gets a `set_value` arm like any key. The rule
  list cannot be expressed as `key value`, so `:set preview.tool…` returns an error naming
  `P` and the config file. This is a deliberate exception to the six-places rule in
  `CLAUDE.md` §5 and is stated in `configuration_guide.md`.
- **Overrides** (`config/mod.rs`): a user's `tool` list replaces the default list whole;
  rules are not merged by extension.

#### Architecture

**Provider contract.** `PreviewProvider::can_handle(&self, entry)` sees no state, so a
provider cannot know the config or the session's toggles. It becomes
`can_handle(&self, entry, ctx)` — a one-line change to each of the four existing
providers and their tests. `PreviewCtx` gains `external: Option<ExternalSpec>`: the
command for this entry, already resolved. Resolution — the rule for the extension, then
the session override, else the rule's `default` — is a pure function in the library
(`preview::external::resolve`), not in `main.rs`, so it is unit-testable;
`refresh_preview` calls it. The alternative, branching around the registry in
`refresh_preview`, would make this the one provider that is not registered, which is the
trap `CLAUDE.md` §5 names.

**`preview/external.rs`** — `ExternalProvider`, registered second (after
`DirectoryProvider`, before `ImageProvider`, so it can take images when asked to).
`can_handle` is `ctx.external.is_some()` for a file; `preview` always spawns and returns
`Deferred`. Nothing runs on the UI thread.

**`workers/external_preview.rs`** — `tokio::process::Command` (tokio's `full` feature
already includes `process`; no new dependency).

- *Isolated from the terminal:* stdin null, stdout and stderr piped — a child that
  inherited them would draw over the alternate screen (invariant 7) and read keystrokes.
  On Windows, `CREATE_NO_WINDOW` through the safe `creation_flags`, so no console flashes.
  Working directory is the file's parent.
- *Bounded:* stdout is read until `[preview] max_lines` lines or `text_sync_threshold_kb`
  bytes, then the child is killed — a `cat`-like tool on a 2 GB file costs the cap, not the
  file. `external_timeout_ms` kills a hung tool.
- *Cancelled when stale:* the generation guard already drops a late result, but not the
  process behind it; holding `j` through a folder of PDFs would leave one `pdftotext` per
  file running. `PreviewSlot` keeps the running task's `AbortHandle`, `refresh_preview`
  aborts it before starting the next, and the child is spawned `kill_on_drop(true)`, so
  aborting the task kills the process. A live-child counter in the worker exists for the
  performance test below.
- Results come back as `WorkerMsg::Preview` with the generation, through the existing
  `merge` guard — no new message type.

**`preview/ansi.rs`** — SGR escape sequences to styled spans: 16, 256 and 24-bit colour,
foreground and background, bold, dim, italic, underline, reverse, and their resets. Every
other sequence (cursor movement, clears, OSC titles, sixel/kitty image payloads) is
discarded, and what is left goes through `provider::sanitize`. Written in the crate
(~200 lines) rather than taking `ansi-to-tui`, whose releases each pin a `ratatui` major
— the two-ratatui trap in `CLAUDE.md` §9. Recorded in the Decision Log.

**`StyledSpan`** gains `bg: Option<Color>` and `modifiers: Modifier`. `chafa` draws almost
entirely with background colours; `glow` uses bold and italic. The highlight worker passes
`None` and empty, so its output is unchanged.

**New content variants.**
- `PreviewContent::External { lines, tool }` — drawn **without the line-number gutter**
  (numbering character art or rendered Markdown wrecks it), scrolls with `J`/`K` like
  text, clips long lines at the pane edge. `tool` (the program's file stem) is shown on
  the pane's top border — `─ pdftotext ─` — so it is always visible which preview is up.
- `PreviewContent::ExternalFailed { error, metadata }` — the failure answer: the error
  lines in the theme's `error` colour (`pdftotext: not found on PATH`, `timed out after
  3000 ms`, `exited with status 1` plus the first lines of stderr, or `produced no
  output`), a blank line, then the same metadata block `binary::build_binary_preview`
  gives today. `P` returns to the built-in preview.

**Pane size.** `{width}` needs the pane's width; the renderer records only
`viewport_height`. `PreviewSlot` gains `viewport_width`, written the same way, with
80×24 assumed before the first frame.

#### The toggle

`toggle_preview_tool = "P"` in `[keymap.navigation]` → `Action::TogglePreviewTool`. It
flips `AppState::preview_mode_overrides` (extension → mode) for the selected file's
extension, then re-previews in place keeping the scroll position, as `R` does. With no
rule for the extension it posts `no previewer configured for .xyz` on the notice channel
and changes nothing. Overrides are process-wide (not per tab) and are not saved.
`P` is a bare character, so the `NAMED_KEYS` trap does not apply; the existing keymap test
still runs over it.

#### Order of work

On a branch, `feat/external-previewers`, pushed after every commit so the work exists
somewhere other than this machine:

1. `[~] feat>` give preview spans a background colour and text modifiers — no visible
   change; render tests prove it.
2. `[+] feat>` ANSI SGR parser for external previewer output — unit tests only.
3. `[+] feat>` `[[preview.tool]]` rules and `external_timeout_ms` — the six places,
   commented examples, configuration guide.
4. `[+] feat>` external previewer provider and worker — the `can_handle` change, the
   worker with cancellation, both content variants, pane width, registration.
5. `[+] feat>` toggle a file type's previewer with `P` — keymap, border label,
   `user_guide.md`, `README.md` key table, `CHANGELOG.md`, the architecture doc's module
   tables, the Decision Log.

#### Tests

- *Parser:* each colour form, resets, discarded non-SGR sequences, CR and tab, a sequence
  cut off at end of input.
- *Config:* both command forms; rejection of an unknown field, empty command, duplicate
  extension, unknown placeholder, any placeholder in the string form; extension
  normalisation; `:set preview.tool…` refused; `:set preview.external_timeout_ms` bounded.
- *Resolution:* override beats `default`, `default` applies without one, `P` on an
  extension with no rule notifies and changes nothing.
- *Worker, against real processes* (`cmd /C type` on Windows, `cat` elsewhere): normal
  output, missing program, timeout, output past the line cap, non-zero exit with stderr,
  abort mid-run.
- *Merge:* a stale external result is dropped.
- *Render (`TestBackend`):* no gutter on `External`, the border label, error lines then
  metadata on `ExternalFailed`.

#### Performance budgets — the release gate

In `tests/perf_external_preview.rs`, every test `#[ignore]`d and run with
`cargo test --release --test perf_external_preview -- --ignored`. Timing assertions are
flaky on shared CI runners, so they gate the release locally rather than CI.

| # | Measures | Budget |
|---|---|---|
| B1 | UI-thread cost of previewing an entry with an external rule (resolve + `preview_for`), median of 1000 | < 1 ms — it only spawns |
| B2 | Config load with 20 rules vs none | < 1 ms difference; zero processes spawned |
| B3 | 200 selection changes 10 ms apart, tool sleeps 5 s | ≤ 1 live child 500 ms after the last change |
| B4 | A tool printing 10 MB | read stops at the byte cap, child killed, result within 200 ms of the cap |
| B5 | ANSI parse of 2000 lines (~200 KB) of `bat --color=always` output | < 20 ms |
| B6 | One frame of a 2000-line `External` preview in `TestBackend` | < 2 ms — only the visible slice is drawn |

Then the maintainer's own pass, against a release build run beside the installed one
(`target\release\trail.exe --config <test config>`, which leaves `%LOCALAPPDATA%\trail`
untouched): hold `j` through a folder of 100+ PDFs with the `pdftotext` rule — the listing
keeps pace and Task Manager shows no more than one or two `pdftotext.exe`; `P` on a `.md`
and back; no console window flashes; a rule naming a missing program shows the error and
the metadata.

#### Shipping

After the maintainer's pass and B1–B6: rebase onto `main` if it has moved,
`merge --ff-only`, push, delete the branch locally and remotely (SHA printed first), then
the release sequence in `CLAUDE.md` §8 as **v1.10.0**, including the post-release doc
update and the `trail.exe` replacement on this machine. Until then the branch stays open
on purpose, and is reported as such at the end of every session that touches it.

#### Cannot be verified here

`glow` and `chafa` output (not installed); how character-art images look in a given
terminal — related to the open image-terminal question in §5.4; whether a console window
still flashes on some Windows configuration only the maintainer's terminal shows.

---

## 3. Sequencing

This was the order used, and it held up: 1 and 2 first, both small, and 1 a prerequisite
for reporting anything else sensibly. Then 4, 5, 6 and 8 — independent UI work. Then the
control-character fix, which is what made it safe to drop the per-frame clear, and only
then the rest of 3. Finally 7, 9 and 10, each of which adds a config key or a dependency.

Every config key added here touches all six places in `CLAUDE.md` §5, and every new
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

The third round is MINOR too, and one of the two is a PATCH under a strict reading.
§2.16's `show_version` is a new key defaulting to `false`, so it adds a capability and
changes nothing — MINOR. §2.17 adds no key, binding, command or flag and restores
something the frame used to show; by the §8 test of whether a user's configuration changes
meaning, that is a PATCH, the same reading that made the v1.8.1 sort badge one. If the two
ship together the release is MINOR, and the notes should say that the path has moved
surface rather than merely reappeared — anyone who had learned where to look will find it
somewhere else.

The fourth round's §2.19 is MINOR: `[[preview.tool]]` defaults to an empty list and
`external_timeout_ms` only matters once a rule exists, so a v1.9.2 config behaves exactly
as before, and `P` was unbound. The `can_handle` signature change is internal — the
plugin API does not expose providers yet — so it does not count. Planned as v1.10.0.

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
untestable in `CLAUDE.md` §9 for exactly this reason.

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
