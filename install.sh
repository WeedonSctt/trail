#!/usr/bin/env sh
# Trail installer — Linux and macOS
#
# Downloads the appropriate release archive from GitHub, extracts it, and
# installs:
#   - The `trail` binary
#   - The shell wrappers (trail.bash, trail.zsh, trail.fish)
#
# Usage:
#   sh install.sh                   # latest release, defaults
#   INSTALL_DIR=$HOME/bin sh install.sh
#   VERSION=1.0.0 sh install.sh
#
# Environment variables:
#   INSTALL_DIR   Where to put the binary.  Default: ~/.local/bin
#   SHELL_DIR     Where to put the bash/zsh wrappers.
#                 Default: ~/.local/share/trail/shell
#   FISH_DIR      Where to put the fish wrapper.
#                 Default: ~/.config/fish/functions  (if it exists)
#                 Set to "" to skip fish installation.
#   VERSION       Release version to install (without the leading "v").
#                 Default: latest GitHub release.
#   GITHUB_REPO   GitHub owner/repo.  Default: WeedonSctt/trail
#   NO_COLOR      Set to any non-empty value to suppress colour output.
#
# After installation, source the appropriate wrapper in your shell's rc file:
#
#   Bash:  echo 'source ~/.local/share/trail/shell/trail.bash' >> ~/.bashrc
#   Zsh:   echo 'source ~/.local/share/trail/shell/trail.zsh'  >> ~/.zshrc
#   Fish:  (installed automatically to ~/.config/fish/functions/ if present)
#
# The shell function is required for cd-on-exit.  The binary alone cannot
# change the parent shell's working directory.

set -eu

##############################################################################
# Configuration
##############################################################################

GITHUB_REPO="${GITHUB_REPO:-WeedonSctt/trail}"
INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
SHELL_DIR="${SHELL_DIR:-${HOME}/.local/share/trail/shell}"
# FISH_DIR default is resolved later (after we confirm ~/.config/fish/functions exists).

##############################################################################
# Helpers
##############################################################################

# Portable ANSI colour helpers; suppressed when NO_COLOR is set or stdout is
# not a terminal.
_use_colour() {
    [ -z "${NO_COLOR:-}" ] && [ -t 1 ]
}

info()  { if _use_colour; then printf '\033[1;34m  info\033[0m  %s\n' "$*"; else printf '  info  %s\n' "$*"; fi; }
ok()    { if _use_colour; then printf '\033[1;32m    ok\033[0m  %s\n' "$*"; else printf '    ok  %s\n' "$*"; fi; }
warn()  { if _use_colour; then printf '\033[1;33m  warn\033[0m  %s\n' "$*"; else printf '  warn  %s\n' "$*"; fi; }
error() { if _use_colour; then printf '\033[1;31m error\033[0m  %s\n' "$*" >&2; else printf ' error  %s\n' "$*" >&2; fi; }
die()   { error "$*"; exit 1; }

# Require a command to be available in PATH.
need_cmd() {
    command -v "$1" > /dev/null 2>&1 || die "Required command not found: $1"
}

# Download a URL to stdout.  Prefers curl, falls back to wget.
download() {
    if command -v curl > /dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location "$1"
    elif command -v wget > /dev/null 2>&1; then
        wget --https-only --quiet --output-document=- "$1"
    else
        die "Neither curl nor wget found.  Install one and re-run this script."
    fi
}

##############################################################################
# Detect OS and architecture
##############################################################################

detect_target() {
    local os arch

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)
            case "$arch" in
                x86_64)  echo "x86_64-unknown-linux-gnu"  ;;
                aarch64) echo "aarch64-unknown-linux-gnu" ;;
                arm64)   echo "aarch64-unknown-linux-gnu" ;;
                *) die "Unsupported Linux architecture: $arch" ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                x86_64) echo "x86_64-apple-darwin"   ;;
                arm64)  echo "aarch64-apple-darwin"   ;;
                *) die "Unsupported macOS architecture: $arch" ;;
            esac
            ;;
        *) die "Unsupported operating system: $os.  For Windows, use install.ps1 instead." ;;
    esac
}

##############################################################################
# Resolve the version to install
##############################################################################

resolve_version() {
    if [ -n "${VERSION:-}" ]; then
        echo "$VERSION"
        return
    fi

    info "Resolving latest release version from GitHub…"
    local latest
    latest="$(
        download "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" \
        | grep '"tag_name":' \
        | sed -E 's/.*"tag_name": *"v?([^"]+)".*/\1/'
    )"

    [ -n "$latest" ] || die "Could not resolve the latest release version.  Set VERSION= and re-run."
    echo "$latest"
}

##############################################################################
# Main
##############################################################################

main() {
    need_cmd uname
    need_cmd tar
    need_cmd mktemp

    local target version archive_name archive_url tmpdir

    target="$(detect_target)"
    version="$(resolve_version)"
    archive_name="trail-v${version}-${target}.tar.gz"
    archive_url="https://github.com/${GITHUB_REPO}/releases/download/v${version}/${archive_name}"

    info "Installing trail v${version} for ${target}"
    info "Downloading ${archive_url}"

    # Work in a temporary directory that is cleaned up on exit.
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    download "$archive_url" > "${tmpdir}/${archive_name}"

    # Optionally verify the SHA-256 checksum if sha256sum / shasum is present.
    if command -v sha256sum > /dev/null 2>&1 || command -v shasum > /dev/null 2>&1; then
        info "Downloading checksums.txt for verification…"
        if download "https://github.com/${GITHUB_REPO}/releases/download/v${version}/checksums.txt" \
                > "${tmpdir}/checksums.txt" 2>/dev/null; then
            # Extract the expected digest for this archive.
            local expected
            expected="$(grep "${archive_name}" "${tmpdir}/checksums.txt" | awk '{print $1}')"
            if [ -n "$expected" ]; then
                local actual
                if command -v sha256sum > /dev/null 2>&1; then
                    actual="$(sha256sum "${tmpdir}/${archive_name}" | awk '{print $1}')"
                else
                    actual="$(shasum -a 256 "${tmpdir}/${archive_name}" | awk '{print $1}')"
                fi
                if [ "$actual" != "$expected" ]; then
                    die "SHA-256 checksum mismatch for ${archive_name}!
  Expected: ${expected}
  Actual:   ${actual}
The download may be corrupted.  Delete any cached files and retry."
                fi
                ok "Checksum verified"
            else
                warn "Archive not found in checksums.txt; skipping verification."
            fi
        else
            warn "Could not download checksums.txt; skipping verification."
        fi
    else
        warn "sha256sum/shasum not found; skipping checksum verification."
    fi

    # Extract the archive.
    tar -xzf "${tmpdir}/${archive_name}" -C "$tmpdir"
    local extracted_dir="${tmpdir}/trail-v${version}-${target}"

    # Install the binary.
    mkdir -p "$INSTALL_DIR"
    cp "${extracted_dir}/trail" "${INSTALL_DIR}/trail"
    chmod 755 "${INSTALL_DIR}/trail"
    ok "Binary installed to ${INSTALL_DIR}/trail"

    # Install the bash + zsh wrappers.
    mkdir -p "$SHELL_DIR"
    cp "${extracted_dir}/shell/trail.bash" "${SHELL_DIR}/trail.bash"
    cp "${extracted_dir}/shell/trail.zsh"  "${SHELL_DIR}/trail.zsh"
    ok "Shell wrappers installed to ${SHELL_DIR}/"

    # Install the fish wrapper if ~/.config/fish/functions exists (or FISH_DIR
    # is explicitly set to a non-empty value).
    local fish_dir="${FISH_DIR:-}"
    if [ -z "$fish_dir" ]; then
        if [ -d "${HOME}/.config/fish/functions" ]; then
            fish_dir="${HOME}/.config/fish/functions"
        fi
    fi
    if [ -n "$fish_dir" ]; then
        mkdir -p "$fish_dir"
        cp "${extracted_dir}/shell/trail.fish" "${fish_dir}/trail.fish"
        ok "Fish wrapper installed to ${fish_dir}/trail.fish"
    fi

    # Ensure INSTALL_DIR is on PATH (best-effort advisory only).
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) ;;  # already on PATH
        *)
            warn "${INSTALL_DIR} is not on your PATH."
            warn "Add it:  export PATH=\"\$PATH:${INSTALL_DIR}\""
            ;;
    esac

    echo ""
    info "Installation complete!"
    echo ""
    echo "  To enable cd-on-exit, source the wrapper for your shell:"
    echo ""
    echo "    Bash:  echo 'source ${SHELL_DIR}/trail.bash' >> ~/.bashrc && source ~/.bashrc"
    echo "    Zsh:   echo 'source ${SHELL_DIR}/trail.zsh'  >> ~/.zshrc  && source ~/.zshrc"
    if [ -n "$fish_dir" ]; then
        echo "    Fish:  wrapper installed automatically — open a new fish session."
    else
        echo "    Fish:  copy ${extracted_dir}/shell/trail.fish to ~/.config/fish/functions/"
    fi
    echo ""
    echo "  Then run:  trail"
    echo ""
}

main "$@"
