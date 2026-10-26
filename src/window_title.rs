//! The terminal window or tab title, so the tab Trail is running in can be
//! told apart from the others.
//!
//! Set with OSC 0 (crossterm's [`SetTitle`]), which terminals show as the tab
//! or window title, and kept as `trail <directory>` while Trail runs, in the
//! form `[general] terminal_title` asks for. The title the shell had is saved
//! on the terminal's title stack (XTWINOPS 22/23) before the first change and
//! restored from it on exit, or when `terminal_title` is set to `off`.
//!
//! Before that restore Trail blanks the title, because a terminal without a
//! title stack ignores the restore — ConPTY, which every Windows terminal
//! reaches the console through, is one. There the tab is left without a title
//! rather than frozen on a directory Trail is no longer reporting, until the
//! shell's prompt sets its own; the PowerShell wrapper restores the shell's
//! title itself as soon as Trail exits.

use std::fmt;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::terminal::SetTitle;
use crossterm::Command;

/// Whether this process has pushed a title it still owes the terminal.
///
/// Process-wide so the panic hook, which cannot reach the event loop's
/// [`WindowTitle`], can restore the title too.
static PUSHED: AtomicBool = AtomicBool::new(false);

/// What the title names, from `[general] terminal_title`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleMode {
    /// Leave the title alone.
    Off,
    /// `trail <directory name>`.
    Dir,
    /// `trail <full path>`.
    Path,
}

impl TitleMode {
    /// Parses a `[general] terminal_title` value: `off`, `dir` or `path`,
    /// ignoring case and surrounding space. `None` for anything else.
    pub fn parse(value: &str) -> Option<TitleMode> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" => Some(TitleMode::Off),
            "dir" => Some(TitleMode::Dir),
            "path" => Some(TitleMode::Path),
            _ => None,
        }
    }
}

/// The title for `cwd` in `mode`, or `None` when the title is left alone.
///
/// A directory with no name of its own — a drive or filesystem root — is
/// named by its full path in either mode. Control characters are dropped: a
/// directory name carrying an escape sequence must not be able to write to
/// the terminal.
pub fn title_for(mode: TitleMode, cwd: &Path) -> Option<String> {
    let place = match mode {
        TitleMode::Off => return None,
        TitleMode::Dir => cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| crate::pathfmt::display(cwd)),
        TitleMode::Path => crate::pathfmt::display(cwd),
    };
    let place: String = place.chars().filter(|c| !c.is_control()).collect();
    Some(format!("trail {place}"))
}

/// Keeps the terminal title in step with Trail's directory, writing only
/// when the title changes.
#[derive(Debug, Default)]
pub struct WindowTitle {
    /// The title last written, `None` when none is shown or it must be
    /// written again.
    shown: Option<String>,
}

impl WindowTitle {
    /// Writes the title for `cwd` in `mode` to `out` if it differs from the
    /// one shown, saving the shell's title first. Switching to
    /// [`TitleMode::Off`] gives the shell's title back.
    ///
    /// # Errors
    /// Any error writing to `out`.
    pub fn update(&mut self, out: &mut impl Write, mode: TitleMode, cwd: &Path) -> io::Result<()> {
        let Some(title) = title_for(mode, cwd) else {
            return self.restore(out);
        };
        if self.shown.as_ref() == Some(&title) {
            return Ok(());
        }
        if !PUSHED.swap(true, Ordering::SeqCst) {
            write_command(out, PushTitle)?;
        }
        write_command(out, SetTitle(&title))?;
        out.flush()?;
        self.shown = Some(title);
        Ok(())
    }

    /// Marks the title as needing to be written again — after an editor or
    /// shell command has had the terminal and may have set its own.
    pub fn forget(&mut self) {
        self.shown = None;
    }

    /// Gives the title back, if Trail changed it: blanks it, then restores
    /// the shell's title where the terminal kept it (see the module docs).
    ///
    /// # Errors
    /// Any error writing to `out`.
    pub fn restore(&mut self, out: &mut impl Write) -> io::Result<()> {
        self.shown = None;
        restore_to(out)
    }
}

/// Gives the title back as [`WindowTitle::restore`] does, for the panic hook.
/// Does nothing if Trail never changed it.
///
/// # Errors
/// Any error writing to `out`.
pub fn restore_to(out: &mut impl Write) -> io::Result<()> {
    if PUSHED.swap(false, Ordering::SeqCst) {
        write_command(out, SetTitle(""))?;
        write_command(out, PopTitle)?;
        out.flush()?;
    }
    Ok(())
}

/// Writes `command`'s escape sequence to `out`.
///
/// Always the ANSI form, never crossterm's Windows-console fallback: every
/// console Trail draws in already takes ANSI, since that is how ratatui draws,
/// and the fallback would write nothing a test could see.
fn write_command(out: &mut impl Write, command: impl Command) -> io::Result<()> {
    let mut ansi = String::new();
    command
        .write_ansi(&mut ansi)
        .map_err(|_| io::Error::other("formatting a terminal command failed"))?;
    out.write_all(ansi.as_bytes())
}

/// XTWINOPS 22: save the window and icon titles on the terminal's stack.
struct PushTitle;

/// XTWINOPS 23: restore the titles saved by [`PushTitle`].
struct PopTitle;

impl Command for PushTitle {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        f.write_str("\x1b[22;0t")
    }

    // A legacy Windows console has no title stack; the PowerShell wrapper
    // restores the title there instead.
    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }
}

impl Command for PopTitle {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        f.write_str("\x1b[23;0t")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn modes_parse_from_what_the_config_can_say() {
        assert_eq!(TitleMode::parse("off"), Some(TitleMode::Off));
        assert_eq!(TitleMode::parse(" Dir "), Some(TitleMode::Dir));
        assert_eq!(TitleMode::parse("PATH"), Some(TitleMode::Path));
        assert_eq!(TitleMode::parse("name"), None);
    }

    #[test]
    fn the_title_names_the_directory_or_its_path() {
        let cwd = PathBuf::from("projects").join("trail");
        assert_eq!(
            title_for(TitleMode::Dir, &cwd).as_deref(),
            Some("trail trail")
        );
        assert_eq!(
            title_for(TitleMode::Path, &cwd),
            Some(format!("trail {}", cwd.display()))
        );
        assert_eq!(title_for(TitleMode::Off, &cwd), None);
    }

    #[test]
    fn a_root_is_named_by_its_path() {
        let root = PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" });
        assert_eq!(
            title_for(TitleMode::Dir, &root),
            Some(format!("trail {}", root.display()))
        );
    }

    #[test]
    fn control_characters_never_reach_the_terminal() {
        let cwd = PathBuf::from("evil\x1b]0;pwned\x07name");
        assert_eq!(
            title_for(TitleMode::Dir, &cwd).as_deref(),
            Some("trail evil]0;pwnedname")
        );
    }

    #[test]
    fn the_title_is_written_once_per_change_and_restored() {
        let mut title = WindowTitle::default();
        let mut out = Vec::new();
        let here = PathBuf::from("here");

        title.update(&mut out, TitleMode::Dir, &here).unwrap();
        let first = String::from_utf8(out.clone()).unwrap();
        assert!(
            first.starts_with("\x1b[22;0t"),
            "saves the shell's title: {first:?}"
        );
        assert!(first.contains("trail here"), "{first:?}");

        out.clear();
        title.update(&mut out, TitleMode::Dir, &here).unwrap();
        assert!(out.is_empty(), "an unchanged title is not rewritten");

        title.forget();
        title.update(&mut out, TitleMode::Dir, &here).unwrap();
        let again = String::from_utf8(out.clone()).unwrap();
        assert!(
            again.contains("trail here") && !again.contains("\x1b[22"),
            "{again:?}"
        );

        out.clear();
        title.update(&mut out, TitleMode::Off, &here).unwrap();
        // Blanked first, for a terminal that ignores the restore after it.
        assert_eq!(
            String::from_utf8(out.clone()).unwrap(),
            "\x1b]0;\x07\x1b[23;0t"
        );

        out.clear();
        title.update(&mut out, TitleMode::Off, &here).unwrap();
        assert!(
            out.is_empty(),
            "off with nothing to give back writes nothing"
        );
    }
}
