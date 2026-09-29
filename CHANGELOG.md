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

Nothing yet.

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

[Unreleased]: https://github.com/WeedonSctt/trail/compare/v1.8.3...HEAD
[1.8.3]: https://github.com/WeedonSctt/trail/compare/v1.8.2...v1.8.3
[1.8.2]: https://github.com/WeedonSctt/trail/compare/v1.8.1...v1.8.2
[1.8.1]: https://github.com/WeedonSctt/trail/compare/v1.8.0...v1.8.1
