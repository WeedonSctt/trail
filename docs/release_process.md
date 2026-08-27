# Trail — Release Process

This document is for Trail **maintainers** and contributors. It describes how to cut a release, what CI does automatically, and what must be done manually.

---

## Overview

The release workflow is fully automated once a version tag is pushed. The maintainer's job is:

1. Prepare the release (update version, changelog).
2. Push the tag — CI does the rest.
3. Review the draft GitHub Release, fill in the notes, and publish.
4. Update package manager formulas (Homebrew, AUR, Scoop).

---

## Step-by-step

### 1. Verify the project is releasable

Run all quality gates locally before tagging:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --all-features
cargo test --workspace --all-features
cargo doc --workspace --no-deps --all-features
```

All must pass. A failing CI quality gate will block the release workflow.

### 2. Bump the version

Update the version in `Cargo.toml`:

```toml
[package]
version = "X.Y.Z"
```

Commit with message `[Phase 9] Bump version to vX.Y.Z`.

Update `pkg/homebrew/trail.rb`, `pkg/aur/PKGBUILD`, `pkg/aur/.SRCINFO`, and `pkg/scoop/trail.json` to reference the new version. *(SHA-256 values are filled in after the release archives exist — see step 4.)*

### 3. Push the tag

```sh
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push origin vX.Y.Z
```

The release workflow (`.github/workflows/release.yml`) fires immediately on tag push.

### 4. Monitor CI

The release workflow:

1. Runs `fmt`, `clippy`, `test`, and `doc` — fails fast if any gate fails.
2. Builds release binaries for all five targets in parallel.
3. Packages each binary with the shell wrappers into an archive.
4. Generates `checksums.txt`.
5. Creates a **draft** GitHub Release and uploads all assets.

Watch the workflow in the **Actions** tab. If a build job fails, fix the issue, delete the tag, and restart from step 1.

### 5. Publish the GitHub Release

Once CI completes:

1. Open the draft Release on GitHub.
2. Review the auto-generated release notes; edit if needed.
3. Verify that all expected assets are attached:
   - `trail-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
   - `trail-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz`
   - `trail-vX.Y.Z-x86_64-apple-darwin.tar.gz`
   - `trail-vX.Y.Z-aarch64-apple-darwin.tar.gz`
   - `trail-vX.Y.Z-x86_64-pc-windows-msvc.zip`
   - `checksums.txt`
4. Verify `checksums.txt` contains a line for every archive.
5. Click **Publish release**.

### 6. Update Homebrew formula

After the release is published:

1. Download the two macOS archives and compute their SHA-256 digests:

   ```sh
   shasum -a 256 trail-vX.Y.Z-aarch64-apple-darwin.tar.gz
   shasum -a 256 trail-vX.Y.Z-x86_64-apple-darwin.tar.gz
   ```

   Or read them from `checksums.txt`.

2. Update `pkg/homebrew/trail.rb`:
   - Set `version "X.Y.Z"`
   - Replace the `sha256` placeholder values with the actual digests.

3. If this formula lives in a tap:
   ```sh
   cd homebrew-trail   # your tap repository
   cp /path/to/trail/pkg/homebrew/trail.rb Formula/trail.rb
   git add Formula/trail.rb
   git commit -m "trail X.Y.Z"
   git push
   ```

### 7. Update AUR package

1. Compute the SHA-256 digests for the Linux archives:

   ```sh
   sha256sum trail-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
   sha256sum trail-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz
   ```

2. Update `pkg/aur/PKGBUILD`:
   - Set `pkgver=X.Y.Z`
   - Replace the `sha256sums_*` placeholder values with the actual digests.

3. Regenerate `.SRCINFO`:

   ```sh
   cd pkg/aur
   makepkg --printsrcinfo > .SRCINFO
   ```

4. Push to the AUR:

   ```sh
   git clone ssh://aur@aur.archlinux.org/trail.git aur-trail
   cp pkg/aur/PKGBUILD aur-trail/
   cp pkg/aur/.SRCINFO aur-trail/
   cp pkg/aur/trail.install aur-trail/
   cd aur-trail
   git add PKGBUILD .SRCINFO trail.install
   git commit -m "Update to vX.Y.Z"
   git push
   ```

### 8. Update Scoop manifest

1. Compute the SHA-256 digest for the Windows archive:

   ```sh
   sha256sum trail-vX.Y.Z-x86_64-pc-windows-msvc.zip
   # or on Windows:
   (Get-FileHash trail-vX.Y.Z-x86_64-pc-windows-msvc.zip -Algorithm SHA256).Hash
   ```

2. Update `pkg/scoop/trail.json`:
   - Set `"version": "X.Y.Z"`
   - Replace the `hash` placeholder value.
   - Update the `extract_dir` field if needed.

3. If this manifest lives in a Scoop bucket repository, update and push it there.

---

## Shell Wrappers — Bundling Reminder

**Every release archive MUST contain the shell wrappers.**

The GitHub Actions release workflow stages them automatically (see `.github/workflows/release.yml`). If you ever build release archives manually, verify that `shell/trail.bash`, `shell/trail.zsh`, `shell/trail.fish`, and `shell/trail.ps1` are inside every archive before uploading.

The binary alone does not provide cd-on-exit behaviour. Shipping a binary-only archive is a regression.

---

## Rolling Back a Release

If a critical bug is found immediately after publishing:

1. **Do not delete the tag** — this breaks anyone who has pinned the version.
2. Yank the release on crates.io if it was published there:  
   `cargo yank --version X.Y.Z`
3. Fix the bug, cut `vX.Y.Z+1` (or `vX.Y.(Z+1)`) immediately.
4. Update the GitHub Release notes with a warning and a link to the fixed release.

---

## Cargo Install Verification

The `cargo install` path should be verified with a fresh `--root`:

```sh
cargo install trail --root /tmp/trail-install-test
/tmp/trail-install-test/bin/trail --version
```

This confirms the crates.io-published version builds and produces a working binary. Note that `cargo install` does not install shell wrappers — document this in the release notes.
