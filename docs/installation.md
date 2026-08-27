# Trail — Installation Guide

Trail is a terminal-first workspace for navigating, inspecting and acting on the filesystem.

> **Note on shell wrappers:** Trail's "cd-on-exit" behaviour — where your shell changes to the last directory Trail was browsing — requires a **shell wrapper function**. Every installation method below includes the wrapper. Sourcing it is a one-time step per shell; see [Shell Wrapper Setup](#shell-wrapper-setup) for instructions.

---

## Installation Methods

### 1. Homebrew (macOS — recommended for macOS users)

First tap the Trail repository:

```sh
brew tap WeedonSctt/trail
brew install trail
```

The formula installs:
- The `trail` binary (added to `PATH` automatically by Homebrew)
- `trail.bash` and `trail.zsh` to `$(brew --prefix)/share/trail/shell/`
- `trail.fish` to Homebrew's `vendor_functions.d/` (loaded automatically by fish)

After installation, source the wrapper for your shell — see [Shell Wrapper Setup](#shell-wrapper-setup).

---

### 2. AUR (Arch Linux)

Using an AUR helper (e.g. `paru` or `yay`):

```sh
paru -S trail
# or
yay -S trail
```

Or manually:

```sh
git clone https://aur.archlinux.org/trail.git
cd trail
makepkg -si
```

The package installs:
- The binary to `/usr/bin/trail`
- Shell wrappers to `/usr/share/trail/shell/`
- The fish wrapper to `/usr/share/fish/vendor_functions.d/trail.fish` (auto-loaded)

Source the wrapper for your shell — see [Shell Wrapper Setup](#shell-wrapper-setup).

---

### 3. Scoop (Windows)

First add the bucket:

```pwsh
scoop bucket add trail https://github.com/WeedonSctt/trail-scoop-bucket
scoop install trail
```

The manifest installs `trail.exe` to your Scoop apps directory and adds it to `PATH` automatically.

To enable cd-on-exit, add the wrapper to your PowerShell profile:

```pwsh
# Add this line to your $PROFILE:
. "$env:SCOOP\apps\trail\current\shell\trail.ps1"
```

---

### 4. Install script (Linux and macOS)

```sh
curl -fsSL https://github.com/WeedonSctt/trail/releases/latest/download/install.sh | sh
```

Or download and inspect before running:

```sh
curl -fsSL https://github.com/WeedonSctt/trail/releases/latest/download/install.sh -o install.sh
less install.sh      # review the script
sh install.sh
```

Defaults:
- Binary: `~/.local/bin/trail`
- Shell wrappers: `~/.local/share/trail/shell/`
- Fish wrapper: `~/.config/fish/functions/trail.fish` (if that directory exists)

Customise with environment variables:

```sh
INSTALL_DIR=$HOME/bin SHELL_DIR=$HOME/.config/trail/shell sh install.sh
VERSION=1.0.1 sh install.sh   # pin a specific version
```

---

### 5. Install script (Windows)

```pwsh
irm https://github.com/WeedonSctt/trail/releases/latest/download/install.ps1 | iex
```

Or download and inspect:

```pwsh
irm https://github.com/WeedonSctt/trail/releases/latest/download/install.ps1 -OutFile install.ps1
Get-Content install.ps1   # review
.\install.ps1
```

Defaults:
- Binary: `$env:LOCALAPPDATA\trail\bin\trail.exe`
- Wrapper: `$env:LOCALAPPDATA\trail\bin\trail.ps1`
- Adds the install directory to the user-scope `PATH` automatically

---

### 6. `cargo install`

If you have Rust installed:

```sh
cargo install trail
```

This compiles the binary from source. After installation, the binary is at `~/.cargo/bin/trail`. The shell wrappers are **not** installed automatically — download them separately from the [latest release](https://github.com/WeedonSctt/trail/releases/latest) or copy them from the repository's `shell/` directory.

---

### 7. Download a release archive manually

Download the appropriate archive from the [Releases page](https://github.com/WeedonSctt/trail/releases):

| Platform | Archive |
|---|---|
| Linux x86_64 | `trail-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `trail-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| macOS x86_64 | `trail-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| macOS arm64 | `trail-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `trail-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

Every archive contains:

```
trail-vX.Y.Z-<target>/
  trail[.exe]
  shell/
    trail.bash
    trail.zsh
    trail.fish
    trail.ps1
  README.md
```

Extract and place the binary somewhere on your `PATH`, then follow the shell wrapper setup below.

Verify the download with `checksums.txt` (also on the Releases page):

```sh
sha256sum --check checksums.txt
```

---

## Shell Wrapper Setup

Trail cannot change the parent shell's working directory by itself — that requires a shell function that calls the binary and then calls `cd`. This is a fundamental OS restriction, not a Trail limitation.

### Bash

Add to `~/.bashrc` (or `~/.bash_profile`):

```bash
source /path/to/trail/shell/trail.bash
```

Reload:

```bash
source ~/.bashrc
```

### Zsh

Add to `~/.zshrc`:

```zsh
source /path/to/trail/shell/trail.zsh
```

Reload:

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

Reload:

```pwsh
. $PROFILE
```

---

## Verifying the Installation

After installing and sourcing the shell wrapper, run:

```sh
trail --version
```

Then launch Trail:

```sh
trail
```

Navigate to a directory and quit with `q`. Your shell should `cd` to the last-browsed directory.
