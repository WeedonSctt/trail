# Trail — Terminal Panel

A product spec for a built-in terminal: a panel at the bottom of Trail that runs real,
interactive shells, so the user can browse and use a shell at the same time. **Nothing
here is built**, and this document deliberately says nothing about *how* it is built —
that is the next document, written once this one is agreed.

Designed with the maintainer on 2026-10-08, at v1.9.2. Companion to
[`upcoming_features.md`](upcoming_features.md) §5.2 ("a second window for commands"),
which is the triage entry that led here. Of the three options listed there, this is the
third — an embedded terminal — chosen because the goal is to *use* a shell, not to run
one command and read its output.

Status of each decision: **agreed** (the maintainer chose it), **proposed** (filled in
to complete the design; confirm or change it), **open** (needs an answer before
implementation starts). §11 collects the proposed and open ones.

---

## 1. What it is, in one paragraph

Press a key and a panel slides in under the file list and the preview, holding a shell —
pwsh, bash, Git Bash, zsh, cmd, whatever the user configured. It is a real terminal:
prompts, colours, `vim`, `ssh`, `git commit` without `-m`, anything that works in a
normal terminal works there. A second key moves the keyboard between the file list and
the shell, so the user can browse with the panel open and type into the shell without
closing anything. Hiding the panel does not stop the shell. The panel can hold several
shells, shown as tabs along its top edge. It is modelled on VS Code's integrated
terminal.

## 2. Principles

These decide the cases the rest of the spec does not mention.

1. **Trail and the shell are two separate tools.** **Agreed.** Changing folder in one
   never changes folder in the other, in either direction. The only meeting point is
   that a new shell *starts* in the folder Trail is showing.
2. **The shell is a real terminal, not an output viewer.** **Agreed.** If a program
   behaves differently in the panel than in a standalone terminal, that is a bug.
3. **The shell gets every key, except a few reserved ones.** **Proposed.** While the
   shell has focus, keys go to the shell — including `Esc`, `q`, `Ctrl+C`, and Trail's
   own bindings. Only the short list in §6.2 is kept back for Trail. Every key on that
   list is a key no program in the shell can receive, so the list stays as short as
   possible.
4. **Costs nothing until used.** **Proposed.** No shell is started, and Trail does not
   start any slower, until the panel is first opened. Trail replaces `cd`/`ls` in a hot
   path, and a feature the user is not using must not tax that.
5. **Nothing existing changes.** **Agreed.** `!command` and `:!` keep suspending Trail
   exactly as today. Cd-on-exit hands the shell wrapper *Trail's* folder, never the
   panel shell's. Someone who never opens the panel sees no difference.

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

- **Agreed:** the panel spans the full width, under both the file list and the preview,
  above the status bar.
- **Agreed:** shells are shown as a tab strip in the panel's top border, each labelled
  `<number>:<profile name>`, with the current one highlighted.
- **Proposed:** whichever side has focus gets a highlighted border, the other keeps the
  normal one. The theme has a single `border` colour today, so this adds a themeable
  "focused border" colour. The status bar's mode label also reads `TERMINAL` while the
  shell has focus, so focus is never signalled by colour alone.
- **Proposed:** while the panel is hidden but shells are still running, the status bar
  shows a small indicator (e.g. `▣ 2`), so a forgotten shell is not invisible.

### 3.1 Size

- **Agreed:** the panel's height is set in config, as a share of the screen.
  **Proposed** default: 35%.
- **Agreed:** a **maximize** key makes the panel take the whole screen (the file list
  and preview are hidden, the status bar stays) and the same key restores it. For
  `vim`, `htop`, long build output.
- **Proposed:** hiding a maximized panel and showing it again brings it back at normal
  size, not maximized.
- **Agreed:** no keys to grow or shrink it at runtime, and the size is not remembered
  between sessions. `:set` can still change the height for the current session, as it
  does for every other config key.
- **Proposed:** on a very short screen the panel never squeezes the file list below a
  few rows; if both cannot fit, the panel opens maximized instead.

---

## 4. Shells and profiles

- **Agreed:** shells are chosen through named **profiles** in config — a name and the
  program to run (with any arguments). One profile is the default.
- **Proposed:** with no profiles configured, there is a single built-in one, the
  platform's usual interactive shell (PowerShell on Windows, the user's login shell
  elsewhere). The feature works out of the box; profiles only add choice.
- **Proposed:** profiles are separate from `[general] shell`. That key is the
  non-interactive shell `!command` runs through, with `-Command`/`-c` attached; a
  terminal profile is an interactive shell. Sharing one setting would make each worse
  at its job.

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

| Event | What happens | Status |
|---|---|---|
| Panel opened, no shells yet | Starts the default profile, in the folder the current Trail tab is showing | agreed |
| Panel hidden | Shells keep running, output keeps arriving; showing the panel again returns to them as they are | agreed |
| New shell (key or `:term new`) | Starts the default profile in Trail's current folder, adds a tab, switches to it | agreed |
| `:term new <profile>` | Same, with the named profile; an unknown name is an error on the status bar | agreed |
| The shell exits (`exit`, `Ctrl+D`, crash) | Its tab closes; the panel switches to a neighbouring shell | agreed |
| The last shell exits | The panel hides and focus returns to the file list | agreed |
| A shell exits with a non-zero code | A notification on the status bar says so — otherwise a crashing shell would just vanish | proposed |
| Close a shell from Trail (key or `:term close`) | Ends it; asks first if it is running a command (same rule as quitting, below) | proposed |
| Quit Trail, no shell busy | All shells are closed, silently | agreed |
| Quit Trail, a shell is running a command | Trail asks: `1 shell is still running a command. Quit anyway? [y/N]` | agreed |

**Agreed:** the quit confirmation is configurable. **Proposed** values:
`"when_busy"` (default — ask only if a shell is running a command), `"always"`, and
`"never"` (quit closes every shell immediately, whatever it is doing).

"Running a command" means, from the user's side: the shell is not sitting at its prompt
waiting for input. **Open** — whether Trail can tell that reliably for every shell is an
implementation question; if it cannot for some shell, the spec's answer is to **ask**
(err towards a confirmation the user did not need, never towards losing a running
command).

Shells belong to the Trail session, not to a Trail tab. **Agreed:** switching Trail tabs
does not switch shells, and closing a Trail tab does not close any shell.

Shells do not survive Trail exiting. There is no "reattach" — that is a terminal
multiplexer's job (tmux, Zellij, Windows Terminal), not Trail's.

---

## 6. Keys and focus

Two kinds of focus: **file list** (everything works as today) and **shell** (keys go to
the shell). The panel can be open with either one focused.

### 6.1 The two main keys — agreed

| Key | Panel hidden | Panel open, file list focused | Panel open, shell focused |
|---|---|---|---|
| **toggle** | open it, focus the shell | hide it | hide it, focus returns to the file list |
| **focus** | open it, focus the shell | focus the shell | focus the file list (panel stays open) |

So **toggle** is "show/hide", and **focus** is "go to the other side". Pressing *focus*
with the panel hidden opens it rather than doing nothing, so there is never a key that
silently fails.

**Open:** which actual keys. The mockup used `Ctrl+\`` and `Ctrl+J`. `Ctrl+\`` is VS
Code's and is the natural toggle, but whether Windows Terminal delivers it to Trail has
to be checked on the maintainer's machine. `Ctrl+J` should **not** be the focus key: to
a shell, `Ctrl+J` *is* the Enter key, so stealing it would make shells behave strangely.
Both keys will be rebindable like every other binding; the question is only the
defaults. §11 lists candidates.

### 6.2 Reserved while the shell is focused — proposed

The complete list of keys the shell does **not** receive. Everything else goes to it.

| Action | Proposed default | Why it is worth stealing |
|---|---|---|
| toggle | see §6.1 | the way out |
| focus | see §6.1 | the way back to the file list |
| next shell / previous shell | `Ctrl+PageDown` / `Ctrl+PageUp` | VS Code's and browsers' tab keys; shells rarely use them |
| scroll back / forward through output | `Shift+PageUp` / `Shift+PageDown` | what every standalone terminal does with these keys |
| maximize | `Ctrl+Shift+M` or similar — **open** | for `vim` and long output, without leaving the shell |

Scrolling back shows earlier output; typing anything returns the view to the bottom, as
in a standalone terminal.

### 6.3 From the file list, with the panel open — proposed

All of Trail's existing keys keep working. In addition, the panel's actions are reachable
as commands, so nothing needs a new letter key:

| Command | Does |
|---|---|
| `:term` | same as the toggle key |
| `:term new [profile]` | new shell in Trail's current folder (default profile if none named) |
| `:term close` | close the current shell |
| `:term <n>` | switch to shell number *n* |
| `:term max` | maximize / restore |

**Open:** whether "new shell in this folder" also deserves a key from the file list. It
is the one integration the maintainer asked for, and a command may be too slow for it.

### 6.4 Text in and out

- **Proposed:** pasting (Ctrl+V / right-click / Shift+Insert, as the outer terminal
  delivers it) goes to the shell when the shell has focus.
- **Known limitation:** selecting text with the mouse is done by the outer terminal
  (Windows Terminal, etc.), not by Trail, so a drag selects across the whole screen —
  file list included — rather than within the panel. Copying a single line of output
  works; copying a block that sits next to the file list picks that up too. This is a
  property of running inside another terminal and is the same in every TUI.

---

## 7. What the shell can and cannot see of Trail

Following principle 1, very little, on purpose:

- **Agreed:** a new shell starts in Trail's current folder. That is the whole link.
- **Agreed, not built:** no "cd here" key, no automatic following, no Trail following
  the shell's `cd`.
- **Not built:** no key to type the selected file's path into the shell. Considered and
  left out; it can be revisited later without changing anything here.
- Trail's file list already refreshes when files change on disk, so a file the shell
  creates or deletes appears or disappears in the listing on its own. Nothing new is
  needed for that.

---

## 8. Configuration — the user's view

What the user can set, without committing to key names (settled at implementation):

| Setting | Proposed default |
|---|---|
| Default profile | built-in platform shell |
| Profiles: name and command | none (the built-in one) |
| Panel height, as a share of the screen | 35% |
| Quit confirmation: when busy / always / never | when busy |
| Keys: toggle, focus, next/previous shell, scroll, maximize | §6 |
| Focused-border colour (theme) | a colour distinct from `border` |

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
change nothing for someone who does not use them. If a default key chosen in §6 turns out
to collide with an existing binding and that binding has to move, it becomes MAJOR — one
more reason the defaults in §11 should avoid every key Trail already uses.

---

## 11. Decisions still to make

| # | Question | Proposal |
|---|---|---|
| 1 | Default **toggle** key | `Ctrl+\``, if Windows Terminal delivers it to Trail — to be checked on the maintainer's terminal before it is fixed |
| 2 | Default **focus** key | not `Ctrl+J` (it is Enter to a shell). Candidates: `Alt+\``, `F12`, `Alt+J` — same check as #1 |
| 3 | Default **maximize** key while the shell is focused | `Ctrl+Shift+M` or a function key; or no key, only `:term max` |
| 4 | A file-list key for "new shell in this folder" | yes, one key — which one depends on #1–#3 |
| 5 | Panel height default | 35% |
| 6 | Quit-confirmation values | `when_busy` (default), `always`, `never` |
| 7 | Closing a single busy shell from Trail | asks, under the same setting as quitting |
| 8 | Indicator for running shells while the panel is hidden | yes |
| 9 | Notification when a shell exits with an error | yes |
| 10 | What "busy" means when Trail cannot tell for some shell | treat it as busy — ask rather than risk losing work |

Everything else in this document marked **proposed** stands unless changed.
