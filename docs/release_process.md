# Trail — Release Process

This document is for Trail **maintainers** and contributors. It describes how to cut a release, what CI does automatically, and what must be done manually.

---

## Overview

The release workflow is fully automated once a version tag is pushed. The maintainer's job is:

1. Prepare the release: close the `CHANGELOG.md` `[Unreleased]` section into `[X.Y.Z]`, and bump the version in `Cargo.toml` and the four package manifests.
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

### 2. Bump the version and close the changelog

Update the version in `Cargo.toml`:

```toml
[package]
version = "X.Y.Z"
```

Update `pkg/homebrew/trail.rb`, `pkg/aur/PKGBUILD`, `pkg/aur/.SRCINFO`, and `pkg/scoop/trail.json` to reference the new version. `.SRCINFO` has to agree with `PKGBUILD` or the AUR package breaks. *(SHA-256 values are filled in after the release archives exist — see step 4.)*

Then **close the changelog**, in the same commit:

1. Rename `## [Unreleased]` in `CHANGELOG.md` to `## [X.Y.Z] - YYYY-MM-DD`.
2. Open a fresh, empty `## [Unreleased]` above it.
3. Update the link definitions at the foot: `[Unreleased]` now compares `vX.Y.Z...HEAD`, and add a `[X.Y.Z]` link.

The section should already describe every user-facing change, because entries are added as the work lands rather than reconstructed here — see `CLAUDE.md` §5. If it is empty and the release is not a pure packaging fix, something was missed; find it before tagging.

Commit with message `[~] release> bump version to vX.Y.Z`, per `CLAUDE.md` §7. (The `[Phase N] ...` prefix this document used to specify is from the phased build-out and is no longer what the history uses.)

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

Watch the workflow in the **Actions** tab.

If a build job fails, the tag has produced no published release, so it is safe to delete and re-push after fixing: `git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z`, then restart from step 1. **This is the only case in which a tag may be deleted.** Once the release is published (step 5), the tag is permanent — someone may have pinned it — and a bad release is fixed by the next patch release, not by moving the tag. See `CLAUDE.md` §6.

### 5. Publish the GitHub Release

Once CI completes:

1. Open the draft Release on GitHub.
2. Set the title to `Trail vX.Y.Z` and write the notes **from the changelog section you closed in step 2**. The workflow sets `generate_release_notes: true`, which produces a list of commit subjects — that is a record of what changed in the code, not something an upgrading user can read. Replace it.
3. Verify that all expected assets are attached:
   - `trail-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
   - `trail-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz`
   - `trail-vX.Y.Z-x86_64-apple-darwin.tar.gz`
   - `trail-vX.Y.Z-aarch64-apple-darwin.tar.gz`
   - `trail-vX.Y.Z-x86_64-pc-windows-msvc.zip`
   - `checksums.txt`
   - `install.sh`, `install.ps1`
   - `uninstall.sh`, `uninstall.ps1`

   Ten in total. The four scripts are attached as *assets*, not merely present in the repository, because the documented one-liners resolve against the release (`.../releases/latest/download/install.sh`). A missing one turns a documented command into a 404.
4. Verify `checksums.txt` contains a line for every archive, and check one digest yourself rather than trusting the job.
5. Click **Publish release**, and mark it as the latest release.

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
