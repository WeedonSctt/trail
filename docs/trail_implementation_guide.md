# Trail — Implementation Guide

**Purpose of this document.** `trail.md` defines *what* Trail does (product spec). `trail-architecture.md` defines *how it's structured* (stack, process model, thread responsibilities). This document is the third layer: a concrete, buildable reference — file layout, data types, function signatures, and a phased plan — so an implementer can go from architecture to working code without re-deriving decisions. Every component named in `trail-architecture.md`'s tables is given a home here.

---

## 1. Confirmed stack (from architecture doc)

Rust, using: `ratatui` (UI), `crossterm` (terminal backend), `tokio` (async worker pool), `nucleo` (fuzzy filter), `syntect` or `tree-sitter-highlight` (syntax highlighting — see §9.3 for the pick), `gix` or `git2` (git — see §9.3), `notify` (fs watching), `image` + `ratatui-image` (image preview), `mlua` or `extism` (plugins — see §9.3), `serde` + `toml` (config), `clap` (CLI flags).

This guide assumes the Rust stack. The Go/`bubbletea` alternative from the architecture doc is not developed further here — if chosen, the module boundaries below still apply, only the crate names change.

---

## 2. Project layout

```
trail/
├── Cargo.toml
├── src/
│   ├── main.rs               # entry point: parse CLI, init terminal, run event loop, teardown
│   ├── cli.rs                 # clap definitions: --cwd-file, --config, positional start path
│   ├── app/
│   │   ├── mod.rs
│   │   ├── state.rs            # AppState, Entry, EntryKind
│   │   ├── mode.rs             # Mode enum (Navigation / Search / Command)
│   │   ├── history.rs          # NavigationHistory (back/forward stack)
│   │   └── tabs.rs             # TabState, multi-tab support (extension)
│   ├── ui/
│   │   ├── mod.rs               # render(state) -> draws all three panels
│   │   ├── nav_panel.rs
│   │   ├── preview_panel.rs
│   │   ├── status_bar.rs
│   │   └── theme.rs             # resolves TOML theme -> ratatui Style objects
│   ├── input/
│   │   ├── mod.rs               # dispatch(key, state) -> Option<Action>
│   │   ├── keymap.rs            # default + user-configured key bindings
│   │   └── command_parser.rs    # Command Mode grammar: history, completion, validation
│   ├── actions/
│   │   ├── mod.rs               # Action enum + apply(action, state)
│   │   ├── fs_ops.rs             # rename/move/duplicate/delete/create file|dir
│   │   ├── clipboard.rs          # copy absolute/relative path, filename
│   │   └── shell_exec.rs         # suspend/resume subprocess execution
│   ├── preview/
│   │   ├── mod.rs
│   │   ├── provider.rs           # PreviewProvider trait + registry
│   │   ├── directory.rs
│   │   ├── text.rs
│   │   ├── binary.rs
│   │   └── image.rs
│   ├── workers/
│   │   ├── mod.rs                # WorkerMsg enum, spawn/dispatch helpers, mpsc plumbing
│   │   ├── git.rs
│   │   ├── fswatch.rs
│   │   ├── highlight.rs
│   │   └── image_decode.rs
│   ├── config/
│   │   ├── mod.rs
│   │   ├── schema.rs             # serde structs mirroring the TOML shape
│   │   └── default.toml
│   ├── plugin/
│   │   ├── mod.rs
│   │   └── lua_api.rs            # v1 scripting surface (see §9.3)
│   └── session.rs                # writes --cwd-file on normal exit
├── shell/
│   ├── trail.bash
│   ├── trail.zsh
│   └── trail.fish
└── tests/
    ├── state_tests.rs
    ├── render_snapshot_tests.rs
    ├── command_parser_tests.rs
    └── fixtures/                 # sample dirs/files used by preview + filter tests
```

Every row of the architecture doc's "UI thread" and "Async worker pool" tables maps 1:1 to a module above; §4–§7 give each one concrete signatures.

---

## 3. Core data types (`app/state.rs`)

```rust
pub struct AppState {
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,          // directory-first sorted, current listing
    pub selected: usize,
    pub mode: Mode,
    pub history: NavigationHistory,
    pub filter: Option<FilterState>,  // Some(..) only while in Search Mode
    pub preview: PreviewSlot,
    pub git: Option<GitDirState>,     // populated asynchronously
    pub status: StatusBarState,
    pub tabs: Vec<TabState>,          // extension point; len==1 for v1
    pub active_tab: usize,
    pub dirty: bool,                  // set true on any state change, cleared after render
}

pub struct Entry {
    pub path: PathBuf,
    pub file_name: String,
    pub kind: EntryKind,              // Dir | File | Symlink
    pub is_hidden: bool,
    pub metadata: Option<std::fs::Metadata>,
    pub git_status: Option<GitFileStatus>, // filled in by the git worker, not blocking
}

pub enum EntryKind { Dir, File, Symlink }

pub enum Mode {
    Navigation,
    Search { query: String, matches: Vec<usize> }, // indices into entries
    Command { buffer: String, cursor: usize, history_index: Option<usize> },
}

// Tags every preview result with the selection it answers, so late-arriving
// worker results for a since-abandoned selection are discarded on merge —
// this is the "stale preview" guard called out in the architecture doc.
pub struct PreviewSlot {
    pub for_path: PathBuf,
    pub generation: u64,
    pub content: PreviewContent,
}
```

`state.dirty` and the `generation` counter are the two invariants the whole render/merge loop depends on — every mutation to `AppState` sets `dirty = true`; every selection change increments `generation`.

---

## 4. UI thread

### 4.1 Main loop (`main.rs`)

```rust
loop {
    let event = select! {
        k = terminal_events.recv() => Event::Key(k),
        w = worker_rx.recv()       => Event::Worker(w),
        f = fs_signal_rx.recv()    => Event::FsChanged(f),
    };

    match event {
        Event::Key(key)        => input::dispatch(key, &mut state, &action_ctx),
        Event::Worker(msg)     => workers::merge(msg, &mut state),  // drops stale generations
        Event::FsChanged(path) => actions::refresh_current_dir(&mut state),
    }

    if state.dirty {
        ui::render(&mut terminal, &state)?;
        state.dirty = false;
    }
}
```

Directory-first sort and fuzzy filtering happen synchronously here, per the architecture doc — no channel round trip for either.

### 4.2 Navigation panel (`ui/nav_panel.rs`)
Renders `state.entries` (or `state.filter.matches` when in Search Mode), directories before files, with hidden-file dimming, git badges from `state.git`, and the highlighted selection. Pure render function: `fn draw(frame, area, state)`.

### 4.3 Preview panel (`ui/preview_panel.rs`)
Dispatches on `Entry::kind` and file size to a `PreviewProvider` (see §6). Renders whatever is currently in `state.preview.content`, which may be a placeholder ("loading…") until a worker result merges in.

### 4.4 Status bar (`ui/status_bar.rs`)
Pure reflection of state — no logic beyond formatting: `cwd`, `mode` label, active filter string, git branch, `entries.len()`, and the Command Mode input line when active.

### 4.5 Input / mode handler (`input/mod.rs`)

```rust
pub fn dispatch(key: KeyEvent, state: &mut AppState, ctx: &ActionCtx) {
    match &mut state.mode {
        Mode::Navigation => keymap::navigation(key, state, ctx),
        Mode::Search { .. } => keymap::search(key, state),
        Mode::Command { .. } => command_parser::feed(key, state, ctx),
    }
}
```

Default keymap (single-key, matches spec's "frequently used operations are single-key"):

| Key | Action |
|---|---|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `l` / `Enter` | Enter directory / open file in editor |
| `h` / `Backspace` | Parent directory |
| `g g` / `G` | Jump to top / bottom |
| `/` | Enter Search Mode |
| `Esc` | Leave Search/Command Mode |
| `:` | Enter Command Mode |
| `y a` | Copy absolute path |
| `y r` | Copy relative path |
| `y n` | Copy filename |
| `o` | Open with OS default |
| `d d` | Delete (with confirmation) |
| `r` | Rename (opens Command Mode pre-filled) |
| `!` | Execute shell command (Command Mode) |
| `u` / `Ctrl-r` | Navigation history back / forward |
| `R` | Refresh current directory |

All bindings are overridable via `config/default.toml` → `[keymap]` (§8).

### 4.6 Command Mode parser (`input/command_parser.rs`)
Grammar covers create/rename/shell-exec/git/config, per the spec's list:

```
:mkdir <name>
:touch <name>
:rename <new-name>
:mv <dest>
:cp <dest>
:git <subcommand...>
:set <key> <value>
!<shell command>
```

Provides history (ring buffer, persisted across sessions to a small cache file), tab-completion (path completion for `mv`/`cp` targets, verb completion for the leading token), and validation (e.g. reject `:rename` with a `/` in the name, reject empty `:mkdir`).

### 4.7 Shell command execution (`actions/shell_exec.rs`)
Implements the suspend/resume sequence from the architecture doc exactly:

```rust
pub fn run_external(cmd: &mut std::process::Command, terminal: &mut Terminal) -> io::Result<()> {
    terminal::leave_alt_screen_and_raw_mode(terminal)?;
    let status = cmd.status()?;         // inherited stdio, cwd = state.cwd
    terminal::enter_alt_screen_and_raw_mode(terminal)?;
    terminal.clear()?;                  // subprocess may have overwritten the screen
    let _ = status;
    Ok(())
}
```

Used for both "Open in configured editor" and Command Mode's `!shell command`.

### 4.8 Session exit (`session.rs` + `shell/trail.*`)
On normal exit, write `state.cwd` to the path given by `--cwd-file`; on cancellation (e.g. `Esc`-driven quit or `Ctrl-c`), write nothing. The shell function wraps the binary and `cd`s into whatever was written:

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

Ship equivalents for zsh (identical, zsh sources bash-compatible functions) and fish (`function trail ... end` with `set -l`/`test -f`). Installation instructions belong in the README, not the binary.

---

## 5. Async worker pool (`workers/`)

### 5.1 Message envelope

```rust
pub enum WorkerMsg {
    Git(PathBuf, GitDirResult),
    FsChanged(PathBuf),                       // already debounced
    Preview(PathBuf, u64 /*generation*/, PreviewContent),
    ImageMeta(PathBuf, u64, ImageInfo),
}
```

Single enum, single `mpsc::Receiver<WorkerMsg>` drained once per UI tick, per the architecture doc's recommendation — this resolves the "one enum vs. per-domain channels" open question in favor of one enum, keeping the UI-thread `select!` simple; per-domain channels would only pay off if worker volume grows enough to need independent backpressure, which isn't the case at this scale.

### 5.2 Git worker (`workers/git.rs`)
`spawn_git_status(path: PathBuf, tx: Sender<WorkerMsg>)` — computes repo indicator, branch name, and (if enabled in config) per-file status; caches per directory; invalidated by `FsChanged` for that directory rather than recomputed every render.

### 5.3 Filesystem watcher (`workers/fswatch.rs`)
Wraps `notify`, watching `state.cwd` only (re-subscribed on directory change). Debounces bursts (e.g. a `git checkout` touching many files) into a single `WorkerMsg::FsChanged` after a quiet window — see §9.3 for the recommended window length.

### 5.4 Syntax highlighter (`workers/highlight.rs`)
`spawn_highlight(path: PathBuf, generation: u64, tx: Sender<WorkerMsg>)` — only invoked for text files over the size threshold (small files highlight synchronously in `preview/text.rs`, per the architecture doc's split).

### 5.5 Image worker (`workers/image_decode.rs`)
Decodes dimensions/metadata always; decodes pixel data only if the detected terminal protocol supports inline images (§9.3).

### 5.6 Dispatch rules

```rust
// on directory_change or fs_event:
spawn(git::status(cwd.clone(), tx.clone()));
watch::resubscribe(&cwd);

// on selection_change(entry):
match entry.kind {
    EntryKind::Dir => preview::directory::build_sync(entry), // cheap: read_dir + counts
    EntryKind::File if is_text(entry) && size(entry) > TEXT_SYNC_THRESHOLD =>
        spawn(highlight::run(entry.path.clone(), state.preview.generation, tx.clone())),
    EntryKind::File if is_text(entry) => preview::text::build_sync(entry),
    EntryKind::File if is_binary(entry) => spawn(binary::read_metadata(entry.path.clone(), tx.clone())),
    EntryKind::File if is_image(entry) => spawn(image_decode::run(entry.path.clone(), state.preview.generation, tx.clone())),
    _ => {}
}
```

`TEXT_SYNC_THRESHOLD` is a config value (default: a few hundred KB) — below it, highlighting happens inline to avoid a channel round trip for the common case.

---

## 6. Preview providers (`preview/`)

```rust
pub trait PreviewProvider: Send + Sync {
    fn can_handle(&self, entry: &Entry) -> bool;
    fn preview(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome;
}

pub enum PreviewOutcome {
    Ready(PreviewContent),                    // synchronous path
    Deferred,                                   // a worker task was spawned; UI shows a placeholder
}

pub struct PreviewRegistry {
    providers: Vec<Box<dyn PreviewProvider>>,  // checked in order, first match wins
}
```

Built-in providers: `directory.rs`, `text.rs`, `binary.rs`, `image.rs`. New types (PDF, archive, etc. — see the spec's extensibility list) are added by implementing this trait and registering an instance at startup; the main loop, worker dispatch, and render code do not change. This is the mechanism that satisfies "additional preview providers may be added without changing the navigation workflow."

---

## 7. Config (`config/`)

TOML, loaded once at startup, resolved by the input handler and theme module.

```toml
[general]
editor = "$EDITOR"
text_sync_threshold_kb = 256
git_status_enabled = true
fs_watch_debounce_ms = 200

[theme]
directory_fg = "blue"
selection_bg = "#3a3a3a"
git_modified_fg = "yellow"
git_added_fg = "green"

[keymap]
"j" = "move_down"
"k" = "move_up"
"l" = "enter_or_open"
"/" = "search_mode"
":" = "command_mode"

[plugins]
enabled = ["bookmarks"]
```

Schema mirrored in `config/schema.rs` via `serde::Deserialize`; unknown keys are rejected in strict mode with a helpful error pointing at the offending line — this is the "validation" the architecture doc lists for Command Mode's config subcommand as well (`:set` writes through the same schema).

---

## 8. Extensibility (spec list → mechanism, restated with implementation hooks)

| Spec item | Mechanism | Where |
|---|---|---|
| Custom preview providers (PDF, archive) | New `PreviewProvider` impl, registered at startup | `preview/*.rs` + `preview/mod.rs::register_defaults()` |
| Plugin system, user-defined commands | Lua (`mlua`) scripting surface with `on_select`, `on_enter_dir`, `register_action` hooks | `plugin/lua_api.rs` |
| Configurable themes, custom keybindings | TOML `[theme]` / `[keymap]` tables | `config/schema.rs`, `ui/theme.rs`, `input/keymap.rs` |
| Bookmarks, recent directories | Persisted list (TOML or small sled/JSON store) keyed by path, exposed as a Command Mode verb (`:bookmark`, `:jump <name>`) | new `bookmarks.rs`, likely under `app/` |
| Multiple tabs, split navigation | `Vec<TabState>` already in `AppState`; `TabState` duplicates `{cwd, entries, selected, history}` | `app/tabs.rs` |

No architectural change is required for any of these — this is the payoff of the trait/registry + flat-config design.

---

## 9. Cross-cutting concerns

### 9.1 Stale-result guard
Every `WorkerMsg::Preview`/`ImageMeta` carries the `generation` it was requested under. `workers::merge` compares against `state.preview.generation` and drops the message if they don't match — this is what prevents a fast arrow-key scroll from flashing a late preview for an entry the user has already moved past.

### 9.2 Testing strategy
- **Unit tests** (`state_tests.rs`): mode transitions, selection movement at list boundaries, filter match/reorder correctness, generation-guard drop logic.
- **Snapshot tests** (`render_snapshot_tests.rs`): render `AppState` fixtures through `ratatui::TestBackend`, assert against stored terminal-buffer snapshots for each panel and mode.
- **Command parser tests**: grammar coverage (valid/invalid `:mkdir`, `:rename`, `!`-prefixed shell), completion and history behavior.
- **Fixture-driven preview tests**: a `tests/fixtures/` tree with a text file, a binary file, a small image, and a nested git repo, exercised through each `PreviewProvider`.
- **Manual matrix**: terminal emulators × image protocols (Kitty, iTerm2, Sixel, none) before each release, since this can't be fully automated.

### 9.3 Resolving the architecture doc's open questions
- **Message enum shape** → single `WorkerMsg` enum (§5.1). Revisit only if worker volume grows enough to need independent backpressure per domain.
- **Fs-watch debounce window** → default 200ms, exposed as `fs_watch_debounce_ms` in config; long enough to coalesce a `git checkout`, short enough not to feel laggy.
- **Plugin API: Lua vs WASM** → Lua (`mlua`) for v1 — synchronous, no sandboxing setup cost, matches the scale of "user-defined commands." Revisit WASM (`extism`) as a v2 opt-in if untrusted third-party plugins become a real use case (WASM's sandboxing earns its complexity there, not for personal dotfiles-style scripts).
- **Git backend: `gix` vs `git2`** → start with `gix` (pure Rust, no libgit2 build/link dependency, simpler cross-compilation for the packaging step in §10); fall back to `git2` only if a needed status/branch feature is missing in `gix` at implementation time.
- **Syntax highlighting: `syntect` vs `tree-sitter-highlight`** → `syntect` for v1 (mature, wide language/theme support out of the box); `tree-sitter-highlight` is worth revisiting if incremental re-highlighting on file edits becomes a requirement, which it isn't for a read-only preview.
- **Terminal image protocol detection** → runtime probe via `ratatui-image`'s picker: query Kitty graphics protocol support, check `TERM_PROGRAM`/`LC_TERMINAL` for iTerm2, probe terminfo for Sixel, fall back to metadata-only preview (dimensions/size, no pixels) if none match.

---

## 10. Phased build plan

| Phase | Scope | Exit criteria |
|---|---|---|
| 0 — Scaffolding | Cargo project, `clap` CLI, raw-mode terminal init/teardown, empty three-panel layout | `trail` launches into an empty ratatui frame and quits cleanly on `q` |
| 1 — Core navigation | `AppState`, `Entry` listing, directory-first sort, Navigation Mode movement, enter/parent dir, synchronous directory + text preview | Can browse a real filesystem tree with live preview, no async yet |
| 2 — Search Mode | `nucleo` integration, incremental filter, reorder-on-type | `/` filters current dir live, `Esc` restores full listing |
| 3 — Command Mode | Parser grammar, `:mkdir`/`:touch`/`:rename`, history + completion | Parameterized actions work end-to-end with validation errors surfaced in the status bar |
| 4 — Async worker pool | `WorkerMsg` plumbing, git worker, fs watcher + debounce, generation-guard | Git branch/status appear without blocking navigation; external file changes trigger auto-refresh |
| 5 — Rich previews | Syntax highlighting (large files via worker), binary metadata, image metadata + protocol-gated pixel preview | All four `PreviewProvider`s implemented and registered |
| 6 — Shell integration | `--cwd-file`, suspend/resume for editor and `!shell`, bash/zsh/fish wrapper functions | Exiting `trail` normally leaves the parent shell in the last-browsed directory; editor/shell exec doesn't corrupt the terminal on return |
| 7 — Config & keymap | TOML schema, theme resolution, keymap overrides, `:set` wired to the same schema | User can remap keys and recolor the UI without a rebuild |
| 8 — Extensibility | Lua plugin hooks, bookmarks, recent-directories, tabs | At least one working example plugin; bookmarks persist across sessions |
| 9 — Packaging | Cross-compile via `cross`/`cargo-zigbuild`, GitHub Releases CI on tag push, Homebrew/AUR/Scoop formulas, `cargo install` verified | Fresh install on Linux/macOS/Windows works from each distribution channel |

Phases 0–3 are a usable MVP on their own (synchronous-only, no git/images/plugins); phases 4+ layer in exactly the pieces the architecture doc marks as worker-pool/extension responsibilities, so the MVP never needs to be re-architected to grow into the full spec.

