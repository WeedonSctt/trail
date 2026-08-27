# Trail — Release Checklist

Use this checklist for every tagged release. It is a **repeatable gate**, not a one-time task.

Copy this list into the GitHub Release draft and check off items as you go.

---

## Pre-Release (run before pushing the tag)

### Code quality
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `cargo build --workspace --all-features` succeeds
- [ ] `cargo test --workspace --all-features` passes
- [ ] `cargo doc --workspace --no-deps --all-features` builds without warnings

### Version consistency
- [ ] `[package] version` in `Cargo.toml` matches the intended tag
- [ ] `pkg/homebrew/trail.rb` — `version` field updated
- [ ] `pkg/aur/PKGBUILD` — `pkgver` updated
- [ ] `pkg/aur/.SRCINFO` — regenerated with `makepkg --printsrcinfo`
- [ ] `pkg/scoop/trail.json` — `version` field updated

### Content review
- [ ] `CHANGELOG.md` (or release notes) describes all user-facing changes
- [ ] No `// TODO(phase-N):` markers remain for phases ≤ current

---

## Tag and CI

- [ ] Tag pushed: `git push origin vX.Y.Z`
- [ ] GitHub Actions release workflow started (check Actions tab)
- [ ] Quality-gate job passed (fmt / clippy / test / doc)
- [ ] All five build jobs passed:
  - [ ] `x86_64-unknown-linux-gnu`
  - [ ] `aarch64-unknown-linux-gnu`
  - [ ] `x86_64-apple-darwin`
  - [ ] `aarch64-apple-darwin`
  - [ ] `x86_64-pc-windows-msvc`
- [ ] Checksums job passed
- [ ] Draft GitHub Release created with all assets attached

---

## GitHub Release Review

- [ ] Release title: `Trail vX.Y.Z`
- [ ] All six expected assets are attached:
  - [ ] `trail-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
  - [ ] `trail-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz`
  - [ ] `trail-vX.Y.Z-x86_64-apple-darwin.tar.gz`
  - [ ] `trail-vX.Y.Z-aarch64-apple-darwin.tar.gz`
  - [ ] `trail-vX.Y.Z-x86_64-pc-windows-msvc.zip`
  - [ ] `checksums.txt`
- [ ] `checksums.txt` contains one line per archive (5 lines total)
- [ ] Each archive contains: binary + `shell/` directory + README
- [ ] Release notes are accurate and complete
- [ ] Release published (un-draft)

---

## Post-Release: Package Manager Updates

### Homebrew
- [ ] SHA-256 computed for `aarch64-apple-darwin` archive
- [ ] SHA-256 computed for `x86_64-apple-darwin` archive
- [ ] `pkg/homebrew/trail.rb` — placeholders replaced with actual digests
- [ ] Formula pushed to the tap repository (or PR opened against homebrew-core)
- [ ] `brew update && brew install trail` installs successfully on macOS arm64
- [ ] `brew update && brew install trail` installs successfully on macOS x86_64

### AUR
- [ ] SHA-256 computed for `x86_64-unknown-linux-gnu` archive
- [ ] SHA-256 computed for `aarch64-unknown-linux-gnu` archive
- [ ] `pkg/aur/PKGBUILD` — placeholders replaced with actual digests
- [ ] `.SRCINFO` regenerated: `makepkg --printsrcinfo > .SRCINFO`
- [ ] Updated PKGBUILD + .SRCINFO pushed to AUR git repository
- [ ] `paru -S trail` installs successfully on Arch Linux (x86_64)

### Scoop
- [ ] SHA-256 computed for `x86_64-pc-windows-msvc` archive
- [ ] `pkg/scoop/trail.json` — placeholder replaced with actual digest
- [ ] Manifest pushed to the Scoop bucket repository
- [ ] `scoop update && scoop install trail` installs successfully on Windows

### cargo install
- [ ] Version published to crates.io (if applicable):
  `cargo publish --dry-run` then `cargo publish`
- [ ] `cargo install trail` produces a working binary:
  `cargo install trail --root /tmp/test && /tmp/test/bin/trail --version`

---

## Fresh-Install Validation (per platform, per channel)

Run these on a **clean machine or VM** with no prior Trail installation.

> This section is the most important part of the checklist. Automation catches build failures; only a fresh install catches packaging bugs.

### Linux — install.sh
- [ ] Script downloads, verifies checksum, and completes without error
- [ ] `trail --version` prints the correct version
- [ ] Shell wrapper sourced in bash → `trail` resolves to the function
- [ ] Navigate to a directory, quit with `q` → shell is in that directory
- [ ] `Ctrl-c` from Trail → shell remains in original directory

### Linux — AUR (Arch)
- [ ] `paru -S trail` completes; post-install message is shown
- [ ] Binary and wrappers installed at expected paths
- [ ] cd-on-exit works in bash and zsh after sourcing wrapper
- [ ] Fish wrapper auto-loads (open a new fish session, run `trail`, quit)

### macOS — install.sh
- [ ] Script runs on macOS arm64 (M-series)
- [ ] Script runs on macOS x86_64
- [ ] cd-on-exit works in bash and zsh

### macOS — Homebrew
- [ ] `brew install trail` on arm64 installs arm64 binary
- [ ] `brew install trail` on x86_64 installs x86_64 binary
- [ ] Caveats block shown; wrapper sourcing instructions are correct
- [ ] Fish wrapper auto-loads via vendor_functions.d

### Windows — install.ps1
- [ ] Script runs without errors
- [ ] Binary added to PATH (open a new PowerShell session, run `trail --version`)
- [ ] `trail.ps1` present in install directory
- [ ] After adding `. "$InstallDir\trail.ps1"` to `$PROFILE`: cd-on-exit works

### Windows — Scoop
- [ ] `scoop install trail` completes
- [ ] `trail.exe` on PATH
- [ ] Notes block displayed; wrapper path is correct
- [ ] After adding wrapper to `$PROFILE`: cd-on-exit works

### cargo install
- [ ] `cargo install trail` from crates.io (or `--git`) succeeds
- [ ] Binary is at `~/.cargo/bin/trail` and on PATH
- [ ] Note: shell wrappers are NOT installed — document this in release notes

---

## Sign-Off

- [ ] All automated gates passed in CI
- [ ] At least one fresh-install validation per platform completed
- [ ] GitHub Release published
- [ ] Package managers updated with real SHA-256 values
- [ ] Release checklist archived (e.g. in the GitHub Release description)
