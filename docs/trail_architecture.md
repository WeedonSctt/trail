# Trail — Architecture & Tech Stack

This document translates `trail.md` (the product spec) into a concrete tech stack, process architecture, and program logic, so it can be used as the basis for an implementation plan.

---

## 1. Tech stack

**Language: Rust.** Same lineage as `yazi` and `broot`. Rationale: Trail replaces `cd`/`ls` in a hot path, so startup latency matters; navigation must never stutter, so no GC pauses; filesystem mutation (rename/move/delete) benefits from Rust's safety guarantees.

| Concern | Crate | Purpose |
|---|---|---|
| Terminal UI | `ratatui` | Widget rendering, layout, the three-panel interface |
| Terminal backend | `crossterm` | Raw mode, alternate screen, cross-platform input events |
| Async runtime | `tokio` | Runs the background worker pool without blocking the UI |
| Fuzzy filtering | `nucleo` | Incremental fuzzy filter in Search Mode |
| Syntax highlighting | `syntect` or `tree-sitter-highlight` | Text file preview |
| Git status/branch | `gix` (pure Rust) or `git2` (libgit2 bindings) | Repo indicators, branch name, optional status |
| Filesystem watching | `notify` | "Automatic refresh after filesystem changes" |
| Image handling | `image` + `ratatui-image` | Metadata/dimensions always; pixel preview if terminal supports Kitty/iTerm2/Sixel |
| Plugin scripting | `mlua` (Lua) or `extism` (WASM) | User-defined commands, custom preview providers |
| Config | `serde` + `toml` | Keybindings, theme, extension points |
| CLI parsing | `clap` | Initial invocation flags (e.g. `--cwd-file`) |

**Alternative stack:** Go + `bubbletea`/`lipgloss`. Viable if the team prefers goroutines over `tokio` tasks. Slightly weaker syntax-highlighting and git ecosystem, but fast enough for this workload.

---

## 2. Process architecture

Trail runs as a **single OS process** split into two concurrency domains:

- **UI thread** — owns the terminal exclusively (ratatui/crossterm). Nothing else is allowed to write to it. Must never block.
- **Async worker pool** (tokio tasks) — does anything that could be slow: git status, filesystem watching, syntax highlighting, image decoding. Reports results back to the UI thread over `mpsc` channels.

This split exists because the spec requires the interface to stay responsive ("Large files may display only an initial portion... to maintain responsiveness", "Automatic refresh after filesystem changes", optional git status) while also doing work that is inherently variable-latency (git, disk I/O, image decode). Mixing the two on one thread would make navigation stutter every time a git status or a large file preview runs.

---

## 3. UI thread

### Responsibilities
Everything in the spec's "Interface," "Navigation," "Filtering," and "Modes" sections lives here.

| Component | Responsibility |
|---|---|
| State manager | Current directory, selection, mode, navigation history stack, tabs |
| Navigation panel | Reads directory entries, sorts directories-first, renders the list, merges in git badges once workers report them |
| Preview panel | Dispatches by entry type to a `PreviewProvider` trait implementation |
| Status bar | Pure reflection of current state — path, mode, filter, branch, entry count |
| Mode/input handler | Routes keystrokes differently depending on Navigation / Search / Command mode |
| Command mode parser | History, completion, validation for parameterized actions (rename, create, shell exec) |

### Program logic — main loop

```
loop:
    event = poll_next(terminal_input | worker_channel | fs_watch_signal)

    match event:
        KeyPress          -> mode_handler.dispatch(key, state)   # may enter/exit modes, move selection, trigger action
        WorkerResult(msg) -> state.merge(msg)                     # git status, highlighted text, image ready, etc.
        FsChangeDebounced -> state.refresh_current_dir()

    if state.dirty:
        render(nav_panel, preview_panel, status_bar)
        state.dirty = false
```

Key properties:
- **Directory-first sort** and **incremental fuzzy filter** happen synchronously in this thread — they're fast enough not to need offloading.
- **Selection change → preview update** is immediate for small/known-safe content (text under a size threshold, directory listing) and asynchronous (dispatched to the worker pool) for anything that could be slow (git-aware directory metadata, large text files, images).
- **The `PreviewProvider` trait** is what lets "additional preview providers be added without changing the navigation workflow" — a new type (PDF, archive) is a new trait implementation registered at startup, not a change to the core loop.

### Shell command execution (owned by UI thread, but leaves the terminal)

"Once execution finishes, the interface is restored and the current navigation state remains intact" implies a suspend/resume sequence:

```
1. Leave alternate screen, restore cooked terminal mode
2. Spawn subprocess (editor or shell command) with inherited stdio, relative to current dir
3. Wait for subprocess to exit
4. Re-enter raw mode + alternate screen
5. Force a full redraw (subprocess may have overwritten the terminal)
```

Same mechanism serves "Open in configured editor" and Command Mode's "Execute shell command."

### Session exit (shell integration)

A subprocess cannot change its parent shell's working directory. Trail's "shell continues in the directory currently displayed" behavior requires a shell-side wrapper function:

```bash
trail() {
  local tmp; tmp="$(mktemp)"
  command trail --cwd-file "$tmp" "$@"
  if [ -f "$tmp" ]; then
    local dir; dir="$(cat "$tmp")"; rm -f "$tmp"
    [ -d "$dir" ] && cd -- "$dir"
  fi
}
```

On normal exit, Trail writes its current directory to `--cwd-file` before terminating. On cancellation, it writes nothing, so the shell falls back to its original directory — matching the spec's distinction between the two exit paths.

---

## 4. Async worker pool

### Responsibilities
Everything that is optional, variable-latency, or explicitly deferred in the spec: "Optional Git status indicators," "Additional metadata," large-file previews, "Automatic refresh after filesystem changes."

| Component | Responsibility |
|---|---|
| Git status worker | Computes repo indicator, branch, optional per-file status; cached, invalidated on fs events |
| Filesystem watcher | Watches the current directory via `notify`; debounces bursts of events (e.g. a `git checkout`) into a single refresh signal |
| Syntax highlighter | Highlights text file previews off-thread for large files |
| Image worker | Decodes metadata/dimensions, and pixel data if the terminal protocol supports inline images |

### Program logic

```
on directory_change or fs_event:
    spawn_task: git_status(path)        -> send WorkerResult::Git(...)
    spawn_task: watch(path)             -> on burst, send FsChangeDebounced (after debounce window)

on selection_change(entry):
    match entry.kind:
        Directory -> synchronous (cheap: read_dir + counts)
        TextFile  -> if size > threshold: spawn_task: highlight(path) -> send WorkerResult::Preview(...)
                     else: synchronous
        Binary    -> spawn_task: read_metadata(path) -> send WorkerResult::Preview(...)
        Image     -> spawn_task: decode_image(path)  -> send WorkerResult::Preview(...)
```

Key properties:
- Every task communicates back via a single `mpsc` channel drained once per UI tick — no worker ever touches ratatui/crossterm state directly.
- Results are tagged with the path/selection they correspond to, so a fast navigation past several entries doesn't render a stale preview arriving late from an earlier selection.
- Git and fs-watch state is cached per directory and invalidated rather than recomputed on every render.

---

## 5. Extensibility (mapped to the spec's list)

| Spec item | Mechanism |
|---|---|
| Custom preview providers (archive, PDF, etc.) | New `PreviewProvider` trait implementation, registered at startup — either built-in or via plugin |
| Plugin system, user-defined commands | Lua (`mlua`) or WASM (`extism`) scripting API with hooks (`on_select`, `on_enter_dir`, custom actions) |
| Configurable themes, custom key bindings | TOML config loaded once at startup, resolved by the mode/input handler |
| Bookmarks, recent directories, tabs, split navigation | Additional fields on the existing state manager — no architectural change required |

---

## 6. Packaging & distribution

- Build via Cargo; cross-compile for Linux/macOS/Windows with `cross` or `cargo-zigbuild`.
- Publish binaries via GitHub Releases (CI-built on tag push).
- Distribute through Homebrew, AUR, Scoop, and `cargo install`.

---

## 7. Open questions for the implementation plan

- Exact channel message enum shape between UI thread and workers (one `WorkerResult` enum vs. per-domain channels).
- Debounce window length for filesystem watch events.
- Whether the plugin API is Lua (simpler, synchronous) or WASM (sandboxed, more setup cost) for v1.
- Terminal image protocol detection strategy (Kitty vs iTerm2 vs Sixel vs fallback).
