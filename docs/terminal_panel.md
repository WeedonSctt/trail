# Trail — Terminal Panel

A product spec for a built-in terminal: a panel at the bottom of Trail that runs real,
interactive shells, so the user can browse and use a shell at the same time. **Nothing
here is built.** Sections 1–11 say only how the feature behaves; §12 records the
technical direction agreed with it, which the implementation document will expand.

Designed with the maintainer on 2026-10-08 and 2026-10-09, at v1.9.2. Companion to
[`upcoming_features.md`](upcoming_features.md) §5.2 ("a second window for commands"),
which is the triage entry that led here. Of the three options listed there, this is the
third — an embedded terminal — chosen because the goal is to *use* a shell, not to run
one command and read its output.

Every decision below is **agreed** with the maintainer unless marked otherwise. Items
marked **later** are agreed as wanted but left out of the first version; §11 collects
them so they are not lost.

---

## 1. What it is, in one paragraph

Press a key and a panel opens under the file list and the preview, holding a shell —
pwsh, bash, Git Bash, zsh, cmd, whatever the user configured. It is a real terminal:
prompts, colours, `vim`, `ssh`, `git commit` without `-m`, anything that works in a
normal terminal works there. A second key moves the keyboard between the file list and
the shell, so the user can browse with the panel open and type into the shell without
closing anything. Hiding the panel does not stop the shell. The panel can hold several
shells, shown as tabs along its top edge. It is modelled on VS Code's integrated
terminal.

## 2. Principles

These decide the cases the rest of the spec does not mention.

1. **Trail and the shell are two separate tools.** Changing folder in one never changes
   folder in the other, in either direction. The only meeting point is that a new shell
   *starts* in the folder Trail is showing.
2. **The shell is a real terminal, not an output viewer.** If a program behaves
   differently in the panel than in a standalone terminal, that is a bug.
3. **The shell gets every key, except a few reserved ones.** While the shell has focus,
   keys go to the shell — including `Esc`, `q`, `Ctrl+C`, and Trail's own bindings. Only
   the short list in §6.2 is kept back for Trail. Every key on that list is a key no
   program in the shell can receive, so the list stays as short as possible.
4. **Costs nothing until used.** No shell is started, and Trail does not start any
   slower, until the panel is first opened. Trail replaces `cd`/`ls` in a hot path, and a
   feature the user is not using must not tax that.
5. **Nothing existing changes.** `!command` and `:!` keep suspending Trail exactly as
   today. Cd-on-exit hands the shell wrapper *Trail's* folder, never the panel shell's.
   Someone who never opens the panel sees no difference.

---

## 3. Layout

```
┌ Trail ─────────────────────┐┌ preview ───────────────────────────┐
│  docs/                     ││ # Trail                            │
│> src/                      ││                                    │
│  Cargo.toml                ││ A terminal-first file manager...   │
│  README.md                 ││                                    │
└─ C:\proj\trail ────────────┘└────────────────────────────────────┘
┌ 1:pwsh │ 2:gitbash │ 3:pwsh ──────────────────────────────────────┐
│C:\proj\trail> cargo test                                          │
│   Compiling trail v1.9.2                                          │
│C:\proj\trail> _                                                   │
└───────────────────────────────────────────────────────────────────┘
 TERMINAL  2/3 gitbash                                       v1.9.2
```

- The panel spans the full width, under both the file list and the preview, above the
  status bar.
- Shells are shown as a tab strip in the panel's top border, each labelled
  `<number>:<profile name>`, with the current one highlighted.
- The status bar's mode label reads `TERMINAL` while the shell has focus. That is how
  the user tells which side has the keyboard. (A highlighted border on the focused side
  is **later**, §11.)

### 3.1 Size

- The panel's height is set in config, as a share of the screen. Default: **35%**.
- `:term max` makes the panel take the whole screen (the file list and preview are
  hidden, the status bar stays); running it again restores the normal size. For `vim`,
  `htop`, long build output. There is no key for it yet — **later**, §11.
- Hiding a maximized panel and showing it again brings it back at normal size, not
  maximized.
- No keys to grow or shrink it at runtime, and the size is not remembered between
  sessions. `:set` can still change the height for the current session, as it does for
  every other config key.
- On a very short screen the panel never squeezes the file list below a few rows; if
  both cannot fit, the panel opens maximized instead.

---

## 4. Shells and profiles

- Shells are chosen through named **profiles** in config — a name and the program to
  run (with any arguments). One profile is the default.
- With no profiles configured, there is a single built-in one, the platform's usual
  interactive shell (PowerShell on Windows, the user's login shell elsewhere). The
  feature works out of the box; profiles only add choice.
- Profiles are separate from `[general] shell`. That key is the non-interactive shell
  `!command` runs through, with `-Command`/`-c` attached; a terminal profile is an
  interactive shell. Sharing one setting would make each worse at its job.

```toml
# Illustrative only — key names are settled at implementation time.
[terminal]
default_profile = "pwsh"
height = 35

[[terminal.profile]]
name = "pwsh"
command = ["pwsh.exe", "-NoLogo"]

[[terminal.profile]]
name = "gitbash"
command = ["C:/Program Files/Git/bin/bash.exe", "--login", "-i"]
```

---

## 5. Lifetime of a shell

| Event | What happens |
|---|---|
| Panel opened, no shells yet | Starts the default profile, in the folder the current Trail tab is showing |
| Panel hidden | Shells keep running, output keeps arriving; showing the panel again returns to them as they are |
| `:term new` | Starts the default profile in Trail's current folder, adds a tab, switches to it |
| `:term new <profile>` | Same, with the named profile; an unknown name is an error on the status bar |
| The shell exits (`exit`, `Ctrl+D`, crash) | Its tab closes; the panel switches to a neighbouring shell |
| The last shell exits | The panel hides and focus returns to the file list |
| `:term close` | Ends the current shell; if it is running a command, asks first (*close confirmation*, below) |
| Quit Trail, no shell busy | All shells are closed, silently |
| Quit Trail, a shell is running a command | Trail asks: `1 shell is still running a command. Quit anyway? [y/N]` (*quit confirmation*, below) |

Both confirmations are configurable, separately, with the same three values:

| Value | Asks |
|---|---|
| `"when_busy"` — **default** | only if a shell is running a command |
| `"always"` | every time |
| `"never"` | never — the shell, and whatever it is running, ends immediately |

"Running a command" means, from the user's side: the shell is not sitting at its prompt
waiting for input. Trail will not be able to tell that reliably for every shell (§12).
Where it cannot, it treats the shell as busy and asks — erring towards a confirmation
the user did not need, never towards losing a running command.

Shells belong to the Trail session, not to a Trail tab. Switching Trail tabs does not
switch shells, and closing a Trail tab does not close any shell.

Shells do not survive Trail exiting. There is no "reattach" — that is a terminal
multiplexer's job (tmux, Zellij, Windows Terminal), not Trail's.

---

## 6. Keys and focus

Two kinds of focus: **file list** (everything works as today) and **shell** (keys go to
the shell). The panel can be open with either one focused.

### 6.1 The two main keys

| Key | Default | Panel hidden | Panel open, file list focused | Panel open, shell focused |
|---|---|---|---|---|
| **toggle** | `Ctrl+.` | open it, focus the shell | hide it | hide it, focus returns to the file list |
| **focus** | `F12` | open it, focus the shell | focus the shell | focus the file list (panel stays open) |

So **toggle** is "show/hide", and **focus** is "go to the other side". Pressing *focus*
with the panel hidden opens it rather than doing nothing, so there is never a key that
silently fails. Both are rebindable like every other binding.

#### Why `F12` for focus

`Alt+.` was the first choice and was replaced on 2026-10-09, because it is "insert the
last argument" in bash, zsh and PowerShell (PSReadLine) — a shortcut people use many
times a day, which the panel would have swallowed. The focus key is pressed constantly,
so it has to be a key **no shell and no common terminal program uses**, and one **every
terminal reports the same way**. Checked against:

| Who | Keys they already own |
|---|---|
| bash / zsh (readline, zle) | almost every `Ctrl+letter`; `Alt+letter`, `Alt+digit`, `Alt+.`, `Alt+_`, `Alt+<`/`>` |
| PowerShell (PSReadLine) | `Alt+.`, `Alt+digit`, `Alt+?`, `Ctrl+Space`, `Ctrl+]`, `F1`–`F3`, `F7`, `F8`, `Shift`/`Ctrl`+arrows |
| cmd.exe | `F1`–`F9` (line recall) |
| vim, less, htop, mc | `Ctrl+letter`, `Ctrl+\`, `Ctrl+]`, `F1`–`F10` (htop, mc) |
| Windows Terminal | `Ctrl+Shift+letter`, `Ctrl+,`, `Ctrl+Shift+,` (settings), `Ctrl+=`/`-`/`0` (zoom), `Alt+Enter`, `F11`, `Alt+Shift+…` (panes), `Ctrl+Alt+digit`, `Ctrl+Shift+M` |
| Windows itself | `Ctrl+Alt+…` is AltGr — it types `\`, `@`, `|` on Spanish layouts |

`F12` is outside all of them. It is also the most robust key to *receive*: every terminal
sends it as the same escape sequence, with no modifier that a terminal might drop, and it
is the same key on every keyboard layout. Combinations with punctuation were rejected
for the second reason: on the Latin American layout `;` is `Shift+,`, so `Ctrl+;` would
arrive as `Ctrl+Shift+,` — Windows Terminal's "open settings".

One consequence of the defaults, recorded so it is not rediscovered as a bug: **`Ctrl+.`
must arrive at Trail with its `Ctrl`.** A terminal that cannot report it would deliver a
plain `.` — which in the file list is *toggle hidden files*. Windows Terminal reports it;
the user guide says what to bind instead on a terminal that does not.

### 6.2 Reserved while the shell is focused

The complete list of keys the shell does **not** receive. Everything else goes to it.

| Action | Default | Why it is worth stealing |
|---|---|---|
| toggle | `Ctrl+.` | the way out |
| focus | `F12` | the way back to the file list |
| next shell / previous shell | `Ctrl+PageDown` / `Ctrl+PageUp` | VS Code's and browsers' tab keys. No shell uses them for editing; vim uses them for its own tabs, where `gt`/`gT` still work |
| scroll back / forward through output | `Shift+PageUp` / `Shift+PageDown` | what every standalone terminal does with these keys |

Scrolling back shows earlier output; typing anything returns the view to the bottom, as
in a standalone terminal.

### 6.3 Commands

Everything else about the panel is a command in the first version. Keys for the common
ones are **later** (§11).

| Command | Does |
|---|---|
| `:term` | same as the toggle key |
| `:term new [profile]` | new shell in Trail's current folder (default profile if none named) |
| `:term close` | close the current shell |
| `:term <n>` | switch to shell number *n* |
| `:term max` | maximize / restore |

### 6.4 Text in and out

- Pasting (Ctrl+V / right-click / Shift+Insert, as the outer terminal delivers it) goes
  to the shell when the shell has focus.
- **Known limitation:** selecting text with the mouse is done by the outer terminal
  (Windows Terminal, etc.), not by Trail, so a drag selects across the whole screen —
  file list included — rather than within the panel. Copying a single line of output
  works; copying a block that sits next to the file list picks that up too. This is a
  property of running inside another terminal and is the same in every TUI.

---

## 7. What the shell can and cannot see of Trail

Following principle 1, very little, on purpose:

- A new shell starts in Trail's current folder. That is the whole link.
- No "cd here" key, no automatic following, no Trail following the shell's `cd`.
- No key to type the selected file's path into the shell. Considered and left out; it
  can be revisited later without changing anything here.
- Trail's file list already refreshes when files change on disk, so a file the shell
  creates or deletes appears or disappears in the listing on its own. Nothing new is
  needed for that.

---

## 8. Configuration — the user's view

What the user can set, without committing to key names (settled at implementation):

| Setting | Default |
|---|---|
| Default profile | built-in platform shell |
| Profiles: name and command | none (the built-in one) |
| Panel height, as a share of the screen | 35% |
| Quit confirmation: `when_busy` / `always` / `never` | `when_busy` |
| Close confirmation (`:term close`): same values | `when_busy` |
| Toggle key | `Ctrl+.` |
| Focus key | `F12` |
| Next / previous shell keys | `Ctrl+PageDown` / `Ctrl+PageUp` |
| Scroll back / forward keys | `Shift+PageUp` / `Shift+PageDown` |

Every setting follows Trail's existing rules: strict parsing (an unknown key is an
error), documented in `configuration_guide.md`, adjustable at runtime with `:set` where
that makes sense.

---

## 9. Out of scope

Said explicitly, so a later request can be checked against it:

- Directory syncing in either direction (§7).
- Running `!command` in the panel — `!` keeps suspending Trail.
- Splitting the panel into side-by-side shells, or putting it anywhere but the bottom.
- Shells that outlive Trail, or reattaching to them.
- Resizing the panel with keys at runtime, or remembering its size between sessions.
- Inserting paths or marked entries into the shell.
- Search within shell output, clickable links, shell-integration features (command
  decorations, "re-run last command").

## 10. Versioning

A **MINOR** release under `CLAUDE.md` §8: new keys and new config with defaults that
change nothing for someone who does not use them. `Ctrl+.` and `F12` collide with no
existing Trail binding, provided they arrive with their modifier (§6.1).

---

## 11. Later — agreed as wanted, not in the first version

| # | Item | Notes |
|---|---|---|
| L1 | **Keys for the `:term` commands** | a maximize key usable from inside the shell, a file-list key for "new shell in this folder", and keys for close / switch-by-number. The first version has the commands only (§6.3). Each new key reserved in the shell is one a shell program loses, so they are chosen together, not one at a time |
| L2 | **Running-shells indicator** | a small mark on the status bar (e.g. `▣ 2`) while the panel is hidden but shells are still running, so a forgotten shell is not invisible |
| L3 | **Exit notice** | a status-bar notification when a shell exits with a non-zero code; without it a crashing shell just vanishes |
| L4 | **Focused-border colour** | the focused side's border drawn in a themeable highlight colour, in addition to the `TERMINAL` label. The theme has a single `border` colour today |

---

## 12. Technical direction — agreed

Recorded here so the decisions travel with the spec; the implementation document expands
them.

1. **Libraries.** `portable-pty` runs the shells (ConPTY on Windows) and `vt100` turns
   their output into a screen grid. Trail draws that grid with its own code rather than
   `tui-term`: `tui-term`'s current releases require ratatui 0.29/0.30 (only 0.1.13
   matches Trail's 0.28), and a second ratatui is the trap `CLAUDE.md` §9 describes. Both
   dependencies go in the Decision Log in `trail_implementation_plan.md` when added.
2. **Busy detection is best-effort.** Shells do not report "running a command" in a
   standard way; where Trail cannot tell, it treats the shell as busy (§5).
3. **Output floods are rate-limited.** Heavy output (a build, a large file) is drawn at a
   capped number of frames per second, so the file list stays responsive. The panel gets
   a performance budget, as the external previewers did, before it reaches `main`.
4. **`#![forbid(unsafe_code)]` stays.** `portable-pty` uses `unsafe` internally to talk
   to the OS, as `crossterm` and `tokio` already do; Trail's own code stays free of it.
5. **Testing.** Key translation, drawing, and shell lifetime are tested automatically.
   Real feel, resizing, ConPTY behaviour and the §6.1 key check need the maintainer's
   manual pass — listed alongside the image matrix as not automatically verifiable.
6. **Order.** The `feat/external-previewers` work lands on `main` first; the terminal
   panel touches the same areas (workers, layout, keymap) and is built after it.
