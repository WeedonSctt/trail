<div align="center">

# Trail

**A terminal-first workspace for navigating, inspecting and acting on the filesystem — without leaving your shell.**

[![CI](https://github.com/WeedonSctt/trail/actions/workflows/ci.yml/badge.svg)](https://github.com/WeedonSctt/trail/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/WeedonSctt/trail?label=release)](https://github.com/WeedonSctt/trail/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 1.80+](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/platforms-linux%20%7C%20macos%20%7C%20windows-lightgrey.svg)](#platform-support)

</div>

<!-- SCREENSHOT SLOT
     Drop a screenshot or demo GIF at docs/assets/demo.gif, then replace this
     comment with:
     <p align="center"><img src="docs/assets/demo.gif" alt="Trail in action" width="800"></p>
-->

Trail is a keyboard-driven file manager for the terminal. It gives you a directory listing,
a live preview of whatever is selected, and a command line for filesystem work — then leaves
your shell in the directory you finished in.

It is not a fuzzy finder and not a file picker. Trail is built around one principle:

> **Navigation is the primary task. Every other feature exists to support it without interrupting the flow.**

```
┌─ ~/proj/trail ───────────────┬─ Cargo.toml ────────────────────────────────┐
│ docs/                        │   1  [package]                              │
│ src/                       M │   2  name = "trail"                         │
│ tests/                       │   3  version = "1.0.1"                      │
│ Cargo.toml                 M │   4  edition = "2021"                       │
│ LICENSE                      │   5                                         │
│ README.md                  ? │   6  [dependencies]                         │
│                              │   7  ratatui = "0.28"                       │
└──────────────────────────────┴─────────────────────────────────────────────┘
 NAV  ~/proj/trail                                       6 entries    main
```

**Contents** — [Features](#features) · [Installation](#installation) · [Shell integration](#shell-integration) ·
[Usage](#usage) · [Configuration](#configuration) · [Plugins](#plugins) · [Documentation](#documentation) ·
[Development](#development)

---

## Features

- **Three-panel interface** — directory listing on the left, type-aware preview on the right,
  and a status bar reflecting path, mode, active filter, entry count and git branch.
- **Type-aware previews** — syntax-highlighted text (via `syntect`), directory summaries with
  file and directory counts, image metadata and dimensions, and binary metadata. Images render
  inline when the terminal speaks Kitty, iTerm2 or Sixel.
- **Git aware** — current branch in the status bar and per-entry status badges
  (`M` modified, `A` added, `D` deleted, `?` untracked, `R` renamed), computed off the UI thread.
- **Never blocks** — git status, filesystem watching, syntax highlighting and image decoding all
  run on a `tokio` worker pool. The UI thread only renders, so navigation never stutters.
- **Live refresh** — the current directory is watched with `notify` and refreshed automatically,
  debounced so that a `git checkout` triggers one repaint instead of a hundred.
- **Fuzzy search** — incremental filtering of the current directory with `nucleo`, results
  reordered by match score as you type.
- **Command mode** — `mkdir`, `touch`, `rename`, `mv`, `cp`, `git`, `set`, `bookmark`, `jump`,
  plus arbitrary shell commands with `!`. Includes history and Tab completion for command verbs
  and destination paths.
- **Tabs** — several directories open at once, each with its own selection and history.
- **cd-on-exit** — quit with `q` and your shell follows you to the directory you were browsing;
  cancel with `Ctrl-c` and it stays put.
- **Strictly validated config** — TOML for theme, keybindings and editor, type-checked at load,
  with the same validation applied to runtime `:set` changes.
- **Lua plugins** — an embedded Lua 5.4 runtime (`mlua`) with `on_select`, `on_enter_dir` and
  custom `:plugin` actions. A misbehaving plugin is caught and skipped, never taking down the TUI.

### Platform support

Prebuilt binaries are published for every release:

| Platform | Targets |
|---|---|
| Linux | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` |
| macOS | `x86_64-apple-darwin`, `aarch64-apple-darwin` (Apple Silicon) |
| Windows | `x86_64-pc-windows-msvc` |

---

## Installation

Full guide: [docs/installation.md](docs/installation.md). Quick paths:

| Method | Platform | Command |
|---|---|---|
| [Homebrew](#homebrew) | macOS | `brew tap WeedonSctt/trail && brew install trail` |
| [AUR](#aur) | Arch Linux | `paru -S trail` |
| [Scoop](#scoop) | Windows | `scoop bucket add trail …` |
| [Install script](#install-script) | Linux / macOS | `curl -fsSL … \| sh` |
| [Install script](#install-script) | Windows | `irm … \| iex` |
| [From source](#from-source) | any | `cargo install --git …` |

Every method except the from-source one ships the shell wrappers needed for
[cd-on-exit](#shell-integration).

### Homebrew

```sh
brew tap WeedonSctt/trail
brew install trail
```

### AUR

```sh
paru -S trail
# or: yay -S trail
```

### Scoop

```pwsh
scoop bucket add trail https://github.com/WeedonSctt/trail-scoop-bucket
scoop install trail
```

### Install script

Linux and macOS:

```sh
curl -fsSL https://github.com/WeedonSctt/trail/releases/latest/download/install.sh | sh
```

Windows:

```pwsh
irm https://github.com/WeedonSctt/trail/releases/latest/download/install.ps1 | iex
```

Both scripts verify the downloaded archive against its published SHA-256 digest before installing.

### From source

Requires Rust 1.80 or newer:

```sh
cargo install --git https://github.com/WeedonSctt/trail
```

> **Note:** this installs only the binary. The shell wrappers that provide cd-on-exit are not
> included — take them from the [Releases page](https://github.com/WeedonSctt/trail/releases/latest)
> or from [`shell/`](shell/) in this repository.

---

## Shell integration

Trail's **cd-on-exit** behaviour — your shell moving to the last directory Trail was browsing —
requires a shell wrapper function. A child process cannot change its parent shell's working
directory, so the wrapper is what actually calls `cd`; the binary alone cannot do it.

The wrapper ships in every release archive and in every package manager formula. Source it once
per shell:

**Bash** — add to `~/.bashrc`:

```bash
source /path/to/trail/shell/trail.bash
```

**Zsh** — add to `~/.zshrc`:

```zsh
source /path/to/trail/shell/trail.zsh
```

**Fish** — copy into fish's functions directory, which autoloads it:

```fish
cp /path/to/trail/shell/trail.fish ~/.config/fish/functions/trail.fish
```

**PowerShell** — add to your `$PROFILE`:

```pwsh
. "/path/to/trail/shell/trail.ps1"
```

Reload your shell afterwards (`source ~/.bashrc`, `. $PROFILE`, and so on).

### How it works

The wrapper creates a temporary file and passes it to the binary via `--cwd-file`.
On a **normal exit** (`q`), Trail writes its current directory to that file and the wrapper
`cd`s into it. On **cancellation** (`Ctrl-c`, or `Esc`-quit), Trail writes nothing, so your
shell stays exactly where it started.

---

## Usage

```
trail [<start-path>]
```

| Flag | Description |
|---|---|
| `<start-path>` | Directory to open. Defaults to the current directory. |
| `--config <path>` | Load a TOML config file. The path is remembered for later runs. |
| `--no-config` | Ignore the remembered config for this run only. |
| `--cwd-file <path>` | Write the final directory here on normal exit (used by the shell wrappers). |
| `--version`, `--help` | Version and usage information. |

### Navigation mode

The default mode. Vim-style bindings, with arrow keys and standard keys as fallbacks.

| Key | Action |
|---|---|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `l` / `Enter` / `→` | Enter directory, or open file in your configured editor |
| `h` / `Backspace` / `←` | Go to parent directory |
| `gg` | Jump to top |
| `G` | Jump to bottom |
| `u` | Back in navigation history |
| `Ctrl-r` | Forward in navigation history |
| `R` | Refresh the current directory |
| `.` | Toggle hidden files |

**File operations**

| Key | Action |
|---|---|
| `o` | Open selection with the OS default application |
| `ya` | Copy absolute path to clipboard |
| `yr` | Copy path relative to the launch directory to clipboard |
| `yn` | Copy filename to clipboard |
| `yc` | Copy content to clipboard — file text, or directory listing |
| `dd` | Delete selection — confirm with `y`/`Enter`, cancel with `n`/`Esc` |

**Tabs**

| Key | Action |
|---|---|
| `Ctrl-t` | New tab |
| `Ctrl-w` | Close current tab |
| `Tab` | Next tab |
| `Shift-Tab` | Previous tab |

**Modes and exit**

| Key | Action |
|---|---|
| `/` | Enter Search mode |
| `:` | Enter Command mode |
| `q` | Quit — your shell cd's here if the wrapper is sourced |
| `Ctrl-c` | Quit without changing directory |

### Search mode

Press `/` and type to fuzzy-filter the current directory. Results reorder by match score as you type.

| Key | Action |
|---|---|
| *any character* | Typed into the query - every letter is text here, including `j` and `k` |
| `↓` / `Ctrl-n`, `↑` / `Ctrl-p` | Move through results |
| `Enter` / `→` | Confirm and return to Navigation mode with the match selected |
| `Esc` | Cancel and restore the full listing |
| `Backspace` / `Ctrl-h` | Delete the last character |

### Command mode

Press `:` for operations that take arguments.

| Command | Description |
|---|---|
| `:mkdir <name>` | Create a directory in the current directory |
| `:touch <name>` | Create an empty file |
| `:rename <new-name>` / `:ren` | Rename the selected entry |
| `:mv <dest>` | Move the selection (relative or absolute destination) |
| `:cp <dest>` | Copy the selection |
| `:git <subcommand>` | Run a git subcommand, e.g. `:git status --short` |
| `:set <key> <value>` | Change a setting at runtime — see [Configuration](#configuration) |
| `:bookmark [name]` / `:bm` | Bookmark the current directory (defaults to its base name) |
| `:jump <name>` / `:j` | Jump to a saved bookmark |
| `:plugin <name> [arg]` | Invoke an action registered by a Lua plugin |
| `!<command>` | Run any shell command in the current directory, e.g. `!ls -la` |

Command mode keeps a **history** (`↑` / `↓`) and offers **Tab completion** for command verbs and
for destination paths in `:mv` and `:cp`.

Shell commands and your editor take over the terminal while they run; Trail restores the interface
and your full navigation state when they exit.

---

## Configuration

Trail runs on built-in defaults and does **not** scan `~/.config/trail/` on its own. You point it
at a config file once, and it remembers:

```sh
trail --config ~/dotfiles/trail.toml   # loads it — and remembers the path
trail                                  # reloads ~/dotfiles/trail.toml
trail --no-config                      # built-in defaults, this run only
```

```toml
[general]
editor = "nvim"                # command used to open files
git_status_enabled = true
text_sync_threshold_kb = 256   # larger text files are highlighted off-thread
fs_watch_debounce_ms = 200

[theme]
background   = "#1a1b26"
directory    = "blue"
selection_bg = "dark_gray"

[keymap.navigation]
move_down = "n"                # rebind anything
quit = "ctrl-q"

[plugins]
enabled = ["bookmarks"]
```

The schema is strictly validated — an unknown key or a malformed colour is an error, not a warning.
Settings can also be changed live with `:set editor nvim`, `:set theme.directory blue` or
`:set keymap.navigation.move_down n`, using the same validation. Runtime changes apply immediately
but are not written back to your file.

Full schema and config-resolution rules: **[docs/configuration_guide.md](docs/configuration_guide.md)**.

---

## Plugins

Trail embeds a Lua 5.4 runtime. Plugins are loaded from your OS config directory
(`~/.config/trail/<name>.lua`, or `%APPDATA%\trail\config\<name>.lua` on Windows) and enabled by
name in `trail.toml`.

```lua
-- ~/.config/trail/activity_logger.lua
trail.on_enter_dir(function(dir)
  trail.log("entered " .. dir)
end)

trail.register_action("note", function(arg)
  local f = io.open(os.getenv("HOME") .. "/notes.txt", "a")
  f:write(os.date("%F %T ") .. arg .. "\n")
  f:close()
end)
-- invoke with:  :plugin note some text
```

The API surface is `trail.log`, `trail.on_select`, `trail.on_enter_dir` and
`trail.register_action`. Runtime errors in plugin code are caught and logged — they never take
down the TUI.

Full guide with a worked end-to-end example: **[docs/plugin_guide.md](docs/plugin_guide.md)**.

---

## Documentation

| Document | Contents |
|---|---|
| [User guide](docs/user_guide.md) | Every mode, key and command in detail |
| [Installation](docs/installation.md) | All install methods, per-platform shell wrapper setup |
| [Configuration guide](docs/configuration_guide.md) | Full TOML schema, `:set` keys, config resolution |
| [Plugin guide](docs/plugin_guide.md) | Lua API, plugin load order, worked example |
| [Product spec](docs/trail.md) | What Trail is, and the design philosophy behind it |
| [Architecture](docs/trail_architecture.md) | Tech stack, UI thread / worker pool split, program logic |
| [Coding standard](docs/coding_standard.md) | Conventions every PR is held to |
| [Release process](docs/release_process.md) · [checklist](docs/release_checklist.md) | How a release is cut and verified |

---

## Development

```sh
git clone https://github.com/WeedonSctt/trail
cd trail
cargo run                       # opens Trail in the current directory
cargo run -- ~/some/path        # or somewhere else
```

Environment setup, including optional tooling:
[docs/trail_development_environment_setup_guide.md](docs/trail_development_environment_setup_guide.md).

### Architecture at a glance

Trail is a single process split into two concurrency domains. The **UI thread** owns the terminal
exclusively and must never block. An **async worker pool** handles everything with variable
latency — git status, filesystem watching, syntax highlighting, image decoding — and reports back
over an `mpsc` channel drained once per tick. Results are tagged with the selection they belong to,
so navigating quickly past several entries never renders a stale preview.

Preview types are pluggable through the `PreviewProvider` trait: a new type (PDF, archive, …) is a
new implementation registered at startup, not a change to the core loop. See
[docs/trail_architecture.md](docs/trail_architecture.md).

### Contributing

The project follows the [coding standard](docs/coding_standard.md). Every PR must pass:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps --all-features
```

CI runs the same four gates on every push and pull request.

---

## License

MIT — see [LICENSE](LICENSE).
