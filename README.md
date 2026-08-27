# Trail

A terminal-first workspace for navigating, inspecting and acting on the filesystem without leaving the shell.

---

## Installation

See [docs/installation.md](docs/installation.md) for the full guide. Quick paths:

### Homebrew (macOS)

```sh
brew tap WeedonSctt/trail
brew install trail
```

### AUR (Arch Linux)

```sh
paru -S trail
# or: yay -S trail
```

### Scoop (Windows)

```pwsh
scoop bucket add trail https://github.com/WeedonSctt/trail-scoop-bucket
scoop install trail
```

### Install script (Linux / macOS)

```sh
curl -fsSL https://github.com/WeedonSctt/trail/releases/latest/download/install.sh | sh
```

### Install script (Windows)

```pwsh
irm https://github.com/WeedonSctt/trail/releases/latest/download/install.ps1 | iex
```

### `cargo install`

```sh
cargo install trail
```

> **Note:** `cargo install` installs only the binary. Download the shell wrappers separately from the [Releases page](https://github.com/WeedonSctt/trail/releases/latest) if you want cd-on-exit behaviour.

---

## Shell Integration

Trail's "cd-on-exit" behaviour — where your shell changes to the last directory Trail was browsing after you quit — requires a shell wrapper function. The wrapper is what invokes the `trail` binary; the binary on its own cannot change the parent shell's working directory.

The wrapper is included in every release archive and in every package manager formula. After installation, source it once per shell:

### Bash

Add to your `~/.bashrc`:

```bash
source /path/to/trail/shell/trail.bash
```

Then reload:

```bash
source ~/.bashrc
```

### Zsh

Add to your `~/.zshrc`:

```zsh
source /path/to/trail/shell/trail.zsh
```

Then reload:

```zsh
source ~/.zshrc
```

### Fish

Copy to fish's functions directory (fish autoloads it automatically):

```fish
cp /path/to/trail/shell/trail.fish ~/.config/fish/functions/trail.fish
```

Or source manually in `~/.config/fish/config.fish`:

```fish
source /path/to/trail/shell/trail.fish
```

### PowerShell

Add to your `$PROFILE`:

```pwsh
. "/path/to/trail/shell/trail.ps1"
```

Then reload:

```pwsh
. $PROFILE
```

### How it works

The shell wrapper creates a temporary file and passes it to the Trail binary via `--cwd-file`. On a **normal exit** (`q`), Trail writes its current directory to the file and the wrapper calls `cd` on it. On **cancellation** (`Ctrl-c` or `Esc`-quit), Trail writes nothing, so the shell stays in its original directory.

---

## Usage

```
trail [<start-path>]
```

| Key | Action |
|---|---|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `l` / `Enter` | Enter directory / open file in `$EDITOR` |
| `h` / `Backspace` | Go to parent directory |
| `gg` | Jump to top |
| `G` | Jump to bottom |
| `u` | Navigate back |
| `Ctrl-r` | Navigate forward |
| `/` | Enter Search Mode |
| `:` | Enter Command Mode |
| `q` | Quit (cd-on-exit if wrapper is sourced) |
| `Ctrl-c` | Quit without changing directory |

---

## Contributing

See [docs/release_process.md](docs/release_process.md) for how to cut a release.

The project follows the [coding standard](docs/coding_standard.md). Every PR must pass:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps --all-features
```
