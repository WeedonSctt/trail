//! The terminal panel: interactive shells in a panel under the file list.
//!
//! Behaviour is specified in `docs/terminal_panel.md`; this module is that
//! spec's state machine. [`TerminalPanel`] lives on [`crate::app::state::AppState`] and owns the
//! [`session::Session`]s; [`handle_key`] runs before Trail's own keymap and
//! decides whether a keystroke belongs to the panel; `:term` commands arrive
//! through [`run_command`]. Drawing is `ui::terminal_panel`'s job.
//!
//! Trail and the shells are deliberately independent: a shell starts in the
//! folder Trail shows, and from then on neither moves the other.

pub mod busy;
mod input;
pub mod keys;
pub mod profile;
pub mod session;

pub use input::{allow_quit, handle_key, run_command};

use std::path::Path;

use tokio::sync::mpsc;

use crate::config::TerminalConfig;
use crate::workers::WorkerMsg;
use session::Session;

/// Smallest panel worth drawing: a border, and three rows of shell inside it.
pub const MIN_PANEL_ROWS: u16 = 5;
/// Fewest rows the file list keeps before the panel takes the whole screen.
pub const MIN_LIST_ROWS: u16 = 5;

/// The action names `[keymap.terminal]` accepts.
pub const TERMINAL_ACTIONS: &[&str] = &[
    "toggle",
    "focus",
    "next_shell",
    "prev_shell",
    "scroll_up",
    "scroll_down",
];

/// When Trail asks before ending shells: `[terminal] confirm_quit` and
/// `confirm_close`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmPolicy {
    /// Ask only when a shell is running a command.
    WhenBusy,
    /// Ask every time there is a shell to end.
    Always,
    /// Never ask.
    Never,
}

/// Explanation attached to a rejected confirmation setting.
pub const CONFIRM_REASON: &str = "must be when_busy, always or never";

impl ConfirmPolicy {
    /// Parses a config value, case-insensitively.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "when_busy" => Some(Self::WhenBusy),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }
}

/// A `:term` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermCommand {
    /// `:term` — the same as the toggle key.
    Toggle,
    /// `:term new [profile]` — a new shell in Trail's current folder.
    New(Option<String>),
    /// `:term close` — end the current shell.
    Close,
    /// `:term <n>` — switch to shell number `n` (1-based).
    Select(usize),
    /// `:term max` — maximize or restore the panel.
    Max,
}

/// A question the panel is waiting on, answered with `y`/Enter or `n`/Esc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    /// Quitting Trail would end the shells. `cancel` is Ctrl+C's quit, which
    /// skips the cd-on-exit handoff.
    Quit {
        /// Whether the quit was a cancel (`Ctrl+C`) rather than `q`.
        cancel: bool,
        /// The prompt, already worded for how many shells are busy.
        prompt: String,
    },
    /// `:term close` would end a shell.
    Close {
        /// The prompt naming the shell.
        prompt: String,
    },
}

impl Confirm {
    /// The question as the status bar shows it.
    #[must_use]
    pub fn prompt(&self) -> &str {
        match self {
            Self::Quit { prompt, .. } | Self::Close { prompt } => prompt,
        }
    }
}

/// What [`handle_key`] did with a keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Not the panel's key: Trail's keymap should handle it.
    NotMine,
    /// The panel used it.
    Handled,
    /// The user confirmed quitting. `cancel` as in [`Confirm::Quit`].
    Quit {
        /// Whether to quit without the cd-on-exit handoff.
        cancel: bool,
    },
}

/// The panel's state: its shells, and how it is shown.
#[derive(Debug, Default)]
pub struct TerminalPanel {
    sessions: Vec<Session>,
    active: usize,
    visible: bool,
    shell_focused: bool,
    maximized: bool,
    next_id: u64,
    pending: Option<Confirm>,
    output_pending: bool,
    notify: Option<mpsc::Sender<WorkerMsg>>,
}

impl TerminalPanel {
    /// Connects the panel to the worker channel its sessions report on.
    /// Until this is called — as in tests — opening a shell is an error.
    pub fn attach(&mut self, notify: mpsc::Sender<WorkerMsg>) {
        self.notify = Some(notify);
    }

    /// Whether the panel is on screen.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Whether keystrokes go to the shell rather than to Trail.
    #[must_use]
    pub fn shell_focused(&self) -> bool {
        self.visible && self.shell_focused
    }

    /// Whether the panel takes the whole screen above the status bar.
    #[must_use]
    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// The open shells, in tab order.
    #[must_use]
    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    /// Index of the shell on screen.
    #[must_use]
    pub fn active(&self) -> usize {
        self.active
    }

    /// The question waiting for an answer, if any.
    #[must_use]
    pub fn pending(&self) -> Option<&Confirm> {
        self.pending.as_ref()
    }

    /// Resizes every shell to the panel's inner size. Called by the renderer,
    /// which is where the size is known.
    pub fn resize_all(&mut self, size: (u16, u16)) {
        for session in &mut self.sessions {
            session.resize(size);
        }
    }

    /// Records that a shell produced output. The event loop draws it at the
    /// next frame rather than immediately, so a flood is drawn at a capped rate.
    ///
    /// A hidden panel draws nothing, so its output is not worth a frame. Its
    /// readers stay quiet until the next frame drawn for any other reason
    /// resets them, which also means a hidden shell costs one message per
    /// frame at most, however much it prints.
    pub fn note_output(&mut self) {
        if self.visible {
            self.output_pending = true;
        }
    }

    /// Whether output is waiting to be drawn.
    #[must_use]
    pub fn output_pending(&self) -> bool {
        self.output_pending
    }

    /// Called after a frame is drawn: what was pending is on screen, and every
    /// reader may announce new output again.
    pub fn frame_drawn(&mut self) {
        self.output_pending = false;
        for session in &self.sessions {
            session.output_drawn();
        }
    }

    /// Removes the shell `id` after it exited. Returns whether anything changed.
    pub fn on_exit(&mut self, id: u64) -> bool {
        let Some(index) = self.sessions.iter().position(|s| s.id() == id) else {
            return false;
        };
        self.sessions.remove(index);
        self.after_removal(index);
        true
    }

    fn after_removal(&mut self, index: usize) {
        if self.sessions.is_empty() {
            self.visible = false;
            self.shell_focused = false;
            self.maximized = false;
            self.active = 0;
            if matches!(self.pending, Some(Confirm::Close { .. })) {
                self.pending = None;
            }
            return;
        }
        if index < self.active || self.active >= self.sessions.len() {
            self.active = self.active.saturating_sub(1);
        }
    }

    /// Starts a new shell from `profile_name` (or the default) in `cwd`, makes
    /// it the active one, and shows and focuses the panel.
    ///
    /// # Errors
    ///
    /// Returns a message for the status bar when the profile is unknown, the
    /// panel is not attached, or the program cannot be started.
    pub fn new_shell(
        &mut self,
        config: &TerminalConfig,
        profile_name: Option<&str>,
        cwd: &Path,
    ) -> Result<(), String> {
        let profile = profile::resolve(config, profile_name)?;
        let notify = self
            .notify
            .clone()
            .ok_or_else(|| "the terminal panel is not available".to_owned())?;
        self.next_id += 1;
        let size = initial_size(config.height, self.maximized);
        let session = Session::spawn(
            self.next_id,
            &profile.name,
            &profile.argv,
            cwd,
            size,
            notify,
        )?;
        self.sessions.push(session);
        self.active = self.sessions.len() - 1;
        self.visible = true;
        self.shell_focused = true;
        Ok(())
    }

    /// The toggle key: shows the panel (starting a shell if there is none) and
    /// focuses it, or hides it and returns focus to the file list.
    ///
    /// # Errors
    ///
    /// As [`Self::new_shell`], when opening has to start a shell.
    pub fn toggle(&mut self, config: &TerminalConfig, cwd: &Path) -> Result<(), String> {
        if self.visible {
            self.visible = false;
            self.shell_focused = false;
            self.maximized = false;
            return Ok(());
        }
        self.show(config, cwd)
    }

    /// The focus key: moves the keyboard to the other side, opening the panel
    /// first if it is hidden.
    ///
    /// # Errors
    ///
    /// As [`Self::new_shell`], when opening has to start a shell.
    pub fn switch_focus(&mut self, config: &TerminalConfig, cwd: &Path) -> Result<(), String> {
        if !self.visible {
            return self.show(config, cwd);
        }
        self.shell_focused = !self.shell_focused;
        Ok(())
    }

    fn show(&mut self, config: &TerminalConfig, cwd: &Path) -> Result<(), String> {
        if self.sessions.is_empty() {
            return self.new_shell(config, None, cwd);
        }
        self.visible = true;
        self.shell_focused = true;
        Ok(())
    }

    /// Switches to the shell `delta` tabs away, wrapping.
    pub fn cycle(&mut self, delta: isize) {
        let len = self.sessions.len();
        if len > 1 {
            let len = len as isize;
            self.active = (self.active as isize + delta).rem_euclid(len) as usize;
        }
    }

    /// Switches to shell number `n`, counted from 1 as the tabs are labelled.
    ///
    /// # Errors
    ///
    /// Returns a message when there is no shell `n`.
    pub fn select(&mut self, n: usize) -> Result<(), String> {
        if n == 0 || n > self.sessions.len() {
            return Err(format!(
                "no shell {n} (there {})",
                match self.sessions.len() {
                    0 => "are none".to_owned(),
                    1 => "is 1".to_owned(),
                    k => format!("are {k}"),
                }
            ));
        }
        self.active = n - 1;
        self.visible = true;
        Ok(())
    }

    /// Ends the active shell now, without asking.
    pub fn close_active(&mut self) {
        if self.active < self.sessions.len() {
            let index = self.active;
            self.sessions.remove(index);
            self.after_removal(index);
        }
    }

    /// Maximizes the panel, or restores it.
    pub fn toggle_maximized(&mut self) {
        if self.visible {
            self.maximized = !self.maximized;
        }
    }

    /// Whether quitting needs a confirmation under `policy`, and if so the
    /// question to ask. Reads the process table when the policy is
    /// `when_busy`, which costs tens of milliseconds — fine on a quit.
    #[must_use]
    pub fn quit_confirmation(&self, policy: ConfirmPolicy, cancel: bool) -> Option<Confirm> {
        if self.sessions.is_empty() {
            return None;
        }
        let busy = match policy {
            ConfirmPolicy::Never => return None,
            ConfirmPolicy::Always | ConfirmPolicy::WhenBusy => self.busy_count(),
        };
        let prompt = match (policy, busy) {
            (ConfirmPolicy::WhenBusy, 0) => return None,
            (_, 0) => format!(" Quit and close {}? [y/N] ", shells(self.sessions.len())),
            (_, 1) => " 1 shell is still running a command. Quit anyway? [y/N] ".to_owned(),
            (_, n) => format!(" {n} shells are still running a command. Quit anyway? [y/N] "),
        };
        Some(Confirm::Quit { cancel, prompt })
    }

    fn busy_count(&self) -> usize {
        let pids: Vec<Option<u32>> = self.sessions.iter().map(Session::pid).collect();
        busy::busy(&pids).into_iter().filter(|b| *b).count()
    }

    /// `:term close`: closes the active shell, or asks first under `policy`.
    pub fn request_close(&mut self, policy: ConfirmPolicy) {
        let Some(session) = self.sessions.get(self.active) else {
            return;
        };
        let name = format!("{}:{}", self.active + 1, session.label());
        let prompt = match policy {
            ConfirmPolicy::Never => None,
            ConfirmPolicy::Always | ConfirmPolicy::WhenBusy => {
                let running = busy::busy(&[session.pid()])
                    .first()
                    .copied()
                    .unwrap_or(true);
                match (policy, running) {
                    (_, true) => Some(format!(
                        " Shell {name} is running a command. Close it? [y/N] "
                    )),
                    (ConfirmPolicy::Always, false) => Some(format!(" Close shell {name}? [y/N] ")),
                    _ => None,
                }
            }
        };
        match prompt {
            Some(prompt) => self.pending = Some(Confirm::Close { prompt }),
            None => self.close_active(),
        }
    }

    /// Puts a quit question up for the user to answer.
    pub fn ask(&mut self, confirm: Confirm) {
        self.pending = Some(confirm);
    }
}

fn shells(n: usize) -> String {
    if n == 1 {
        "1 shell".to_owned()
    } else {
        format!("{n} shells")
    }
}

/// The panel's height for a main area of `main_rows`, as `ui` lays it out.
///
/// `percent` is `[terminal] height`. A panel that would leave the file list
/// fewer than [`MIN_LIST_ROWS`] takes the whole area instead, as the spec
/// says for a very short screen.
#[must_use]
pub fn panel_rows(main_rows: u16, percent: u16, maximized: bool) -> u16 {
    if maximized {
        return main_rows;
    }
    let wanted = (u32::from(main_rows) * u32::from(percent) / 100) as u16;
    let rows = wanted.max(MIN_PANEL_ROWS);
    if main_rows.saturating_sub(rows) < MIN_LIST_ROWS {
        main_rows
    } else {
        rows
    }
}

/// The inner size (rows, cols) a new shell starts at, from the real terminal's
/// size. The renderer corrects it on the first frame if the guess is off.
fn initial_size(percent: u16, maximized: bool) -> (u16, u16) {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let main_rows = rows.saturating_sub(1);
    let panel = panel_rows(main_rows, percent, maximized);
    (panel.saturating_sub(2), cols.saturating_sub(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_policies_parse() {
        assert_eq!(
            ConfirmPolicy::parse("when_busy"),
            Some(ConfirmPolicy::WhenBusy)
        );
        assert_eq!(
            ConfirmPolicy::parse(" Always "),
            Some(ConfirmPolicy::Always)
        );
        assert_eq!(ConfirmPolicy::parse("never"), Some(ConfirmPolicy::Never));
        assert_eq!(ConfirmPolicy::parse("sometimes"), None);
    }

    #[test]
    fn the_panel_takes_its_share_and_falls_back_to_the_whole_area() {
        assert_eq!(panel_rows(40, 35, false), 14);
        assert_eq!(panel_rows(40, 35, true), 40);
        // Never smaller than a usable panel.
        assert_eq!(panel_rows(20, 10, false), MIN_PANEL_ROWS);
        // Too short for both: the panel gets everything.
        assert_eq!(panel_rows(9, 35, false), 9);
    }

    #[test]
    fn an_empty_panel_never_asks_and_never_opens_without_a_channel() {
        let panel = TerminalPanel::default();
        assert_eq!(panel.quit_confirmation(ConfirmPolicy::Always, false), None);

        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::load(None).unwrap().terminal;
        let mut panel = TerminalPanel::default();
        let err = panel.toggle(&config, dir.path()).unwrap_err();
        assert!(err.contains("not available"), "{err}");
        assert!(!panel.is_visible(), "a failed open leaves the panel hidden");
    }
}
