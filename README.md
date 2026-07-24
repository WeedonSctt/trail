# Trail

A terminal-first workspace for navigating, inspecting and acting on the filesystem without leaving the shell.

## Shell Integration

Trail's "cd-on-exit" behaviour — where your shell changes to the last directory Trail was browsing after you quit — requires a shell wrapper function. The wrapper is what invokes the `trail` binary; the binary on its own cannot change the parent shell's working directory.

### Bash

Add to your `~/.bashrc`:

```bash
source /path/to/trail/shell/trail.bash
```

Then reload:

```bash
source ~/.bashrc
```

### Zsh

Add to your `~/.zshrc`:

```zsh
source /path/to/trail/shell/trail.zsh
```

Then reload:

```zsh
source ~/.zshrc
```

### Fish

Copy to fish's functions directory (fish autoloads it automatically):

```fish
cp /path/to/trail/shell/trail.fish ~/.config/fish/functions/trail.fish
```

Or source it manually in `~/.config/fish/config.fish`:

```fish
source /path/to/trail/shell/trail.fish
```

### How it works

The shell wrapper creates a temporary file and passes it to the Trail binary via `--cwd-file`. On a **normal exit** (`q`), Trail writes its current directory to the file and the wrapper calls `cd` on it. On **cancellation** (`Ctrl-c` or `Esc`-quit), Trail writes nothing, so the shell stays in its original directory.

## Usage

```
trail [<start-path>]
```

| Key | Action |
|---|---|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `l` / `Enter` | Enter directory / open file in `$EDITOR` |
| `h` / `Backspace` | Go to parent directory |
| `gg` | Jump to top |
| `G` | Jump to bottom |
| `u` | Navigate back |
| `Ctrl-r` | Navigate forward |
| `/` | Enter Search Mode |
| `:` | Enter Command Mode |
| `q` | Quit (cd-on-exit if wrapper is sourced) |
| `Ctrl-c` | Quit without changing directory |
