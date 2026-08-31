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

---

## Where Trail Keeps Things

Trail writes to three places, and the paths differ per platform. Rather than
guessing, ask the binary:

```sh
trail --paths
```

It prints the binary, the configuration and data directories, the log file, and
any shell wrappers it finds, each marked with whether it exists. This is the
authoritative answer for the build you are actually running, and it is what the
uninstall instructions below refer to.

| | Linux | macOS | Windows |
|---|---|---|---|
| Config, plugins | `~/.config/trail/` | `~/Library/Application Support/trail/` | `%APPDATA%\trail\config\` |
| Bookmarks, recent dirs, remembered `--config` | `~/.local/share/trail/` | `~/Library/Application Support/trail/` | `%APPDATA%\trail\data\` |
| Log | `$TMPDIR/trail.log` | `$TMPDIR/trail.log` | `%TEMP%\trail.log` |

---

## Uninstalling

Use the method that matches how you installed. In every case, **configuration
and data are left alone unless you explicitly ask for them to be removed** —
uninstalling a program should not destroy the bookmarks you built up in it.

Whatever the method, if you added a wrapper line to your shell rc file by hand,
remove it. Leave it behind and every new shell will complain about a file that
no longer exists. The uninstall scripts find that line and print it for you;
they do not edit your rc files.

### Install script (Linux and macOS)

> **Availability.** The uninstall scripts are attached to releases from
> **v1.1.0** onward. On v1.0.1 and earlier the `releases/latest/download/`
> URLs below return 404 — fetch the script from the repository instead:
> `curl -fsSL https://raw.githubusercontent.com/WeedonSctt/trail/main/uninstall.sh | sh`

```sh
curl -fsSL https://github.com/WeedonSctt/trail/releases/latest/download/uninstall.sh | sh
```

A piped script cannot take arguments directly. To pass `--dry-run` or
`--purge`, hand them to `sh` after `-s --`:

```sh
curl -fsSL .../uninstall.sh | sh -s -- --dry-run
curl -fsSL .../uninstall.sh | sh -s -- --purge
```

Or, if you still have the repository or a release archive:

```sh
sh uninstall.sh --dry-run   # show what would be removed, change nothing
sh uninstall.sh             # remove the binary and the shell wrappers
sh uninstall.sh --purge     # also remove configuration and data
```

It honours the same environment variables as `install.sh`, so a custom install
is undone by passing the same values:

```sh
INSTALL_DIR=$HOME/bin SHELL_DIR=$HOME/.trail sh uninstall.sh
```

### Install script (Windows)

> **Availability.** As above, the release asset exists from **v1.1.0** onward.
> On earlier versions use the repository copy:
> `irm https://raw.githubusercontent.com/WeedonSctt/trail/main/uninstall.ps1 | iex`

```pwsh
irm https://github.com/WeedonSctt/trail/releases/latest/download/uninstall.ps1 | iex
```

`iex` runs the script text with no way to pass parameters, so that form always
takes the defaults. For `-DryRun` or `-Purge`, turn the downloaded text into a
script block and call it:

```pwsh
& ([scriptblock]::Create((irm .../uninstall.ps1))) -DryRun
& ([scriptblock]::Create((irm .../uninstall.ps1))) -Purge
```

Or from a downloaded copy:

```pwsh
pwsh -File uninstall.ps1 -DryRun
pwsh -File uninstall.ps1
pwsh -File uninstall.ps1 -Purge
```

Windows PowerShell 5.1 works too, via
`powershell -ExecutionPolicy Bypass -File uninstall.ps1`.

This also removes the `%LOCALAPPDATA%	railin` entry that `install.ps1` added
to your user PATH. Quit any running Trail first — Windows will not delete an
executable that is still mapped into a live process.

### Homebrew

```sh
brew uninstall trail
brew untap WeedonSctt/trail   # optional, if you added the tap
```

### AUR

```sh
sudo pacman -Rns trail        # or: paru -Rns trail
```

### Scoop

```pwsh
scoop uninstall trail
```

### `cargo install`

```sh
cargo uninstall trail
```

`cargo install` never installed the shell wrappers, so there are none to
remove — but if you set one up by hand, remove its rc line too.

### Removing configuration and data by hand

The uninstall scripts do this for you with `--purge` / `-Purge`. To do it
yourself, run `trail --paths` first to get the exact locations, then remove the
configuration and data directories it names.

> **Linux users:** the default wrapper directory `~/.local/share/trail/shell`
> lives *inside* the data directory `~/.local/share/trail`. Removing the parent
> to get rid of the wrappers takes your bookmarks with it. Remove the `shell`
> subdirectory specifically, or use `uninstall.sh`, which handles this.
