##
## Homebrew formula for Trail — a terminal file manager.
##
## Usage (after tapping):
##   brew tap <WeedonSctt>/trail
##   brew install trail
##
## To submit to homebrew-core, replace this file's URL/sha256 values and
## open a PR against homebrew/homebrew-core following their contribution
## guidelines.
##
## NOTE ON SHA256 PLACEHOLDERS:
##   The sha256 values below are placeholders that MUST be replaced with the
##   actual digests of the release archives before this formula is published.
##   Run `sha256sum` (Linux) or `shasum -a 256` (macOS) against each archive,
##   or read them from the checksums.txt attached to the GitHub Release.
##
## NOTE ON SHELL WRAPPERS:
##   Trail's "cd-on-exit" behaviour requires a shell function, not just the
##   binary.  This formula installs shell/trail.bash, shell/trail.zsh, and
##   shell/trail.fish alongside the binary.  The `caveats` block below tells
##   users how to activate the wrapper for their shell.
##

class Trail < Formula
  desc "Terminal-first workspace for navigating, inspecting and acting on the filesystem"
  homepage "https://github.com/WeedonSctt/trail"
  version "1.0.0"
  license "MIT"  # Update to match the actual LICENSE file when added.

  # ── Platform-specific source archives ─────────────────────────────────────
  # Each archive is produced by the GitHub Actions release workflow and
  # contains: the binary + shell/ directory + README.md.

  on_macos do
    on_arm do
      url "https://github.com/WeedonSctt/trail/releases/download/v#{version}/trail-v#{version}-aarch64-apple-darwin.tar.gz"
      # TODO(release): replace with `shasum -a 256` of the arm64 macOS archive.
      sha256 "PLACEHOLDER_SHA256_AARCH64_APPLE_DARWIN"
    end

    on_intel do
      url "https://github.com/WeedonSctt/trail/releases/download/v#{version}/trail-v#{version}-x86_64-apple-darwin.tar.gz"
      # TODO(release): replace with `shasum -a 256` of the x86_64 macOS archive.
      sha256 "PLACEHOLDER_SHA256_X86_64_APPLE_DARWIN"
    end
  end

  # ── No bottles — this formula uses pre-built release archives. ─────────────
  # Once submitted to homebrew-core, bottles will be built by the Homebrew
  # infrastructure.  For a tap-only formula, this section can stay empty.

  def install
    # Install the compiled binary.
    bin.install "trail"

    # Install the shell wrappers into a share directory so they survive Homebrew
    # upgrades without clobbering anything in the user's home directory.
    (share/"trail/shell").install "shell/trail.bash"
    (share/"trail/shell").install "shell/trail.zsh"

    # Fish: Homebrew manages a vendor_functions.d directory that fish picks up
    # automatically, so no user action is needed for fish.
    (share/"fish/vendor_functions.d").install "shell/trail.fish"
  end

  def caveats
    <<~EOS
      Trail's "cd-on-exit" behaviour requires a shell wrapper function.
      The binary alone cannot change the parent shell's working directory.

      ── Bash ────────────────────────────────────────────────────────────────
      Add the following line to your ~/.bashrc or ~/.bash_profile:

        source #{opt_share}/trail/shell/trail.bash

      Then reload your shell:

        source ~/.bashrc   # or open a new terminal

      ── Zsh ─────────────────────────────────────────────────────────────────
      Add the following line to your ~/.zshrc:

        source #{opt_share}/trail/shell/trail.zsh

      Then reload your shell:

        source ~/.zshrc   # or open a new terminal

      ── Fish ─────────────────────────────────────────────────────────────────
      The fish wrapper was installed to Homebrew's vendor_functions.d directory
      and is available automatically in new fish sessions.  No extra setup is
      needed.

      ── How it works ─────────────────────────────────────────────────────────
      The wrapper creates a temporary file, passes it to the Trail binary via
      --cwd-file, and after Trail exits reads the path Trail wrote there and
      calls `cd` on it.  On cancellation (Ctrl-c / Esc quit) Trail writes
      nothing and your shell stays where it was.
    EOS
  end

  test do
    # Smoke-test: verify the binary runs and exits cleanly with --version.
    # A full interactive TUI cannot be exercised in a headless Homebrew test,
    # so we just confirm the binary is executable and returns a version string.
    assert_match version.to_s, shell_output("#{bin}/trail --version 2>&1", 0)
  end
end
