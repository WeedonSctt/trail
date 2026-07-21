# Trail — Coding Standard

**Scope.** This document governs *how* code is written for every phase in
`trail-implementation-plan.md` (Phases 0–9). The product spec (`trail.md`),
architecture doc, and implementation guide decide *what* gets built and *how
it's structured*; this document is phase-agnostic and applies identically to
Phase 0 scaffolding and Phase 9 packaging alike. Where a rule below conflicts
with something written for convenience in an earlier planning doc, this
document wins for anything touching code quality, and the plan should be
updated to match rather than the other way around.

It lives at the root of the `trail/` repository, alongside `Cargo.toml`.

---

## 1. Formatting

- `rustfmt` is **mandatory**, using the project's `rustfmt.toml` (default
  settings until a reason to deviate is documented there).
- Run `cargo fmt` before every commit. CI runs `cargo fmt --check` as a hard
  gate — a PR that fails it does not get reviewed until fixed.
- `#[rustfmt::skip]` is allowed only with an inline comment explaining why
  (e.g. a hand-aligned table of key bindings that's more readable unformatted).

## 2. Linting

- `cargo clippy --all-targets --all-features -- -D warnings` must pass clean.
  No exceptions merge with outstanding warnings.
- Any suppressed lint (`#[allow(clippy::...)]`) is scoped as narrowly as
  possible — one expression or function, never a whole module or crate — and
  carries a same-line or immediately-preceding comment: `// clippy: <lint> —
  <reason>`.

## 3. Documentation

- Every public item — `pub fn`, `pub struct`, `pub enum`, `pub trait`, `pub
  mod` — has a `///` doc comment. State what it does and any non-obvious
  contract (panics, errors, invariants); skip restating the signature.
- Every module (`mod.rs`, or a file acting as one) opens with a `//!` summary
  matching its responsibility as listed in `trail-architecture.md`'s tables,
  so the doc and the architecture stay traceable to each other.
- If a doc comment includes a usage example, it must be a real doctest that
  passes under `cargo test --doc`.
- `cargo doc --no-deps` must build without warnings before a phase is
  considered closed.

## 4. Error Handling

- **No `unwrap()` or `expect()` outside `#[cfg(test)]` code or the `tests/`
  directory.** Production code paths return `Result` and propagate with `?`.
  - The one narrow exception: a top-level `.expect(...)` in `main.rs` for a
    genuinely unrecoverable startup failure (e.g. terminal init), and only
    once the Phase 0 panic hook (§6) is already installed so the terminal
    isn't left corrupted. Document the exception inline.
- Prefer typed errors (`thiserror`) in library-shaped modules (`app`,
  `preview`, `workers`, `actions`, `config`). `anyhow`/`Box<dyn Error>` is
  acceptable only at the outermost boundary in `main.rs`.
- Never discard an error silently (`let _ = fallible_call();`) without a
  comment explaining why the failure is inconsequential, and log it at
  `debug` level at minimum (see §11).

## 5. Panics

- No `panic!`/`unreachable!()` as control flow in library code.
  `unreachable!()` is allowed only where the type system genuinely rules out
  the branch, with a comment saying so.
- The panic hook that restores cooked terminal mode (a Phase 0 deliverable)
  must be installed before any other terminal state is touched, and must stay
  intact — don't let a later refactor drop it.

## 6. Unsafe Code

- `#![forbid(unsafe_code)]` at the crate root by default.
- Exceptions (most likely at the Lua FFI boundary in Phase 8) are isolated to
  their own small module, minimized in scope, and each `unsafe` block carries
  a `// SAFETY: ...` comment justifying it. A crate-wide blanket `allow` is
  not acceptable.

## 7. Testing

- New functionality gets tests **where practical**: unit tests colocated via
  `#[cfg(test)] mod tests` for pure logic (state transitions, sorting,
  parsing, the generation-guard), plus the dedicated suites in `tests/` for
  cross-cutting behavior (`state_tests.rs`, `render_snapshot_tests.rs`,
  `command_parser_tests.rs`, fixture-driven preview tests).
- "Where practical" is not a blanket excuse — if a case is skipped (e.g. real
  terminal rendering, OS-level file dialogs, the manual image-protocol
  matrix), say so explicitly in the PR description rather than leaving it
  unaddressed.
- Tests are deterministic: no dependence on wall-clock time, network access,
  or ambient machine state. Use `tests/fixtures/` and injected paths/clocks
  instead.
- A failing test blocks merge. There is no "known flaky, skip for now"
  without an accompanying tracked follow-up.

## 8. Compilation & Build Health

- **Every phase must leave the project compiling** (`cargo build`) and
  passing its full test suite at the point the phase is considered done —
  this is a hard exit gate, not a nice-to-have.
- Prefer commits small enough that each one builds; where that's not
  practical, the requirement still holds at every PR and phase boundary.
- Stubs are fine *between* phases when the phased plan says so explicitly
  (e.g. the editor-open action stubbed as a no-op until Phase 6, or a
  `PreviewOutcome::Deferred` placeholder before Phase 5's real worker lands).
  Mark them with `// TODO(phase-N): ...` referencing the plan, and remove the
  marker in the phase that's supposed to resolve it — a stub outliving its
  assigned phase is a bug, not a shortcut.

## 9. Concurrency Rules (Trail-specific)

These enforce the two-domain split from `trail-architecture.md` §2–4 at the
code level, not just the design level:

- UI-thread code never performs blocking I/O beyond small/fast metadata reads
  (no git calls, no image decode, no large file reads, no network) — those
  always go through the worker pool.
- Every `WorkerMsg::Preview` / `WorkerMsg::ImageMeta` carries the
  `generation` it answers to, and `workers::merge` must check it against
  `state.preview.generation` before applying. This invariant starts in Phase
  4 and must never regress in any later phase — treat a regression here as a
  release blocker, not a minor bug.
- Worker tasks never touch `ratatui`/`crossterm` state directly. All
  rendering-relevant mutation happens on the UI thread, after a channel
  receive.

## 10. Naming & Organization

- Standard Rust conventions: `snake_case` for functions/modules/variables,
  `UpperCamelCase` for types/traits/enums, `SCREAMING_SNAKE_CASE` for consts
  and statics.
- One primary type or responsibility per file where practical; file names
  mirror what they own (`state.rs` → `AppState`, `git.rs` → the git worker).
- Treat ~500 lines as a soft ceiling per file — past that, it's usually
  covering more than one responsibility from the architecture doc's tables
  and should split.
- No magic numbers: give them named constants (`TEXT_SYNC_THRESHOLD`,
  `DEFAULT_DEBOUNCE_MS`) even before Phase 7 wires them into config.

## 11. Logging

- Use a structured logging crate (`tracing` or `log`), not `println!` /
  `eprintln!`, for anything beyond throwaway local debugging — Trail owns the
  alternate screen, so stray stdout writes corrupt the UI.
- Never write to stdout/stderr while the alternate screen is active; route
  through a subscriber that writes to a file or in-memory buffer instead.

## 12. Dependency Management

- A new dependency needs a one-line justification in the PR description: why
  it's needed, and why nothing already in the stack covers it.
- Pin versions in `Cargo.toml`; commit `Cargo.lock` for the binary.
- Prefer the crates already named in the architecture/guide docs. Deviating
  means updating the Decision Log in `trail-implementation-plan.md`, not just
  silently swapping the crate in code.

## 13. Security & Safety-Sensitive Paths

- `actions/fs_ops.rs` (rename/move/delete) and `actions/shell_exec.rs`
  (subprocess spawn) are higher-risk by nature: destructive operations always
  go through the confirmation flow the spec requires, and shell execution
  never interpolates raw, unvalidated user input beyond what Command Mode's
  own grammar already checked.
- Config deserialization (Phase 7) runs in strict mode: unknown keys are a
  hard error surfaced to the user, never silently dropped — a silently
  ignored typo in something like `git_status_enabled` should not be able to
  quietly change behavior.

## 14. Commits & Reviews

- Commit messages: imperative mood, phase-tagged, e.g. `[Phase 3] Add
  :rename validation`.
- Every PR is self-checked against the Definition of Done (§15) before
  requesting review.
- At least one reviewer approval is required before merging into the branch
  later phases build on — later phases assume earlier ones are correct, so
  this is where that assumption gets earned.

## 15. Definition of Done (every task / every phase)

- [ ] `cargo fmt --check` clean
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` clean
- [ ] `cargo build` succeeds
- [ ] `cargo test` passes (unit, integration, and doc tests)
- [ ] Every new `pub` item is documented
- [ ] No new `unwrap()` / `expect()` / `panic!()` outside tests, or an
      explicitly documented exception per §4/§5
- [ ] No new `unsafe` block without a `// SAFETY:` justification
- [ ] Tests added for new logic where practical, or a stated reason if not
- [ ] The phase's exit criteria (as written in
      `trail-implementation-plan.md`) are demonstrably met
