# Trail — Development Environment Setup Guide

Step-by-step setup to go from an empty machine to `cargo run` working, based on the stack defined in `trail-architecture.md`.

---

## 1. Install the Rust toolchain

Install via `rustup` (not your OS package manager — you need easy toolchain/component switching):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Then pin and verify:

```bash
rustup default stable
rustup component add clippy rustfmt
rustc --version   # 1.75+ recommended (async traits, let-else, etc.)
```

On Windows, use `rustup-init.exe` from rust-lang.org and install the "Desktop development with C++" workload in Visual Studio Build Tools first (needed for the MSVC linker).

---

## 2. System-level build dependencies

Two crates in the stack (`git2`, `mlua`) normally link against system C libraries (`libgit2`, `liblua`). **Avoid that entirely** by using their vendored/bundled feature flags in `Cargo.toml` (see step 5) — this means step 2 only needs a basic C toolchain to compile the vendored C sources, not the libraries themselves pre-installed.

**Linux (Debian/Ubuntu):**
```bash
sudo apt update
sudo apt install build-essential pkg-config cmake
```

**Linux (Fedora):**
```bash
sudo dnf groupinstall "Development Tools"
sudo dnf install cmake pkgconf-pkg-config
```

**macOS:**
```bash
xcode-select --install
brew install cmake pkg-config
```

**Windows:**
Covered by the Visual Studio Build Tools C++ workload installed in step 1. No separate `cmake`/`pkg-config` needed if you stick to vendored features.

---

## 3. Terminal emulator

Trail's image preview and true-color rendering depend on the terminal you develop in. For testing all preview code paths, use one that supports true color and, ideally, one of the inline image protocols:

| Terminal | True color | Image protocol | OS |
|---|---|---|---|
| **Kitty** | Yes | Kitty graphics protocol | Linux/macOS |
| **WezTerm** | Yes | Kitty protocol + Sixel | Linux/macOS/Windows |
| **iTerm2** | Yes | iTerm2 inline images | macOS |
| **Windows Terminal** | Yes | None (fallback path only) | Windows |

Recommendation: install **WezTerm** for development — it's cross-platform and covers the widest protocol surface, so you can exercise the fallback path (no protocol) and at least one graphics protocol without switching terminals.

---

## 4. Editor setup

Any editor works, but for `rust-analyzer` support:

- **VS Code**: install the `rust-analyzer` extension (not the older deprecated "Rust" extension) and `CodeLLDB` for debugging.
- **Neovim**: `rust-analyzer` via `mason.nvim` or your LSP client of choice.
- **JetBrains**: RustRover or the Rust plugin for IntelliJ.

Add a `rust-toolchain.toml` at the project root once the repo exists, so every contributor's editor and `cargo` use the same pinned version automatically:

```toml
[toolchain]
channel = "1.75.0"
components = ["clippy", "rustfmt"]
```

---

## 5. Scaffold the project

```bash
cargo new trail --bin
cd trail
```

Replace `Cargo.toml` dependencies with a starting set, using vendored features to sidestep step 2's system libraries where possible:

```toml
[package]
name = "trail"
version = "0.1.0"
edition = "2021"

[dependencies]
ratatui = "0.28"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
nucleo = "0.5"
syntect = { version = "5", default-features = false, features = ["default-fancy"] } # avoids libonig C dependency
git2 = { version = "0.19", features = ["vendored-libgit2"] }                        # bundles libgit2, no system install needed
notify = "6"
image = "0.25"
ratatui-image = "1"
mlua = { version = "0.9", features = ["lua54", "vendored"] }                        # bundles Lua, no system install needed
serde = { version = "1", features = ["derive"] }
toml = "0.8"
clap = { version = "4", features = ["derive"] }
directories = "5"

[profile.dev]
opt-level = 1        # ratatui/crossterm feel noticeably laggy at opt-level 0
```

Confirm it fetches and compiles cleanly (this alone validates steps 1–2):

```bash
cargo build
```

---

## 6. OS-level configuration

**Linux only — raise the inotify watch limit.** The `notify` crate uses inotify, whose default per-user watch limit (usually 8192) is easy to exceed once Trail is watching several directories across tabs/sessions during development:

```bash
echo fs.inotify.max_user_watches=524288 | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

**macOS/Windows**: no equivalent limit to raise; FSEvents and `ReadDirectoryChangesW` don't have this ceiling.

---

## 7. Git test fixtures

Since git status is a core preview feature, create a small set of repos in a scratch directory to exercise every state Trail needs to render:

```bash
mkdir -p ~/dev/trail-fixtures && cd ~/dev/trail-fixtures

mkdir clean-repo && cd clean-repo && git init -q && echo "content" > a.txt && git add . && git commit -qm "init" && cd ..
mkdir dirty-repo && cd dirty-repo && git init -q && echo "content" > a.txt && git add . && git commit -qm "init" && echo "changed" >> a.txt && cd ..
mkdir staged-repo && cd staged-repo && git init -q && echo "content" > a.txt && git add . && cd ..
mkdir untracked-repo && cd untracked-repo && git init -q && echo "content" > new.txt && cd ..
mkdir not-a-repo && cd not-a-repo && cd ..
```

Point Trail at `~/dev/trail-fixtures` during early development so the navigation panel's git indicators and the async git worker have real cases to hit (clean, dirty, staged, untracked, no-repo).

---

## 8. Config directory scaffold

Use the `directories` crate's platform-correct config path (`~/.config/trail/` on Linux, `~/Library/Application Support/trail/` on macOS, `%APPDATA%\trail\` on Windows) and seed it with a starter file so Command Mode / theming code has something to load from day one:

```bash
mkdir -p ~/.config/trail
cat > ~/.config/trail/config.toml << 'EOF'
[keybindings]
# left empty for now — resolved by the mode/input handler once it exists

[theme]
# left empty for now

[preview]
large_file_threshold_kb = 512
EOF
```

---

## 9. Dev tooling

Install a few cargo subcommands that make the UI-thread/async-worker split easier to iterate on:

```bash
cargo install cargo-watch     # re-run `cargo run` on file save
cargo install cargo-nextest   # faster, clearer test output than `cargo test`
```

Typical inner loop:

```bash
cargo watch -x 'run -- ~/dev/trail-fixtures'
```

Add a pre-commit check (manual or via a git hook) so formatting/lint issues never accumulate:

```bash
cargo fmt --check && cargo clippy -- -D warnings
```

---

## 10. Verify the environment end-to-end

Replace `src/main.rs` with the smallest possible ratatui program and confirm the whole toolchain — terminal, crossterm, ratatui, tokio — works together before writing any real logic:

```rust
use crossterm::{terminal, execute};
use ratatui::{Terminal, backend::CrosstermBackend, widgets::{Block, Borders}};
use std::io::stdout;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    execute!(stdout(), terminal::EnterAlternateScreen)?;
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;

    term.draw(|f| {
        f.render_widget(Block::default().title("Trail — env check OK").borders(Borders::ALL), f.area());
    })?;

    std::thread::sleep(std::time::Duration::from_secs(2));

    execute!(stdout(), terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    Ok(())
}
```

```bash
cargo run
```

If a bordered box titled "Trail — env check OK" appears for two seconds and the terminal returns cleanly to your shell prompt afterward, every layer (Rust toolchain, C build tools for vendored deps, terminal, ratatui/crossterm, tokio) is confirmed working, and you're ready to start on the state manager and main loop.

---

## Summary checklist

- [ ] `rustup` installed, `stable` toolchain, `clippy` + `rustfmt` components
- [ ] C build toolchain installed (`build-essential`/Xcode CLT/MSVC + `cmake`, `pkg-config`)
- [ ] True-color terminal installed (WezTerm recommended) for preview testing
- [ ] Editor configured with `rust-analyzer`
- [ ] `cargo new trail`, `Cargo.toml` populated with vendored-feature dependencies, `cargo build` succeeds
- [ ] (Linux) `fs.inotify.max_user_watches` raised
- [ ] Git fixture repos created under a scratch directory
- [ ] Config scaffold seeded at the OS-appropriate path
- [ ] `cargo-watch`, `cargo-nextest` installed
- [ ] Smoke-test `main.rs` runs and renders correctly
