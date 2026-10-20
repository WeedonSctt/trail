# The README demo, recorded with VHS (https://github.com/charmbracelet/vhs).
#
# Re-record after any release that changes what this shows:
#
#   sh docs/assets/demo-fixture.sh     # builds the sample project at ~/projects/aurora
#   cargo build --release
#   vhs docs/assets/demo.tape          # run from the repository root
#
# Needs pwsh, vhs and ttyd on PATH, and a GNU userland for the fixture script
# (Git Bash on Windows). The recording runs the freshly built
# target/release binary through the repository's own shell/trail.ps1 wrapper.

Output "docs/assets/demo.gif"

Set Shell "pwsh"
Set FontSize 16
Set Width 1400
Set Height 760
Set Padding 16
# Catppuccin Mocha, except that ANSI black is the background colour, as it is
# in most stock terminal palettes, and bright black is light enough to carry
# the default theme's black-on-dark_gray selection. In stock Catppuccin the two
# are nearly equal, and the default `background = "black"` draws a box behind
# every file name.
Set Theme { "name": "Trail demo", "background": "#1e1e2e", "foreground": "#cdd6f4", "cursor": "#f5e0dc", "selection": "#585b70", "black": "#1e1e2e", "red": "#f38ba8", "green": "#a6e3a1", "yellow": "#f9e2af", "blue": "#89b4fa", "magenta": "#f5c2e7", "cyan": "#94e2d5", "white": "#bac2de", "brightBlack": "#9399b2", "brightRed": "#f38ba8", "brightGreen": "#a6e3a1", "brightYellow": "#f9e2af", "brightBlue": "#89b4fa", "brightMagenta": "#f5c2e7", "brightCyan": "#94e2d5", "brightWhite": "#a6adc8" }
Set TypingSpeed 70ms

Hide
# Whatever terminal launched VHS leaks its variables into the recording, and
# those pick an image protocol the recorder cannot draw. Clear them so images
# fall back to Halfblocks, which it can.
Type `'TERM_PROGRAM','TERM_PROGRAM_VERSION','WEZTERM_EXECUTABLE','WEZTERM_PANE','WEZTERM_UNIX_SOCKET','KITTY_WINDOW_ID','ITERM_SESSION_ID','LC_TERMINAL','WT_SESSION' | ForEach-Object { Remove-Item "Env:$_" -ErrorAction SilentlyContinue }`
Enter
# --no-config so the recording shows the defaults a new user gets, not the
# recorder's own trail.toml.
Type `$env:PATH = "$PWD/target/release;$env:PATH"; . ./shell/trail.ps1; Rename-Item Function:trail trail-wrapper; function trail { trail-wrapper --no-config @args }; function prompt { "$(Split-Path -Leaf (Get-Location)) ❯ " }; Set-Location "$HOME/projects/aurora"; Clear-Host`
Enter
Sleep 1.5s
Show

Sleep 500ms
Type "trail"
Sleep 400ms
Enter
Sleep 1.8s

# The preview follows the selection.
Down@500ms 7
Sleep 1.2s

# An image previews inline.
Type "gg"
Sleep 400ms
Enter
Sleep 2.5s
Left
Sleep 600ms

# Into src/: syntax highlighting, and a preview that scrolls on its own.
Down 2
Sleep 500ms
Enter
Sleep 900ms
Down 1
Sleep 1.2s
Type@200ms "JJJJJ"
Sleep 1s
Left
Sleep 700ms

# Fuzzy-find a directory and jump into it.
Type "/"
Sleep 300ms
Type@220ms "doc"
Sleep 900ms
Enter
Sleep 1.2s

# Commands run in place.
Type ":"
Sleep 300ms
Type "mkdir drafts"
Sleep 500ms
Enter
Sleep 1.5s

# Quit, and the shell is where Trail was.
Type "q"
Sleep 1.2s
Type "ls -Name"
Sleep 300ms
Enter
Sleep 3.5s
