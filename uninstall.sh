#!/usr/bin/env sh
# Trail uninstaller — Linux and macOS
#
# Removes what `install.sh` installed: the binary and the shell wrappers.
# Configuration and data (bookmarks, recent directories, the remembered
# --config path) are LEFT ALONE unless you pass --purge.
#
# Usage:
#   sh uninstall.sh                 # remove the binary and wrappers
#   sh uninstall.sh --purge         # also remove config and data
#   sh uninstall.sh --dry-run       # print what would be removed, change nothing
#   INSTALL_DIR=$HOME/bin sh uninstall.sh
#
# Environment variables — the same contract install.sh uses, so a custom
# install can be undone by passing the same values:
#   INSTALL_DIR   Where the binary was put.  Default: ~/.local/bin
#   SHELL_DIR     Where the bash/zsh wrappers were put.
#                 Default: ~/.local/share/trail/shell
#   FISH_DIR      Where the fish wrapper was put.
#                 Default: ~/.config/fish/functions
#   NO_COLOR      Set to any non-empty value to suppress colour output.
#
# This script does NOT edit your shell rc files. It finds the lines that
# source the wrapper and prints them for you to remove, because silently
# rewriting ~/.bashrc is not a thing an uninstaller should do unasked.

set -eu

##############################################################################
# Configuration
##############################################################################

INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
SHELL_DIR="${SHELL_DIR:-${HOME}/.local/share/trail/shell}"
FISH_DIR="${FISH_DIR:-${HOME}/.config/fish/functions}"

PURGE=0
DRY_RUN=0

for arg in "$@"; do
    case "$arg" in
        --purge)   PURGE=1   ;;
        --dry-run) DRY_RUN=1 ;;
        -h|--help)
            sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            printf 'Unknown option: %s\n' "$arg" >&2
            printf 'Try: sh uninstall.sh --help\n' >&2
            exit 1
            ;;
    esac
done

##############################################################################
# Helpers
##############################################################################

_use_colour() {
    [ -z "${NO_COLOR:-}" ] && [ -t 1 ]
}

info()  { if _use_colour; then printf '\033[1;34m  info\033[0m  %s\n' "$*"; else printf '  info  %s\n' "$*"; fi; }
ok()    { if _use_colour; then printf '\033[1;32m    ok\033[0m  %s\n' "$*"; else printf '    ok  %s\n' "$*"; fi; }
warn()  { if _use_colour; then printf '\033[1;33m  warn\033[0m  %s\n' "$*"; else printf '  warn  %s\n' "$*"; fi; }
kept()  { if _use_colour; then printf '\033[1;36m  kept\033[0m  %s\n' "$*"; else printf '  kept  %s\n' "$*"; fi; }

# Removes a file, honouring --dry-run.  Absent files are not an error: a
# partial install is exactly the situation an uninstaller has to cope with.
remove_file() {
    target="$1"
    [ -e "$target" ] || [ -L "$target" ] || return 0

    if [ "$DRY_RUN" -eq 1 ]; then
        info "would remove  $target"
    else
        rm -f "$target"
        ok "removed  $target"
    fi
}

# Removes a directory tree, honouring --dry-run.
remove_dir() {
    target="$1"
    [ -d "$target" ] || return 0

    if [ "$DRY_RUN" -eq 1 ]; then
        info "would remove  $target/"
    else
        rm -rf "$target"
        ok "removed  $target/"
    fi
}

# Removes a directory only if it is empty, so that uninstalling does not
# delete a directory the user put other things in.
remove_dir_if_empty() {
    target="$1"
    [ -d "$target" ] || return 0
    [ -z "$(ls -A "$target" 2>/dev/null)" ] || return 0

    if [ "$DRY_RUN" -eq 1 ]; then
        info "would remove empty  $target/"
    else
        rmdir "$target" 2>/dev/null || true
    fi
}

##############################################################################
# Refuse to fight a package manager
##############################################################################

# Removing files out from under brew/pacman leaves the package manager
# believing trail is still installed, which breaks its next upgrade. Detect
# that case and hand the user the right command instead.
detect_package_manager() {
    resolved="$(command -v trail 2>/dev/null || true)"
    [ -n "$resolved" ] || return 0

    case "$resolved" in
        /opt/homebrew/*|/usr/local/Cellar/*|/home/linuxbrew/*)
            warn "trail at ${resolved} was installed by Homebrew."
            warn "Use:  brew uninstall trail && brew untap WeedonSctt/trail"
            exit 1
            ;;
        /usr/bin/*|/usr/local/bin/trail)
            if command -v pacman > /dev/null 2>&1 && pacman -Qo "$resolved" > /dev/null 2>&1; then
                warn "trail at ${resolved} is owned by a pacman package."
                warn "Use:  sudo pacman -Rns trail    (or: paru -Rns trail)"
                exit 1
            fi
            ;;
    esac

    case "$resolved" in
        "${HOME}/.cargo/bin/trail")
            warn "trail at ${resolved} was installed by cargo."
            warn "Use:  cargo uninstall trail"
            warn "Continuing anyway would leave cargo's registry out of step."
            exit 1
            ;;
    esac
}

##############################################################################
# Report rc-file lines rather than editing them
##############################################################################

report_rc_lines() {
    found=0
    for rc in "${HOME}/.bashrc" "${HOME}/.bash_profile" "${HOME}/.profile" "${HOME}/.zshrc"; do
        [ -f "$rc" ] || continue
        # Match any line sourcing a trail wrapper, wherever it was installed
        # from — the script path differs between install.sh, Homebrew and AUR.
        matches="$(grep -n 'trail\.\(bash\|zsh\)' "$rc" 2>/dev/null || true)"
        [ -n "$matches" ] || continue

        if [ "$found" -eq 0 ]; then
            echo ""
            warn "These lines still source a Trail wrapper. Remove them by hand,"
            warn "or every new shell will report a missing file:"
            echo ""
            found=1
        fi
        printf '    %s\n' "$rc"
        printf '%s\n' "$matches" | sed 's/^/      /'
    done
}

##############################################################################
# Main
##############################################################################

detect_package_manager

if [ "$DRY_RUN" -eq 1 ]; then
    info "Dry run — nothing will be removed."
fi

info "Removing Trail"

# The binary.
remove_file "${INSTALL_DIR}/trail"

# The wrappers. SHELL_DIR is removed as a tree because install.sh created it,
# but only after checking it is the wrapper directory and not something wider:
# on Linux the default SHELL_DIR sits INSIDE the data directory
# (~/.local/share/trail), so removing the parent would take the user's
# bookmarks with it.
case "$SHELL_DIR" in
    */shell) remove_dir "$SHELL_DIR" ;;
    *)
        remove_file "${SHELL_DIR}/trail.bash"
        remove_file "${SHELL_DIR}/trail.zsh"
        ;;
esac
remove_file "${FISH_DIR}/trail.fish"

##############################################################################
# Config and data — only with --purge
##############################################################################

# Resolved the same way the binary resolves them (the `directories` crate).
# `trail --paths` prints these, which is the authoritative answer if this
# script and the binary ever disagree.
case "$(uname -s)" in
    Darwin)
        CONFIG_DIR="${HOME}/Library/Application Support/trail"
        DATA_DIR="${HOME}/Library/Application Support/trail"
        ;;
    *)
        CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/trail"
        DATA_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}/trail"
        ;;
esac

if [ "$PURGE" -eq 1 ]; then
    warn "--purge: removing configuration and data"
    remove_dir "$CONFIG_DIR"
    remove_dir "$DATA_DIR"
else
    remove_dir_if_empty "$DATA_DIR"
    # `cmd && report` would abort the script under `set -e` whenever the test
    # is false, so these stay as full if-blocks.
    if [ -d "$CONFIG_DIR" ]; then
        kept "$CONFIG_DIR/  (config — remove with --purge)"
    fi
    if [ -d "$DATA_DIR" ]; then
        kept "$DATA_DIR/  (bookmarks, recent dirs — remove with --purge)"
    fi
fi

# The log is a debugging artefact in the temp directory, not state.
remove_file "${TMPDIR:-/tmp}/trail.log"

report_rc_lines

echo ""
if [ "$DRY_RUN" -eq 1 ]; then
    info "Dry run complete. Re-run without --dry-run to apply."
else
    info "Trail has been removed."
fi
echo ""
