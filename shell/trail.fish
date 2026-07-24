# Fish wrapper for the `trail` terminal file manager.
#
# Source this file (or place it in ~/.config/fish/functions/trail.fish) to
# enable the "cd-on-exit" behaviour: when you quit Trail normally (`q`), your
# current shell session changes to the directory Trail was browsing.
#
# Installation (recommended — fish autoloads functions from this directory):
#   cp trail.fish ~/.config/fish/functions/trail.fish
#
# Or source manually in config.fish:
#   source /path/to/trail.fish
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

function trail --description 'Terminal file manager with cd-on-exit'
    set -l tmp (mktemp)
    or return 1
    command trail --cwd-file $tmp $argv
    set -l exit_code $status
    if test -f $tmp
        set -l dir (cat $tmp)
        rm -f $tmp
        if test -d $dir
            cd -- $dir
        end
    else
        rm -f $tmp
    end
    return $exit_code
end
