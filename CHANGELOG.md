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

- External previewers: a `[[preview.tool]]` rule runs a program for a file type
  and shows its output, with its colours, in the preview pane — `pdftotext` for
  PDFs, `glow` for Markdown, `chafa` for images, `ffprobe` for media. None is
  active until you write one; the default config carries commented examples.
  A rule's command is either a list, spawned directly with `{path}`, `{width}`
  and `{height}` substituted, or a string run through `[general] shell`, which
  reads the file from `$TRAIL_PREVIEW_PATH`.
- `P` switches the selected file's type between its tool and the built-in
  preview for the rest of the session.
- `[preview] external_timeout_ms` (default 3000) kills a previewer that hangs.
  Output is cut at `max_lines` and `text_sync_threshold_kb`, and moving to
  another entry stops a tool that is still running. A tool that is missing,
  times out, fails or prints nothing shows why, above the file's size and date.

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

[Unreleased]: https://github.com/WeedonSctt/trail/compare/v1.9.2...HEAD
[1.9.2]: https://github.com/WeedonSctt/trail/compare/v1.9.1...v1.9.2
[1.9.1]: https://github.com/WeedonSctt/trail/compare/v1.9.0...v1.9.1
[1.9.0]: https://github.com/WeedonSctt/trail/compare/v1.8.3...v1.9.0
[1.8.3]: https://github.com/WeedonSctt/trail/compare/v1.8.2...v1.8.3
[1.8.2]: https://github.com/WeedonSctt/trail/compare/v1.8.1...v1.8.2
[1.8.1]: https://github.com/WeedonSctt/trail/compare/v1.8.0...v1.8.1
