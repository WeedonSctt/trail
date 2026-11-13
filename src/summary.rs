//! The summary printed to the shell when Trail exits: how long the session ran,
//! where it ended, what it changed, and what it left behind.
//!
//! [`SessionStats`] is counted on the UI thread as the session goes. At exit it
//! is read into a [`SessionReport`], a plain snapshot, and [`summary`] turns that
//! into lines, so what gets printed is tested without a terminal. Only what
//! Trail did itself is counted: a file a `!` command or a terminal panel shell
//! changed is invisible to it.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use crossterm::style::{Color, Stylize};

use crate::actions::run_report::RunResult;
use crate::actions::transfer::TransferKind;
use crate::app::state::AppState;
use crate::pathfmt;

/// How much `[general] exit_summary` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitSummary {
    /// Nothing, except the line explaining a wait for an unfinished copy.
    Off,
    /// Only the warnings, and nothing at all when there are none.
    Warnings,
    /// The first line, version, length and last folder, plus any warnings.
    Brief,
    /// Every field that is not zero.
    Full,
}

impl ExitSummary {
    /// Parses an `exit_summary` value, or `None` for one it does not know.
    pub fn parse(value: &str) -> Option<ExitSummary> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "never" => Some(ExitSummary::Off),
            "warnings" => Some(ExitSummary::Warnings),
            "brief" => Some(ExitSummary::Brief),
            "full" => Some(ExitSummary::Full),
            _ => None,
        }
    }
}

/// Why an `exit_summary` value was rejected, for `validate` and `:set`.
pub const EXIT_SUMMARY_REASON: &str = "must be off, warnings, brief or full";

/// A file Trail hands to another program, so the summary can count it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opens {
    /// The configured editor, which Trail waits for, so an edit can be told.
    Editor(PathBuf),
    /// The OS handler (`o`), which returns at once, so it is never an edit.
    OsHandler(PathBuf),
}

/// What the session has done so far, counted as it happens.
#[derive(Debug)]
pub struct SessionStats {
    started: Instant,
    /// `:` and `!` commands run, typed or repeated with `@`.
    pub commands: usize,
    /// Those of them that failed.
    pub failed_commands: usize,
    /// Entries copied, by `:cp` and `pp`.
    pub copied: usize,
    /// Entries moved, by `:mv` and `pm`.
    pub moved: usize,
    /// Entries renamed.
    pub renamed: usize,
    /// Folders and files created, by `:mkdir` and `:touch`.
    pub created: usize,
    /// Entries sent to the recycle bin.
    pub trashed: usize,
    /// Entries deleted for good.
    pub deleted: usize,
    opened: HashSet<PathBuf>,
    edited: HashSet<PathBuf>,
    used_marks: HashSet<PathBuf>,
}

impl Default for SessionStats {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            commands: 0,
            failed_commands: 0,
            copied: 0,
            moved: 0,
            renamed: 0,
            created: 0,
            trashed: 0,
            deleted: 0,
            opened: HashSet::new(),
            edited: HashSet::new(),
            used_marks: HashSet::new(),
        }
    }
}

impl SessionStats {
    /// Records a file handed to another program, from how the run went and the
    /// file's modification time before and after it.
    ///
    /// Counts only an open that happened: an editor that could not be started
    /// counts nothing, and neither does an OS handler that reported failure. A
    /// file counts once however often it is opened.
    pub fn record_open(
        &mut self,
        opens: &Opens,
        result: &RunResult,
        modified_before: Option<SystemTime>,
        modified_after: Option<SystemTime>,
    ) {
        match opens {
            Opens::Editor(path) => {
                if matches!(result, RunResult::CouldNotRun(_)) {
                    return;
                }
                self.opened.insert(path.clone());
                if modified_before.is_some() && modified_before != modified_after {
                    self.edited.insert(path.clone());
                }
            }
            Opens::OsHandler(path) => {
                if *result == RunResult::Success {
                    self.opened.insert(path.clone());
                }
            }
        }
    }

    /// Records marks used by a paste, `:cp`, `:mv` or `dd`. A copy keeps its
    /// marks, so the same set can be pasted twice; this is how they are still
    /// known to have been used.
    pub fn mark_used(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.used_marks.extend(paths);
    }

    /// How many of `marks` were never used.
    pub fn unused_marks<'a>(&self, marks: impl Iterator<Item = &'a PathBuf>) -> usize {
        marks
            .filter(|path| !self.used_marks.contains(*path))
            .count()
    }

    /// Files opened in another program this session.
    pub fn opened(&self) -> usize {
        self.opened.len()
    }

    /// Of those, files the editor changed.
    pub fn edited(&self) -> usize {
        self.edited.len()
    }
}

/// A copy or move still running when Trail exits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnfinishedTransfer {
    /// Copy or move.
    pub kind: TransferKind,
    /// Entries finished.
    pub done: usize,
    /// Entries in the job.
    pub total: usize,
    /// The folder they are going into, as a person reads it.
    pub into: String,
}

/// Everything [`summary`] needs, read out of the session at exit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionReport {
    /// The version label, as `trail --version` gives it.
    pub version: String,
    /// How long the session ran.
    pub elapsed: Duration,
    /// The folder it ended in, as a person reads it.
    pub last_dir: String,
    /// Whether it ended with a cancel, so the shell does not move.
    pub cancelled: bool,
    /// `:` and `!` commands run.
    pub commands: usize,
    /// Those that failed.
    pub failed_commands: usize,
    /// Files opened in another program.
    pub opened: usize,
    /// Of those, files the editor changed.
    pub edited: usize,
    /// Terminal panel shells started.
    pub shells_opened: usize,
    /// Of those, shells still open at exit.
    pub shells_open: usize,
    /// Shells running a command at exit, as (number, program).
    pub busy_shells: Vec<(usize, String)>,
    /// Entries copied.
    pub copied: usize,
    /// Entries moved.
    pub moved: usize,
    /// Entries renamed.
    pub renamed: usize,
    /// Folders and files created.
    pub created: usize,
    /// Entries sent to the recycle bin.
    pub trashed: usize,
    /// Entries deleted for good.
    pub deleted: usize,
    /// Marks never used by any operation.
    pub unused_marks: usize,
    /// Copies and moves still running.
    pub transfers: Vec<UnfinishedTransfer>,
}

impl SessionReport {
    /// Reads the session out of `state` at exit.
    ///
    /// `check_shells` asks the process table which shells are busy, which takes
    /// tens of milliseconds; it is left off when nothing will show the answer.
    pub fn from_state(state: &AppState, cancelled: bool, check_shells: bool) -> SessionReport {
        let stats = &state.stats;
        SessionReport {
            version: crate::version::VERSION.to_owned(),
            elapsed: stats.started.elapsed(),
            last_dir: pathfmt::display(&state.cwd),
            cancelled,
            commands: stats.commands,
            failed_commands: stats.failed_commands,
            opened: stats.opened(),
            edited: stats.edited(),
            shells_opened: state.terminal.shells_started(),
            shells_open: state.terminal.sessions().len(),
            busy_shells: if check_shells {
                state.terminal.running_commands()
            } else {
                Vec::new()
            },
            copied: stats.copied,
            moved: stats.moved,
            renamed: stats.renamed,
            created: stats.created,
            trashed: stats.trashed,
            deleted: stats.deleted,
            unused_marks: stats.unused_marks(state.marks.paths()),
            transfers: state
                .transfers
                .active()
                .iter()
                .map(|t| UnfinishedTransfer {
                    kind: t.kind,
                    done: t.done,
                    total: t.total,
                    into: pathfmt::display(&t.into),
                })
                .collect(),
        }
    }
}

/// The lines to print for `report` at `level`, fitted to `width` columns.
///
/// Zeroes are left out, so a quick look around prints one line at `full`. Each
/// warning line starts with `!`, so it reads as one without colour. A copy or
/// move still running gets its own line at every level, `off` included: it
/// explains a wait that happens anyway.
pub fn summary(report: &SessionReport, level: ExitSummary, width: u16) -> Vec<String> {
    let width = usize::from(width).max(20);
    let mut lines = Vec::new();
    if level == ExitSummary::Off {
        lines.extend(transfer_lines(report, width));
        return lines;
    }

    if matches!(level, ExitSummary::Brief | ExitSummary::Full) {
        lines.push(first_line(report, width));
    }
    if level == ExitSummary::Full {
        lines.extend(joined(activity(report)));
        lines.extend(joined(operations(report)));
    }
    let warnings = warnings(report, level);
    if !warnings.is_empty() {
        lines.push(format!("! {}", warnings.join(" · ")));
    }
    lines.extend(transfer_lines(report, width));
    lines
}

/// Joins the lines for printing, warnings in `colour` when there is one.
pub fn paint(lines: &[String], colour: Option<Color>) -> String {
    lines
        .iter()
        .map(|line| match colour {
            Some(colour) if line.starts_with("! ") => line.as_str().with(colour).to_string(),
            _ => line.clone(),
        })
        .map(|line| line + "\n")
        .collect()
}

/// `trail 1.15.0 · 42m 10s · ended in C:\…\trail`, the path shortened to fit.
fn first_line(report: &SessionReport, width: usize) -> String {
    let head = format!(
        "trail {} · {} · ",
        report.version,
        format_length(report.elapsed)
    );
    let (before, after) = if report.cancelled {
        ("still in ", " (shell not moved)")
    } else {
        ("ended in ", "")
    };
    let used = head.chars().count() + before.len() + after.len();
    // A line one column short of the width, so a terminal that wraps at the last
    // column does not push the next one down.
    let room = width.saturating_sub(used + 1).max(10);
    let path = pathfmt::elide_front(&report.last_dir, room);
    format!("{head}{before}{path}{after}")
}

/// Commands, files and shells: `12 commands (1 failed) · 3 files opened, 2 edited`.
fn activity(report: &SessionReport) -> Vec<String> {
    let mut parts = Vec::new();
    if report.commands > 0 {
        let mut text = count(report.commands, "command", "commands");
        if report.failed_commands > 0 {
            text.push_str(&format!(" ({} failed)", report.failed_commands));
        }
        parts.push(text);
    }
    if report.opened > 0 {
        let mut text = format!("{} opened", count(report.opened, "file", "files"));
        if report.edited > 0 {
            text.push_str(&format!(", {} edited", report.edited));
        }
        parts.push(text);
    }
    if report.shells_opened > 0 {
        let mut text = format!("{} opened", count(report.shells_opened, "shell", "shells"));
        if report.shells_open > 0 {
            text.push_str(&format!(", {} open at exit", report.shells_open));
        }
        parts.push(text);
    }
    parts
}

/// What changed on disk: `5 copied · 2 moved · 3 deleted (recycle bin)`.
fn operations(report: &SessionReport) -> Vec<String> {
    [
        (report.copied, "copied"),
        (report.moved, "moved"),
        (report.renamed, "renamed"),
        (report.created, "created"),
        (report.trashed, "deleted (recycle bin)"),
        (report.deleted, "deleted permanently"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, what)| format!("{n} {what}"))
    .collect()
}

/// What needs attention. At `full` a failed command is already on the line
/// above, so it is not said twice.
fn warnings(report: &SessionReport, level: ExitSummary) -> Vec<String> {
    let mut parts: Vec<String> = report
        .busy_shells
        .iter()
        .map(|(number, program)| format!("shell {number} was still running {program}"))
        .collect();
    if report.failed_commands > 0 && level != ExitSummary::Full {
        parts.push(format!(
            "{} failed",
            count(report.failed_commands, "command", "commands")
        ));
    }
    if report.unused_marks > 0 {
        parts.push(format!(
            "{} never used",
            count(report.unused_marks, "mark", "marks")
        ));
    }
    parts
}

/// One line for each copy or move still running.
fn transfer_lines(report: &SessionReport, width: usize) -> Vec<String> {
    const TAIL: &str =
        " — waiting for it to finish (Ctrl-c stops it, and may leave a file half-copied)";
    report
        .transfers
        .iter()
        .map(|t| {
            let head = format!("! {} {}/{} into ", t.kind.progressive(), t.done, t.total);
            let room = width
                .saturating_sub(head.chars().count() + TAIL.chars().count() + 1)
                .max(10);
            format!("{head}{}{TAIL}", pathfmt::elide_front(&t.into, room))
        })
        .collect()
}

/// The parts joined into one line, or nothing when there are none.
fn joined(parts: Vec<String>) -> Option<String> {
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// `1 command`, `12 commands`.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A session's length: `8s`, `42m 10s`, `1h 05m`.
fn format_length(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet() -> SessionReport {
        SessionReport {
            version: "1.15.0".to_owned(),
            elapsed: Duration::from_secs(192),
            last_dir: "C:\\Users\\weedo\\proj\\it\\util\\trail".to_owned(),
            ..SessionReport::default()
        }
    }

    fn busy() -> SessionReport {
        SessionReport {
            elapsed: Duration::from_secs(42 * 60 + 10),
            commands: 12,
            failed_commands: 1,
            opened: 3,
            edited: 2,
            shells_opened: 4,
            shells_open: 2,
            busy_shells: vec![(2, "cargo".to_owned())],
            copied: 5,
            moved: 2,
            trashed: 3,
            unused_marks: 4,
            ..quiet()
        }
    }

    #[test]
    fn levels_parse_from_what_the_config_can_say() {
        assert_eq!(ExitSummary::parse("off"), Some(ExitSummary::Off));
        assert_eq!(
            ExitSummary::parse(" Warnings "),
            Some(ExitSummary::Warnings)
        );
        assert_eq!(ExitSummary::parse("brief"), Some(ExitSummary::Brief));
        assert_eq!(ExitSummary::parse("FULL"), Some(ExitSummary::Full));
        assert_eq!(ExitSummary::parse("loud"), None);
        assert_eq!(ExitSummary::parse(""), None);
    }

    #[test]
    fn a_quick_look_around_is_one_line_at_full() {
        let lines = summary(&quiet(), ExitSummary::Full, 120);
        assert_eq!(
            lines,
            vec!["trail 1.15.0 · 3m 12s · ended in C:\\Users\\weedo\\proj\\it\\util\\trail"]
        );
    }

    #[test]
    fn a_busy_session_is_four_lines_at_full() {
        let lines = summary(&busy(), ExitSummary::Full, 120);
        assert_eq!(
            lines,
            vec![
                "trail 1.15.0 · 42m 10s · ended in C:\\Users\\weedo\\proj\\it\\util\\trail",
                "12 commands (1 failed) · 3 files opened, 2 edited · 4 shells opened, 2 open at exit",
                "5 copied · 2 moved · 3 deleted (recycle bin)",
                "! shell 2 was still running cargo · 4 marks never used",
            ]
        );
    }

    #[test]
    fn each_level_against_the_same_session() {
        let report = busy();
        assert_eq!(
            summary(&report, ExitSummary::Off, 120),
            Vec::<String>::new()
        );
        assert_eq!(
            summary(&report, ExitSummary::Warnings, 120),
            vec!["! shell 2 was still running cargo · 1 command failed · 4 marks never used"]
        );
        let brief = summary(&report, ExitSummary::Brief, 120);
        assert_eq!(brief.len(), 2);
        assert!(brief[0].starts_with("trail 1.15.0 · 42m 10s"));
        assert_eq!(
            brief[1],
            "! shell 2 was still running cargo · 1 command failed · 4 marks never used"
        );
    }

    #[test]
    fn warnings_says_nothing_when_nothing_needs_attention() {
        assert!(summary(&quiet(), ExitSummary::Warnings, 120).is_empty());
        assert_eq!(summary(&quiet(), ExitSummary::Brief, 120).len(), 1);
    }

    #[test]
    fn a_cancel_says_the_shell_did_not_move() {
        let report = SessionReport {
            cancelled: true,
            ..quiet()
        };
        let line = &summary(&report, ExitSummary::Brief, 120)[0];
        assert!(
            line.ends_with("still in C:\\Users\\weedo\\proj\\it\\util\\trail (shell not moved)"),
            "{line}"
        );
    }

    #[test]
    fn each_warning_on_its_own() {
        let one = |report: SessionReport| summary(&report, ExitSummary::Warnings, 120);
        assert_eq!(
            one(SessionReport {
                busy_shells: vec![(1, "nvim".to_owned())],
                ..quiet()
            }),
            vec!["! shell 1 was still running nvim"]
        );
        assert_eq!(
            one(SessionReport {
                unused_marks: 1,
                ..quiet()
            }),
            vec!["! 1 mark never used"]
        );
        assert_eq!(
            one(SessionReport {
                commands: 3,
                failed_commands: 2,
                ..quiet()
            }),
            vec!["! 2 commands failed"]
        );
    }

    #[test]
    fn an_unfinished_copy_is_said_even_when_the_summary_is_off() {
        let report = SessionReport {
            transfers: vec![UnfinishedTransfer {
                kind: TransferKind::Copy,
                done: 3,
                total: 10,
                into: "D:\\backup".to_owned(),
            }],
            ..quiet()
        };
        let lines = summary(&report, ExitSummary::Off, 120);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("! copying 3/10 into D:\\backup — waiting for it to finish"),
            "{}",
            lines[0]
        );
        assert!(lines[0].contains("Ctrl-c"));
    }

    #[test]
    fn a_long_path_is_shortened_to_the_width_keeping_its_end() {
        let report = SessionReport {
            last_dir: format!("C:\\{}\\trail", "deep\\".repeat(30)),
            ..quiet()
        };
        let line = &summary(&report, ExitSummary::Brief, 60)[0];
        assert!(line.chars().count() < 60, "{line}");
        assert!(line.contains('…') && line.ends_with("\\trail"), "{line}");
    }

    #[test]
    fn colour_goes_only_on_warnings_and_only_when_asked() {
        let lines = summary(&busy(), ExitSummary::Full, 120);
        let plain = paint(&lines, None);
        assert!(!plain.contains('\u{1b}'), "{plain:?}");
        assert_eq!(plain.lines().count(), 4);

        let coloured = paint(&lines, Some(Color::Red));
        let escaped: Vec<&str> = coloured
            .lines()
            .filter(|line| line.contains('\u{1b}'))
            .collect();
        assert_eq!(escaped.len(), 1);
        assert!(escaped[0].contains("never used"));
    }

    #[test]
    fn lengths_read_the_way_a_person_says_them() {
        assert_eq!(format_length(Duration::from_secs(8)), "8s");
        assert_eq!(format_length(Duration::from_secs(42 * 60 + 10)), "42m 10s");
        assert_eq!(format_length(Duration::from_secs(3900)), "1h 05m");
    }

    #[test]
    fn a_failed_editor_launch_counts_nothing() {
        let mut stats = SessionStats::default();
        let path = PathBuf::from("/work/a.txt");
        stats.record_open(
            &Opens::Editor(path.clone()),
            &RunResult::CouldNotRun("no such program".to_owned()),
            None,
            None,
        );
        stats.record_open(
            &Opens::OsHandler(path),
            &RunResult::Failed { code: Some(1) },
            None,
            None,
        );
        assert_eq!((stats.opened(), stats.edited()), (0, 0));
    }

    #[test]
    fn an_editor_that_changed_the_file_counts_an_edit_once() {
        let mut stats = SessionStats::default();
        let path = PathBuf::from("/work/a.txt");
        let before = SystemTime::UNIX_EPOCH;
        let after = before + Duration::from_secs(5);
        let editor = Opens::Editor(path.clone());
        stats.record_open(&editor, &RunResult::Success, Some(before), Some(after));
        stats.record_open(&editor, &RunResult::Success, Some(after), Some(after));
        // An editor that exits non-zero still had the file open.
        stats.record_open(
            &Opens::Editor(PathBuf::from("/work/b.txt")),
            &RunResult::Failed { code: Some(1) },
            Some(before),
            Some(before),
        );
        assert_eq!((stats.opened(), stats.edited()), (2, 1));
    }

    #[test]
    fn the_os_handler_opens_but_never_edits() {
        let mut stats = SessionStats::default();
        let before = SystemTime::UNIX_EPOCH;
        stats.record_open(
            &Opens::OsHandler(PathBuf::from("/work/a.png")),
            &RunResult::Success,
            Some(before),
            Some(before + Duration::from_secs(1)),
        );
        assert_eq!((stats.opened(), stats.edited()), (1, 0));
    }

    #[test]
    fn a_used_mark_is_not_unused_even_after_a_copy_keeps_it() {
        let mut stats = SessionStats::default();
        let marks = [PathBuf::from("/a"), PathBuf::from("/b")];
        stats.mark_used([marks[0].clone()]);
        assert_eq!(stats.unused_marks(marks.iter()), 1);
    }
}
