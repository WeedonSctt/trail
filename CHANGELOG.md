# Changelog

All notable user-facing changes to Trail are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
Trail follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as
`docs/release_process.md` §7 defines it — judged from the **user's** point of
view, where the keybindings, the config file, the CLI flags and the shell
wrapper contract are the public API.

## How to use this file

- Add to `## [Unreleased]` as the work lands, not at release time. A change that
  is only described in a commit message is a change no user will ever read.
- Group entries under `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed` or
  `Security`. Omit a heading rather than leaving it empty.
- Write the entry for someone upgrading, not for someone reviewing the diff:
  what they can now do, or what will behave differently, and the config key or
  binding that controls it.
- On release, rename `[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD` and open a fresh
  `[Unreleased]` above it. This is step 2 of the release sequence in
  `CLAUDE.md` §8, alongside the version bump.

This file starts at v1.8.0. Earlier releases are described in their GitHub
release notes and in `docs/upcoming_features.md` §6, which records what each
assessed defect was and what was done about it; they are deliberately not
back-filled here, because a changelog reconstructed after the fact says what
the commits said rather than what the users saw.

## [Unreleased]

## [1.11.3] - 2026-10-26

### Changed

- **A preview that stops early says so.** When `[preview] max_lines` or
  `[general] text_sync_threshold_kb` cuts a preview short, the bottom-left of the preview
  border reads `preview limit: first 2000 lines · max_lines` (or `first 256 KB ·
  text_sync_threshold_kb`), naming the key to raise. The footer's `+` stays.
- **An empty file reads `(empty file)`** instead of a blank pane that looked like a failed
  preview.

### Fixed

- **Leaving search keeps the cursor on what you found.** Pressing `Esc` after `/` used to
  drop the cursor on whatever entry sat at the same row of the full listing, so `dd`, `o`,
  `:` and the rest acted on the wrong file. It now stays on the match. Opening a file from
  search leaves it selected when the editor closes, and entering a directory from search
  then pressing `h` comes back to that directory rather than an unrelated one.
- **A dev build reports itself as one.** A build made after a `cargo publish` could report
  the bare release version (`1.11.2`) in `--version` and the `show_version` badge, because the
  publish's own test build replaced the version check. It now reads `1.11.2-dev.N+<commit>`.
- **Text files that are not UTF-8 are previewed.** A file saved as Windows "ANSI"
  (Windows-1252 / Latin-1) or as UTF-16 used to show an empty pane; it now shows its text,
  accents included. A stray invalid byte part-way down a file no longer ends the preview
  there without marking it truncated.
- **A single line longer than `text_sync_threshold_kb` is cut at that limit** and the
  preview marked truncated. It used to be loaded whole and reported as complete.

## [1.11.2] - 2026-10-21

### Fixed

- **`cargo install trail-fm` compiles.** Without `--locked`, v1.11.1 picked ratatui 0.30
  for its image preview beside Trail's 0.28, and the build failed. The image preview now
  uses `ratatui-image` 2, which asks for exactly the ratatui Trail uses, so the plain
  command works. `--locked` is still recommended.

## [1.11.1] - 2026-10-20

### Added

- **Install with `cargo install trail-fm`.** Trail is on crates.io under the name
  `trail-fm`, because `trail` there is an unrelated library. The command it installs is
  still `trail`. As with every source install, the shell wrappers for cd-on-exit are not
  included — take them from the release archive or `shell/` in the repository.

### Fixed

- **The selected entry is readable with the default theme.** The selected row was drawn bold,
  and WezTerm, Windows Terminal and xterm draw bold `black` as bright black — the same
  colour as the `dark_gray` highlight behind it — so the selected name disappeared. The
  selection is no longer bold, including over directory names. Themes set in hex colours
  were never affected.
- **The preview keeps up with a search.** Typing a search query showed the preview of
  whatever was first in the list when the search began, not of the match actually
  highlighted, until the selection was moved by hand. It now follows the top match as
  you type.
- The README and user guide said `Enter` in Search mode only selects the match. It opens
  it — entering a directory, or opening a file in your editor — and the docs now say so.

## [1.11.0] - 2026-10-19

### Added

- **Dev builds say so.** A binary not built from a tagged release now reports itself as one:
  `trail --version` and the `[general] show_version` badge read `1.10.1-dev.3+f7863b6`
  (three commits past v1.10.1, at commit `f7863b6`, with `.dirty` if built with uncommitted
  changes) instead of passing for `1.10.1`, and the badge is drawn in the `[theme] git_dirty`
  colour. Released binaries still read a plain `1.10.1`.
- **Marks that survive navigation.** `Space` marks the selection and moves down; go anywhere
  — another directory, another tab — and `pp` copies the marked entries there, `pm` moves
  them. `Esc` or `:unmark` clears the marks. Marked entries carry a `*`, and the status
  bar shows `3 marked` while any exist. A name already taken where you paste is skipped
  and reported, never overwritten.
- While anything is marked, `:mv`, `:cp` and `dd` act on the marked entries instead of the
  selection; `dd` asks once for the whole set (`Recycle 4 marked?`).
- **Visual mode**, `v`: extend a range with `j`/`k`/`G`/`gg` as in Vim, then `Space` or `v` to
  mark it, `h`/`l` to mark it and move on, `dd` to delete it, or `Esc` to abandon it.
  `Shift`-arrows still scroll the preview.
- **`@` repeats the last file operation**: the newest `:` or `!` command, or `dd`, `pp` or
  `pm`, whichever came last, on whatever is selected or marked now. A repeated `dd` still
  asks before deleting.
- **`:config write`** saves the session's `:set` changes into your `trail.toml`, touching
  only those keys and keeping every comment. With no config file loaded it creates one in
  Trail's config directory and loads it from then on.
- **`U` returns from a jump.** After `:jump`, however far you wander, `U` goes straight back
  to where you jumped from; `u` still steps back one directory at a time.
- **Bookmarks as destinations.** `:mv @docs` moves into the directory bookmarked as `docs`,
  and `:cp @docs\next\` into a route under it; `Tab` after `@` completes bookmark names and
  then the folders inside. An unknown name is an error. A real folder whose name starts
  with `@` is now written `.\@folder`.
- `p` is now a prefix (for `pp` and `pm`). If you had bound a longer key starting with `p`
  in `[keymap.navigation]`, check that it still does what you expect.

### Changed

- A half-typed key sequence is shown in the status bar — `g…` while `g` waits for its second
  `g`, `d…` for `dd` — so it no longer looks like a keystroke that was swallowed.
- `u` and `Ctrl-r` at either end of the history say so, instead of doing nothing visible.
- `:cp` and `:mv` run in the background. Copying a large folder no longer freezes Trail;
  the status bar shows `copying 3/10` while it works and says what happened when it is
  done.
- A `:cp` or `:mv` of several files (`:mv *.md docs\`) no longer stops at the first file
  whose name is already taken at the destination: that file is skipped, the rest are
  done, and the report counts both. Nothing is ever overwritten.

### Fixed

- A Navigation Mode binding to a symbol typed with `AltGr` (`@`, `|`, `~` on Spanish, Latin
  American and German layouts) could never be pressed: Windows reports `AltGr` as
  `Ctrl`+`Alt`, which Trail read as a `ctrl-` chord.
- `:mv` to another drive (`C:` to `D:`) failed, because a rename cannot cross drives. It
  now copies and then removes the original, and removes the original only once the copy
  has fully succeeded.
- `:cp` of a folder into itself never finished. It is now refused.

## [1.10.1] - 2026-10-18

### Fixed

- The preview could not scroll to the end of a file whose lines are wider than the pane: a
  wrapped line takes more than one row, so the last lines stayed below the bottom edge.
  Scrolling now counts those rows, and the `first–last/total` footer shows the lines
  actually on screen.

## [1.10.0] - 2026-10-12

### Added

- **A terminal panel**: real, interactive shells in a panel under the file list, so you can
  browse and use a shell at the same time. `Ctrl-.` shows and hides it, `F12` moves the
  keyboard between the file list and the shell, `Alt-Shift-PageDown`/`Alt-Shift-PageUp`
  switch shells and `Alt-PageUp`/`Alt-PageDown` scroll back. `:term new [profile]`, `:term close`,
  `:term <n>` and `:term max` do the rest. Each shell starts in the folder Trail is showing
  and is independent of it after that; `!command` is unchanged.
- `[terminal]` config: `default_profile`, `height` (35% by default), `confirm_quit` and
  `confirm_close` (`when_busy` by default — quitting asks only if a shell is running a
  command), and `[[terminal.profile]]` entries naming the shells to run. With none, Trail
  runs PowerShell on Windows and `$SHELL` elsewhere.
- `[keymap.terminal]`: `toggle`, `focus`, `next_shell`, `prev_shell`, `scroll_up`,
  `scroll_down`.

### Known issues

- Making the window narrower can leave the screen drawn as a "staircase" of broken rows
  until the next redraw (moving the selection is enough). Widening it, and resizing
  vertically, are fine.

## [1.9.2] - 2026-10-04

### Fixed

- `\` now reaches the command line and the search box on Windows keyboard
  layouts that type it with AltGr, such as Spanish (Latin America). Windows
  reports AltGr as Ctrl+Alt, and Trail threw away any character that came with
  Ctrl held. That dropped every backslash of a path pasted after `ya`, since a
  paste arrives as one keystroke per character. `@`, `|`, `~` and the other
  AltGr characters are fixed the same way.

## [1.9.1] - 2026-09-29

### Fixed

- `t` and `c` no longer swallow the key pressed after them in the file list.
  Trail mistook the `tab` and `ctrl-r` bindings for two-letter sequences
  starting with `t` and `c`, so each letter waited for a second key that could
  never complete one, and threw that key away. Both are now ordinary unbound
  keys that do nothing, and the key after them works as usual.

## [1.9.0] - 2026-09-29

### Added

- `[navigation] scroll_margin` keeps rows between the selection and the top or
  bottom of the navigation panel while you move, like vim's `scrolloff`.
  Default `3`; `0` lets the selection ride the edge. The margin gives way at
  the ends of the listing, so the first and last entries still reach the first
  and last rows. `:set scroll_margin 5` applies on the next frame.

### Fixed

- In a listing longer than the panel, the selection no longer sticks to the
  bottom row. Past the first screenful every entry was drawn on the last row,
  and moving up scrolled the list under the selection instead of moving it.
  The panel now remembers where it was scrolled to, so moving up moves the
  selection. Entering a directory centres the entry it lands on.

## [1.8.3] - 2026-09-29

### Changed

- The current directory has moved from the status bar to the **navigation
  panel's bottom border**, and is elided from the front (`…\util\trail`) rather
  than the back when it does not fit — the end of a path is the part that
  answers "where am I". It is still drawn once; it has only changed surface. If
  you had learned where to look, look lower and left.

### Fixed

- The path no longer disappears while you type a command. Command Mode takes
  the whole status bar, which is where the path used to be, so it vanished
  exactly when a relative `:mv` or `:cp` destination was being typed against it.
  A border is not the status bar, so nothing covers it now.
- The status bar's right section no longer cuts `  branch*  12 items ` on an
  80-column terminal. Its share of the row grew from 20% to 45%, using the room
  the path gave up.

## [1.8.2] - 2026-09-29

### Added

- `[general] show_version` puts Trail's version on the preview pane's top
  border. Off by default. `trail --version` cannot be asked of a session that is
  already running, and on Windows an upgrade cannot overwrite a running
  `trail.exe` — it renames the old one aside — so open windows can be on
  different builds and look identical. The badge stands down when a long file
  name needs the whole border.

## [1.8.1] - 2026-09-29

### Fixed

- The navigation panel now shows which sort order is in use, on the right-hand
  end of its top border (`size↓`, `time↑ mixed`). v1.8.0 shipped six ways to
  re-order a listing and no way to see which one was active — the notice that
  confirmed the change aged out on the next keystroke, so the only way to find
  out was to press a sort key and watch. The badge is per tab, like the sort
  itself, and gives up its room to the directory name on a panel too narrow for
  both.

[Unreleased]: https://github.com/WeedonSctt/trail/compare/v1.11.3...HEAD
[1.11.3]: https://github.com/WeedonSctt/trail/compare/v1.11.2...v1.11.3
[1.11.2]: https://github.com/WeedonSctt/trail/compare/v1.11.1...v1.11.2
[1.11.1]: https://github.com/WeedonSctt/trail/compare/v1.11.0...v1.11.1
[1.11.0]: https://github.com/WeedonSctt/trail/compare/v1.10.1...v1.11.0
[1.10.1]: https://github.com/WeedonSctt/trail/compare/v1.10.0...v1.10.1
[1.10.0]: https://github.com/WeedonSctt/trail/compare/v1.9.2...v1.10.0
[1.9.2]: https://github.com/WeedonSctt/trail/compare/v1.9.1...v1.9.2
[1.9.1]: https://github.com/WeedonSctt/trail/compare/v1.9.0...v1.9.1
[1.9.0]: https://github.com/WeedonSctt/trail/compare/v1.8.3...v1.9.0
[1.8.3]: https://github.com/WeedonSctt/trail/compare/v1.8.2...v1.8.3
[1.8.2]: https://github.com/WeedonSctt/trail/compare/v1.8.1...v1.8.2
[1.8.1]: https://github.com/WeedonSctt/trail/compare/v1.8.0...v1.8.1
