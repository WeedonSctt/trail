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

### Added

- **A plugin API that can do things.** Plugins could only watch and log; they can
  now read the selection, the listing and the config (`trail.selection()`,
  `trail.entries()`, `trail.cwd()`, `trail.config()`), navigate, select, sort and
  run Trail commands (`trail.navigate`, `trail.select`, `trail.command`, …), put
  messages and a standing segment in the status bar (`trail.notify`,
  `trail.set_status`), and run commands in the terminal (`trail.run`). The full
  reference is `docs/plugin_guide.md`.
- `trail.bind(keys, action)` gives a plugin action a key. A plugin can claim keys
  Trail does not use but cannot take one over; a binding that can never fire is
  reported at startup.
- `trail.spawn{ cmd, on_exit }` runs a command in the background and calls back
  with its output, so a plugin can do slow work without freezing Trail.
- `trail.register_previewer{ extensions, command }` previews matching files with
  a command's output — `jq` for JSON, for example. It runs off the UI thread with
  a timeout, like Trail's own previews.
- `trail.on_fs_change` fires when files change in the current directory.
- `[plugins] budget_ms` (default `50`): how long one call into a plugin may run
  before Trail stops it and says which plugin it was.
- Five example plugins in `examples/plugins/`.

### Changed

- Plugin errors — a plugin that fails to load, a hook that raises, an action that
  fails — now appear in the status bar, named after the plugin. They used to go
  only to a debug-level log line, so a broken plugin looked like one that did
  nothing.
- `on_enter_dir` now fires for every change of directory — `h`, `u`, `Ctrl-r`,
  `:jump`, tab switches — and for the directory Trail starts in. It used to fire
  only for `l`/`Enter` into a directory.
- Hooks receive an entry table as a second argument; the first is still the path.
  Existing plugins keep working.
- The built-in `bookmarks` plugin's `bookmark_add` and `bookmark_jump` actions now
  add and jump rather than only logging, and `b` bookmarks the current directory
  when the plugin is enabled.

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

[Unreleased]: https://github.com/WeedonSctt/trail/compare/v1.9.1...HEAD
[1.9.1]: https://github.com/WeedonSctt/trail/compare/v1.9.0...v1.9.1
[1.9.0]: https://github.com/WeedonSctt/trail/compare/v1.8.3...v1.9.0
[1.8.3]: https://github.com/WeedonSctt/trail/compare/v1.8.2...v1.8.3
[1.8.2]: https://github.com/WeedonSctt/trail/compare/v1.8.1...v1.8.2
[1.8.1]: https://github.com/WeedonSctt/trail/compare/v1.8.0...v1.8.1
