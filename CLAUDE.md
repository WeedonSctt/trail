# CLAUDE.md

Guidance for AI agents working in this repository. Read this before touching code.

This file is **tracked in git on purpose** — it is not in `.gitignore` and must not be
added to it. It is shared project context, the same as `docs/`.

---

## 1. What Trail is

Trail is a terminal-first file manager written in Rust: a three-panel TUI (navigation
list, type-aware preview, status bar) plus a command line for filesystem work. It
replaces `cd`/`ls` in a hot path, so **startup latency and input responsiveness are
product requirements, not nice-to-haves**.

The authoritative documents, in order of precedence for code-quality questions:

| Document | Decides |
|---|---|
| `docs/coding_standard.md` | *How* code is written — wins over every other doc on quality |
| `docs/trail.md` | *What* gets built (product spec) |
| `docs/trail_architecture.md` | *How it is structured* (tech stack, threading model, module tables) |
| `docs/trail_implementation_plan.md` | Phase breakdown and the Decision Log |
| `docs/configuration_guide.md` | Every user-facing config key |
| `docs/upcoming_features.md` | Assessed-but-unbuilt improvements, and why the deferred ones wait |
| `docs/release_process.md` / `docs/release_checklist.md` | Cutting a release |

If code and a doc disagree, that is a bug in one of them — say so rather than silently
picking a side.

---

## 2. Commands

```sh
cargo fmt --check                                                    # formatting gate
cargo clippy --workspace --all-targets --all-features -- -D warnings # lint gate
cargo build --workspace --all-features                               # build gate
cargo test --workspace --all-features                                # test gate
cargo doc --workspace --no-deps --all-features                       # doc gate
```

These five, in this order, are exactly what `.github/workflows/ci.yml` runs. Locally,
`cargo test --all-targets` compiles everything and is the fastest single signal.

`cargo run` takes over the terminal (alternate screen) — it is not usable for automated
verification. Rendering is verified through `tests/render_snapshot_tests.rs`
(`ratatui::backend::TestBackend`), never by launching the binary. Those tests assert on the
buffer's *contents* — a substring, a cursor position, a count of occurrences — not against
golden files: `insta` is a dev-dependency kept for the day a whole-frame snapshot is worth
the churn, and there are no `.snap` files. Assert the thing that would regress, not the
frame.

---

## 3. Repository map

```
src/
  main.rs          startup, terminal setup, the event loop; the only place anyhow is allowed
  lib.rs           library surface (so integration tests can reach internals)
  cli.rs           clap flags (--config, --cwd-file, --paths, ...)
  pathfmt.rs       how a path is spelled for a human vs. for the OS (Windows `\\?\`)
  paths.rs         every location Trail owns; the only ProjectDirs caller
  session.rs       cd-on-exit handoff to the shell wrappers
  app/             state.rs (AppState), mode.rs, history.rs, tabs.rs
  ui/              mod.rs (render entry), nav_panel, preview_panel, status_bar, theme
  input/           keymap.rs, command_parser.rs
  preview/         provider.rs (PreviewProvider trait + PreviewContent), text, directory,
                   binary, image, graphics (terminal inline-image protocol state)
  workers/         mod.rs (WorkerMsg + merge), git, fswatch, highlight, image_decode
  actions/         mod.rs (dispatch), fs_ops, shell_exec, clipboard
  config/          schema.rs (serde structs), mod.rs (overrides + merge), default.toml,
                   last_used.rs
  plugin/          mod.rs, lua_api.rs, bookmarks.rs
tests/             state_, preview_, command_parser_, render_snapshot_, test_gg_sequence
pkg/               homebrew/ aur/ scoop/ packaging manifests
shell/             trail.bash|zsh|fish|ps1 — cd-on-exit wrappers, shipped in every archive
```

Soft ceiling of ~500 lines per file. Past that it usually owns more than one
responsibility from the architecture doc's tables and should be split.

---

## 4. Invariants — treat a regression here as a release blocker

1. **The UI thread never blocks.** No git calls, image decodes, large file reads, or
   network on it. Anything variable-latency goes to the tokio worker pool and reports
   back over `mpsc`.
2. **The generation guard holds.** Every `WorkerMsg::Preview` / `WorkerMsg::ImageMeta`
   carries the `generation` it answers, and `workers::merge` checks it against
   `state.preview.generation` before applying. This is what stops a stale preview
   appearing when you navigate quickly.
3. **Workers never touch `ratatui`/`crossterm`.** All render-relevant mutation happens on
   the UI thread after a channel receive.
4. **`#![forbid(unsafe_code)]` stays.** This is why terminal capabilities are detected
   from environment variables rather than by querying the tty.
5. **No `unwrap()`/`expect()`/`panic!` outside `#[cfg(test)]` and `tests/`.** The single
   documented exception is unrecoverable startup failure in `main.rs`, after the panic
   hook is installed.
6. **The panic hook that restores cooked terminal mode is installed first** and stays
   intact — without it a panic leaves the user's terminal corrupted.
7. **Never write to stdout/stderr while the alternate screen is active.** Use the logging
   subscriber, never `println!`.
8. **Config parsing is strict.** `#[serde(deny_unknown_fields)]` everywhere; an unknown
   key is a hard error surfaced to the user, never silently dropped.
9. **Every release archive contains `shell/`.** A binary-only archive is a regression —
   the binary alone cannot cd the parent shell.

---

## 5. Workflow rules — what to do after what

### After editing any Rust source
1. `cargo fmt` — always, before anything else reads the file.
2. `cargo clippy --all-targets` — must be clean; fix rather than `#[allow]`. If a lint
   genuinely must be suppressed, scope it to one expression or function and add
   `// clippy: <lint> — <reason>`.
3. `cargo test` — the full suite, not just the file you touched.
4. Only then report the work as done, and state anything you could not verify.

Do not re-run a gate that already passed unless you changed code since.

### After adding or changing a `pub` item
1. Write the `///` doc comment in the same edit — state the contract (panics, errors,
   invariants), not the signature.
2. If it is a new module, open it with a `//!` summary that matches its row in
   `docs/trail_architecture.md`'s tables; if there is no such row, add one.
3. If the doc comment contains an example, it must be a passing doctest.

### After adding a config key
Touch all six places or the key is half-wired:
1. `src/config/schema.rs` — the field on the section struct.
2. `src/config/schema.rs` — `TrailConfig::validate` for it.
3. `src/config/schema.rs` — the `set_value` arm, so `:set` reaches it at runtime.
4. `src/config/mod.rs` — the matching `*Overrides` struct and its `apply_to`.
5. `src/config/default.toml` — the default plus a comment explaining the trade-off.
6. `docs/configuration_guide.md` — the user-facing description.

If the key feeds process-wide state rather than being read at render time (as
`[preview] image_*` does), the `set_value` caller in `src/actions/mod.rs` must rebuild
that state too, or `:set` silently does nothing until restart.

### After adding a preview provider
Implement `PreviewProvider`, then register it in the `PreviewRegistry` built in
`main.rs` — an unregistered provider compiles fine and never runs. Slow work belongs in
a worker returning `PreviewOutcome::Deferred`, not in `preview()`.

### After adding a keybinding or command
Update `src/input/keymap.rs` or `command_parser.rs`, then `docs/user_guide.md`, then the
key table in `README.md`. A binding documented in only one of the three is a bug report
waiting to happen.

### After adding a dependency
1. Justify it in the commit body: why it is needed, why nothing in the stack covers it.
2. Pin the version in `Cargo.toml` and commit the updated `Cargo.lock`.
3. If it replaces or deviates from a crate named in the architecture doc, update the
   Decision Log in `docs/trail_implementation_plan.md` — do not silently swap it.

### After resolving a `// TODO(phase-N):` marker
Delete the marker. A stub outliving its assigned phase is a bug, not a shortcut, and the
release checklist gates on no markers remaining for phases ≤ current.

### Before committing
1. Run the full Definition of Done gate (§15 of `docs/coding_standard.md`): fmt, clippy,
   build, test, docs on new `pub` items, no new `unwrap`/`unsafe`, tests for new logic.
2. Check `git status` for **untracked** files that belong to the change — a new module is
   invisible to CI until it is `git add`ed.
3. Write the commit message in the repo's format (§7).
4. Commit it. See §6 — git is yours to run, not the maintainer's.

### After changing anything a user would notice
Add the entry to `CHANGELOG.md` under `## [Unreleased]`, in the same change. Written at
release time it becomes a summary of the commits; written now it is a note to whoever
upgrades.

### Before cutting a release
Follow §8 and `docs/release_checklist.md` in order. Do not tag until all five gates pass
locally — a failing gate blocks the release workflow anyway, and a deleted tag is worse
than a delayed one.

### When something cannot be verified automatically
Say so explicitly rather than leaving it implied. Known-unverifiable paths: the
Kitty/iTerm2/Sixel image matrix (needs those terminals; only Halfblocks is testable),
real terminal rendering outside snapshot tests, OS file dialogs, and terminal cell-size
measurement on Windows.

---

## 6. Git is yours to manage

**The maintainer does not use git.** They have said so plainly: they do not know the
commands and will not run them. Nothing gets committed, merged, pushed, tagged or cleaned
up unless the agent does it. Treat every git operation this file describes as part of the
job, not as something to hand back.

Two consequences, and they are the whole of this section:

- **Never end a turn leaving work only in the working tree.** An uncommitted change is
  work that no one else can save. If it passes the gates, commit it and push it. If it
  does not, say so and either finish it or revert it — do not leave a half-edited tree
  behind.
- **Never leave a branch for someone else to merge.** There is no reviewer and no one to
  click "Merge pull request". A branch that is not merged back is work that has been
  thrown away with extra steps.

### The branch model

`main` is the trunk. It is always the newest thing, it is always what was released last,
and every commit lands on it.

- **Ordinary work commits straight to `main`** and pushes. There is no review gate to
  protect, and a feature branch nobody merges is how this repository ended up with `main`
  stranded at v1.2.0 while six releases shipped from a branch called `feat/preview-scroll`.
- **Branch only when the work might not land** — a spike, something risky, or a change the
  maintainer may want to see before it becomes real. Then, before the turn ends, either
  fast-forward `main` onto it and delete the branch, or say plainly that the branch exists
  and why it is still open.
- **`git merge --ff-only` is the merge you want.** If it refuses, `main` has moved and the
  branch needs rebasing onto it; do that rather than reaching for a merge commit. The
  history here is linear and worth keeping that way.
- **Delete a branch once it is merged.** `git branch -d` refuses unmerged work, which is
  the safety net — if it refuses, find out why before forcing it.

### Things that are never the agent's call

- **Never force-push, and never rewrite history that has been pushed.** Not to tidy a
  message, not to squash, not to drop a commit. Fix forward with another commit.
- **Never delete or move a published tag.** `docs/release_process.md` says this and means
  it: someone may have pinned it. A bad release is fixed by the next patch release.
- **Never delete a remote branch or a tag without saying so first.** Local branch deletion
  is recoverable from the reflog; a remote one is not, and it is the maintainer's
  repository. Print the SHA before deleting anything, so it can be recovered from the
  message alone.
- **Never commit a secret, a token, or anything from `~/.claude`.** Check `git status`
  rather than `git add -A` when the tree has files you did not create.

### Keeping it clean

At the end of a session the repository should look like someone tidy left it: `main`
current and pushed, no stray local branches, no uncommitted changes, `git status` clean.
If it does not, say what is outstanding and why, in plain words — not in git terminology
the maintainer will have to look up.

---

## 7. Commit messages

The format actually used in this repository:

```
[<sigil>] <kind>> <lowercase summary, imperative, no trailing period>
```

- **Sigil** — `[+]` adds something new, `[~]` changes or fixes something existing.
- **Kind** — `feat`, `fix`, `doc`, `phase`, `release`.
- **Summary** — lowercase, under ~72 characters, says what changed and, for fixes, what
  was wrong. Backticks around identifiers are fine.

```
[+] feat> yank entry content with `yc`, and fix `yr` duplicating `yn`
[~] fix> install.ps1 silently skipped SHA-256 verification
[~] release> fill v1.0.1 sha256 digests into package manifests
```

Note: `docs/coding_standard.md` §14 and `docs/release_process.md` still describe the
older `[Phase N] ...` prefix from the phased build-out. The format above is what the
history uses and what new commits should follow.

---

## 8. Versioning and release naming

### Version numbers
Semantic versioning, `MAJOR.MINOR.PATCH`, judged from the **user's** point of view — the
keybindings, the config file, the CLI flags, and the shell wrapper contract are the
public API of a TUI, not the Rust types.

- **MAJOR** — a config key is removed or changes meaning, a default keybinding changes to
  something incompatible, or a shell wrapper needs re-sourcing to keep working.
- **MINOR** — new features, new config keys with defaults that preserve current behaviour,
  new preview providers, new commands.
- **PATCH** — bug fixes, doc changes, packaging and digest fixes, dependency bumps with no
  behaviour change.

Every version number appears in exactly these forms:

| Where | Form | Example |
|---|---|---|
| `Cargo.toml` `[package] version` | bare | `1.0.1` |
| `pkg/scoop/trail.json` `version` | bare | `1.0.1` |
| `pkg/aur/PKGBUILD` `pkgver` | bare | `1.0.1` |
| `pkg/homebrew/trail.rb` `version` | bare | `1.0.1` |
| Git tag (annotated) | `v` prefix | `v1.0.1`, message `Release v1.0.1` |
| GitHub Release title | prose | `Trail v1.0.1` |
| Release archives | `trail-vX.Y.Z-<target>.<ext>` | `trail-v1.0.1-x86_64-pc-windows-msvc.zip` |
| Archive extract dir | `trail-vX.Y.Z-<target>` | matches the archive stem |

The `v` prefix belongs to tags, archives, and human-facing titles. Manifest fields take
the bare number. Getting this wrong breaks the installers, which build download URLs from
these strings.

No codenames, no pre-release suffixes unless one is deliberately introduced — if it is,
use `X.Y.Z-rc.N` and keep it out of the package manifests.

### Release sequence

**Release from `main`, and tag `main`.** Earlier releases were cut from feature branches
and pushed to a `release/vX.Y.Z` branch; that is why `main` sat at v1.2.0 while v1.7.2
shipped. The tag is what `release.yml` fires on and what pins the commit, so the release
branch bought nothing and cost the trunk. Do not recreate it.

1. All five gates pass locally, on `main`, with everything committed and pushed.
2. Close the changelog: rename `## [Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD`, open a
   fresh empty `[Unreleased]` above it, and update the link definitions at the foot.
3. Bump `Cargo.toml` and the four package manifests, plus `pkg/aur/.SRCINFO`, which has to
   agree with `PKGBUILD` or the AUR package breaks. Leave SHA-256 digests as placeholders.
   Commit as `[~] release> bump version to vX.Y.Z` and push.
4. `git tag -a vX.Y.Z -m "Release vX.Y.Z"` and push the tag — `release.yml` fires on it,
   runs the gates, builds all five targets, generates `checksums.txt`, and opens a
   **draft** release.
5. Review the draft: ten assets (five archives, `checksums.txt`, and the four install /
   uninstall scripts), every archive containing the binary, `shell/`, the README and the
   uninstallers. Verify a digest against `checksums.txt` rather than trusting the job.
6. Set the title to `Trail vX.Y.Z` and write real release notes — `generate_release_notes`
   produces a commit list, which is not what an upgrading user needs. Publish.
7. Fill the real digests from `checksums.txt` into the four manifests and commit as
   `[~] release> fill vX.Y.Z sha256 digests into package manifests`.
8. Push the manifests to their tap / AUR / bucket repositories.

Installing the new release locally is not part of the release, but if asked: on Windows
`install.ps1` **fails when Trail is running**, because it force-copies over a locked
`trail.exe`. Rename the running binary aside (`trail.exe.inuse-<stamp>.tmp` — Windows
permits renaming a running image, only not overwriting it) and copy the new one into
place. Open sessions keep the old inode until they exit; nothing has to be killed.

**Never delete or move a published tag** — it breaks anyone who pinned it. Fix forward
with a patch release, and annotate the bad release's notes with a pointer to the fix.

---

## 9. Current state and known traps

- Released: **v1.8.2**, from `main`, which is current and pushed.
- `t` and `c` are accidental prefix keys: `configured_nav_prefix` treats any binding
  longer than one character as a multi-key sequence, so `"tab"` makes `t` one and
  `"ctrl-r"` makes `c` one, and both swallow the following keystroke. Assessed in
  `docs/upcoming_features.md` §2.15, not yet fixed.
- Three `// TODO(phase-N):` markers outlive their phases (`phase-4` in
  `command_parser.rs`, `phase-5` in `tests/fixtures/.keep`, two `phase-something` in
  `history.rs`). The release checklist gates on these; they have shipped unfixed since
  before v1.7.2.
- `ratatui-image` must resolve against the **same** `ratatui` the crate uses (0.28). Its
  dependency range also admits ratatui 0.30, and a `cargo update` will happily re-split
  the graph into two ratatui versions — at which point `StatefulImage` no longer matches
  the `Frame` it draws into and the preview path stops compiling. The unification is held
  only by `Cargo.lock`. Verify with `cargo tree -i ratatui` after any dependency change:
  it must show a single version.
- Inline-image protocol detection is environment-variable based and floors at Halfblocks,
  so an image always renders *something*. Terminal cell size cannot be measured on
  Windows at all; it falls back to a deliberately small default so images leave a margin
  rather than overflowing (an overflowing image is dropped entirely by the terminal).
- `.gitignore` contains `test-*`, which will swallow a new top-level file or directory
  starting with `test-`. Name test fixtures accordingly, or place them in `tests/`.
- Line endings: the working tree is CRLF on Windows, the repo is LF. `git diff` warnings
  about this are expected and not something to fix.
