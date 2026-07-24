# Bash/POSIX-sh wrapper for the `trail` terminal file manager.
#
# Source this file in your ~/.bashrc (or ~/.bash_profile) to enable the
# "cd-on-exit" behaviour: when you quit Trail normally (`q`), your current
# shell session changes to the directory Trail was browsing.
#
# Installation:
#   echo 'source /path/to/trail.bash' >> ~/.bashrc
#   source ~/.bashrc
#
# Usage:
#   trail [<start-path>]      # launches Trail; on normal exit, `cd`s to the
#                             # last-visited directory.
#
# How it works:
#   The wrapper creates a temporary file and passes its path to the Trail
#   binary via --cwd-file.  On a normal exit Trail writes its current
#   directory to that file; the wrapper reads it, removes it, then `cd`s
#   into the directory.  On cancellation (Ctrl-c / Esc quit) Trail writes
#   nothing, so the shell stays where it was.

trail() {
    local tmp
    tmp="$(mktemp)" || return 1
    command trail --cwd-file "$tmp" "$@"
    local exit_code=$?
    if [ -f "$tmp" ]; then
        local dir
        dir="$(cat "$tmp")"
        rm -f "$tmp"
        [ -d "$dir" ] && cd -- "$dir"
    else
        rm -f "$tmp"
    fi
    return $exit_code
}
