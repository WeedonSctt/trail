//! What became of an external command: the result block printed under its
//! output, the key that answers the block, and the status bar's report.
//!
//! Everything here is pure, so it can be tested without a terminal; the printing
//! and the waiting are `shell_exec`'s. The block is designed in
//! `upcoming_features.md` §2.25:
//!
//! ```text
//! ── !cargo test ───────────────────── failed: exit 101 · 12.4s
//! C:\Users\weedo\proj\it\util\trail
//! r: run again · Enter: back to Trail
//! ─────────────────────────────────────────────────────────────
//! ```

use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::input::is_text_modifiers;
use crate::pathfmt;
use crate::preview::provider::sanitize;

/// The line that says what can be done next.
const BLOCK_KEYS: &str = "r: run again · Enter: back to Trail";

/// How a command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    /// It exited with status zero.
    Success,
    /// It exited with a failure. `code` is `None` when there is no exit code,
    /// which is how a process killed by a signal looks on Unix.
    ///
    /// Commands run through `[general] shell`, so this is the shell's code:
    /// PowerShell's `-Command` can report a failed tool as plain `1`.
    Failed {
        /// The exit code, when the platform reported one.
        code: Option<i32>,
    },
    /// It never ran to an exit: the program could not be started, or waiting
    /// for it failed. Holds the operating system's reason.
    CouldNotRun(String),
}

/// One run of an external command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    /// The command as typed, `!cargo test` or `:git status`. `None` for what
    /// Trail starts on the user's behalf — the editor, the OS file handler —
    /// which report only when they fail.
    pub label: Option<String>,
    /// The program spawned, which names the run when there is no `label`.
    pub program: String,
    /// How it ended.
    pub result: RunResult,
    /// How long it ran.
    pub elapsed: Duration,
}

impl RunReport {
    /// Whether the command succeeded.
    pub fn succeeded(&self) -> bool {
        self.result == RunResult::Success
    }

    /// The result in words: `ok`, or `failed: exit 101`.
    ///
    /// The words carry the meaning, so it reads correctly without colour.
    pub fn outcome(&self) -> String {
        match &self.result {
            RunResult::Success => "ok".to_owned(),
            RunResult::Failed { code: Some(code) } => format!("failed: exit {code}"),
            RunResult::Failed { code: None } => "failed: terminated".to_owned(),
            RunResult::CouldNotRun(_) => "failed: could not start".to_owned(),
        }
    }

    /// What names this run on screen: the command as typed, else the program.
    ///
    /// Sanitized, because a pasted command may hold control characters, and
    /// this is printed straight to the terminal.
    fn name(&self) -> String {
        sanitize(self.label.as_deref().unwrap_or(&self.program))
    }

    /// The status bar's report, or `None` when there is nothing to say: the
    /// editor and the OS file handler report only a failure.
    ///
    /// Deliberately independent of `[general] shell_pause`. The block is out of
    /// sight as soon as the alternate screen returns, and under `never` this is
    /// the only report there is.
    pub fn status_line(&self) -> Option<String> {
        if self.label.is_none() && self.succeeded() {
            return None;
        }
        let name = self.name();
        let outcome = self.outcome();
        Some(match &self.result {
            RunResult::CouldNotRun(reason) => format!("{name} {outcome}: {reason}"),
            _ => format!("{name} {outcome} ({})", format_elapsed(self.elapsed)),
        })
    }
}

/// Spells a duration the way the block and the status bar show it: `0.3s`,
/// `12.4s`, `2m 05s`, `1h 02m`.
pub fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => format!("{:.1}s", elapsed.as_secs_f64()),
        60..=3599 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

/// What a key means while the result block is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockAnswer {
    /// Run the same command again, in the same folder.
    RunAgain,
    /// Go back to Trail.
    Return,
    /// Not an answer; keep waiting.
    Ignore,
}

/// Decides what `key` means at the result block.
///
/// Only presses count: Windows also reports key releases, and the release of
/// the Enter that launched the command must not read as an answer. `R` is
/// accepted so Caps Lock does not get in the way, but `Ctrl-r` is a chord, not
/// the letter.
///
/// This cannot tell a held `r` from a new one, because Windows reports a held
/// key as more presses, not as `Repeat`. [`RerunKey`] is what tells them apart.
pub fn answer_for(key: &KeyEvent) -> BlockAnswer {
    if key.kind != KeyEventKind::Press {
        return BlockAnswer::Ignore;
    }
    match key.code {
        KeyCode::Char('r' | 'R') if is_text_modifiers(key.modifiers) => BlockAnswer::RunAgain,
        KeyCode::Enter | KeyCode::Esc => BlockAnswer::Return,
        _ => BlockAnswer::Ignore,
    }
}

/// How long `r` must have been silent before a press with no release in between
/// counts as new.
///
/// Longer than the wait before a key starts repeating, which Windows lets the
/// user set to at most a second. Otherwise, after a command that finishes
/// quickly, a held key's first repeat would look like a second press.
const RERUN_KEY_QUIET: Duration = Duration::from_millis(1100);

/// Tracks whether `r` is still down, so that holding it runs the command once.
///
/// Holding a key reaches Trail as a stream of presses, on Windows and on any
/// terminal that reports no key kinds, so the block has to work this out itself.
/// A press counts only if `r` came up first. A release says so directly. Where
/// none arrives, either because the terminal does not report them or because the
/// command read it from the console, a long enough silence says so instead. That
/// makes `r` tapped again within about a second do nothing on those terminals: the
/// safe way to be wrong, since the command may be `!del old.log`.
///
/// One value lasts across every block of a run, and it must see every key, those
/// thrown away as typed too early included, or it loses track of the key.
#[derive(Debug, Default)]
pub struct RerunKey {
    /// When `r` was last seen going down, while it is thought to still be down.
    last_seen_down: Option<Instant>,
}

impl RerunKey {
    /// Decides what `key`, received at `now`, means at the result block.
    pub fn answer(&mut self, key: &KeyEvent, now: Instant) -> BlockAnswer {
        let answer = answer_for(key);
        if !matches!(key.code, KeyCode::Char('r' | 'R')) {
            return answer;
        }
        if key.kind == KeyEventKind::Release {
            self.last_seen_down = None;
            return answer;
        }
        let still_down = self
            .last_seen_down
            .is_some_and(|seen| now.saturating_duration_since(seen) < RERUN_KEY_QUIET);
        self.last_seen_down = Some(now);
        if still_down {
            BlockAnswer::Ignore
        } else {
            answer
        }
    }
}

/// The result block, laid out for a terminal of a given width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultBlock {
    /// The start of the header, up to the result: `── !cargo test ─────── `.
    pub header_start: String,
    /// The result in words, printed in green or red.
    pub outcome: String,
    /// The end of the header: ` · 12.4s`.
    pub header_end: String,
    /// Whether to print `outcome` as a success.
    pub succeeded: bool,
    /// Why a command could not start, which nothing else on screen would say.
    pub reason: Option<String>,
    /// The folder it ran in.
    pub folder: String,
    /// What can be done next.
    pub keys: &'static str,
    /// The rule that ends the block, which keeps runs apart.
    pub closing: String,
}

impl ResultBlock {
    /// Lays out the block for `report`, run in `cwd`, in `columns` columns.
    ///
    /// Everything stays one column short of the width: a line that exactly fills
    /// the row leaves a blank line after it on some Windows consoles. Columns
    /// are counted in characters, as the command line counts them.
    pub fn new(report: &RunReport, cwd: &Path, columns: u16) -> Self {
        let width = usize::from(columns).saturating_sub(1);
        let outcome = report.outcome();
        let header_end = format!(" · {}", format_elapsed(report.elapsed));

        // `── name ─ outcome · time`: three columns before the name, then a
        // space, at least one column of rule, and a space.
        let tail = outcome.chars().count() + header_end.chars().count();
        let room = width.saturating_sub(tail + 6);
        let name = shorten(&report.name(), room);
        let rule = width.saturating_sub(tail + 5 + name.chars().count()).max(1);

        ResultBlock {
            header_start: format!("── {name} {} ", "─".repeat(rule)),
            outcome,
            header_end,
            succeeded: report.succeeded(),
            reason: match &report.result {
                RunResult::CouldNotRun(reason) => Some(sanitize(reason)),
                _ => None,
            },
            folder: pathfmt::elide_front(&pathfmt::display(cwd), width),
            keys: BLOCK_KEYS,
            closing: "─".repeat(width),
        }
    }

    /// The block as plain lines, top to bottom.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "{}{}{}",
            self.header_start, self.outcome, self.header_end
        )];
        lines.extend(self.reason.clone());
        lines.push(self.folder.clone());
        lines.push(self.keys.to_owned());
        lines.push(self.closing.clone());
        lines
    }
}

/// Fits `text` into `width` columns, marking anything dropped with `…`.
fn shorten(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    if width == 0 {
        kept
    } else {
        format!("{kept}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventState, KeyModifiers};

    fn report(result: RunResult) -> RunReport {
        RunReport {
            label: Some("!cargo test".to_owned()),
            program: "cmd.exe".to_owned(),
            result,
            elapsed: Duration::from_millis(12_400),
        }
    }

    fn key(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn r_runs_again_and_enter_or_esc_go_back() {
        let press = |code| answer_for(&key(code, KeyEventKind::Press));
        assert_eq!(press(KeyCode::Char('r')), BlockAnswer::RunAgain);
        assert_eq!(
            press(KeyCode::Char('R')),
            BlockAnswer::RunAgain,
            "Caps Lock must not get in the way"
        );
        assert_eq!(press(KeyCode::Enter), BlockAnswer::Return);
        assert_eq!(press(KeyCode::Esc), BlockAnswer::Return);
        assert_eq!(press(KeyCode::Char('x')), BlockAnswer::Ignore);
        assert_eq!(press(KeyCode::Char(' ')), BlockAnswer::Ignore);
        assert_eq!(press(KeyCode::Down), BlockAnswer::Ignore);
    }

    /// The release of the Enter that launched the command arrives after the
    /// command has started, and must not dismiss its result.
    #[test]
    fn only_a_press_is_an_answer() {
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            for code in [KeyCode::Char('r'), KeyCode::Enter, KeyCode::Esc] {
                assert_eq!(
                    answer_for(&key(code, kind)),
                    BlockAnswer::Ignore,
                    "{code:?} {kind:?}"
                );
            }
        }
    }

    fn r(kind: KeyEventKind) -> KeyEvent {
        key(KeyCode::Char('r'), kind)
    }

    /// What Windows sends for a held key: presses, and no `Repeat`. The gaps are
    /// its default wait before repeating, then its default repeat rate.
    #[test]
    fn holding_r_is_one_answer_however_long_it_is_held() {
        let start = Instant::now();
        let mut rerun = RerunKey::default();

        let first = rerun.answer(&r(KeyEventKind::Press), start);
        assert_eq!(first, BlockAnswer::RunAgain);

        let mut now = start + Duration::from_millis(500);
        for _ in 0..100 {
            let repeat = rerun.answer(&r(KeyEventKind::Press), now);
            assert_eq!(repeat, BlockAnswer::Ignore);
            now += Duration::from_millis(33);
        }
    }

    /// The longest wait Windows can be set to before a key starts repeating.
    #[test]
    fn the_first_repeat_of_a_slow_keyboard_is_still_the_same_hold() {
        let start = Instant::now();
        let mut rerun = RerunKey::default();
        rerun.answer(&r(KeyEventKind::Press), start);

        let repeat = rerun.answer(&r(KeyEventKind::Press), start + Duration::from_secs(1));
        assert_eq!(repeat, BlockAnswer::Ignore);
    }

    #[test]
    fn r_works_again_at_once_after_a_release() {
        let start = Instant::now();
        let mut rerun = RerunKey::default();
        rerun.answer(&r(KeyEventKind::Press), start);
        let released = rerun.answer(&r(KeyEventKind::Release), start);

        assert_eq!(released, BlockAnswer::Ignore);
        assert_eq!(
            rerun.answer(&r(KeyEventKind::Press), start),
            BlockAnswer::RunAgain
        );
    }

    /// Terminals that report no releases, and a command that read the release
    /// from the console: `r` must not stay dead for good.
    #[test]
    fn r_works_again_after_silence_when_no_release_arrives() {
        let start = Instant::now();
        let mut rerun = RerunKey::default();
        rerun.answer(&r(KeyEventKind::Press), start);

        let later = start + RERUN_KEY_QUIET;
        assert_eq!(
            rerun.answer(&r(KeyEventKind::Press), later),
            BlockAnswer::RunAgain
        );
    }

    /// `r` held through a long command, whose repeats were thrown away as typed
    /// too early: the block that follows must not take the next one as an answer.
    #[test]
    fn a_hold_seen_among_discarded_keys_still_counts_as_held() {
        let start = Instant::now();
        let mut rerun = RerunKey::default();
        rerun.answer(&r(KeyEventKind::Press), start);

        let next = rerun.answer(&r(KeyEventKind::Press), start + Duration::from_millis(33));
        assert_eq!(next, BlockAnswer::Ignore);
    }

    /// `r` tapped during a command, then tapped at its block.
    #[test]
    fn a_tap_during_the_command_does_not_block_the_next_one() {
        let start = Instant::now();
        let mut rerun = RerunKey::default();
        rerun.answer(&r(KeyEventKind::Press), start);
        rerun.answer(&r(KeyEventKind::Release), start);

        assert_eq!(
            rerun.answer(&r(KeyEventKind::Press), start),
            BlockAnswer::RunAgain
        );
    }

    #[test]
    fn other_keys_answer_while_r_is_held() {
        let start = Instant::now();
        let mut rerun = RerunKey::default();
        rerun.answer(&r(KeyEventKind::Press), start);

        let enter = key(KeyCode::Enter, KeyEventKind::Press);
        assert_eq!(rerun.answer(&enter, start), BlockAnswer::Return);
        let other = key(KeyCode::Char('x'), KeyEventKind::Press);
        assert_eq!(rerun.answer(&other, start), BlockAnswer::Ignore);
    }

    #[test]
    fn ctrl_r_is_a_chord_not_the_letter() {
        let chord = KeyEvent {
            modifiers: KeyModifiers::CONTROL,
            ..key(KeyCode::Char('r'), KeyEventKind::Press)
        };
        assert_eq!(answer_for(&chord), BlockAnswer::Ignore);
    }

    #[test]
    fn the_block_says_what_happened_on_success() {
        let block = ResultBlock::new(&report(RunResult::Success), Path::new("/work"), 61);
        let lines = block.lines();

        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("── !cargo test ─"), "{}", lines[0]);
        assert!(lines[0].ends_with("─ ok · 12.4s"), "{}", lines[0]);
        assert_eq!(lines[1], crate::pathfmt::display(Path::new("/work")));
        assert_eq!(lines[2], "r: run again · Enter: back to Trail");
        assert_eq!(lines[3], "─".repeat(60));
        assert!(block.succeeded);
    }

    #[test]
    fn the_block_says_what_happened_on_failure() {
        let block = ResultBlock::new(
            &report(RunResult::Failed { code: Some(101) }),
            Path::new("/work"),
            61,
        );
        assert!(
            block.lines()[0].ends_with("─ failed: exit 101 · 12.4s"),
            "{}",
            block.lines()[0]
        );
        assert!(!block.succeeded);
    }

    #[test]
    fn a_failed_spawn_says_why_on_a_line_of_its_own() {
        let block = ResultBlock::new(
            &report(RunResult::CouldNotRun("program not found".to_owned())),
            Path::new("/work"),
            61,
        );
        let lines = block.lines();
        assert!(lines[0].contains("failed: could not start"), "{}", lines[0]);
        assert_eq!(lines[1], "program not found");
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn a_long_command_is_shortened_to_the_width_and_keeps_the_result() {
        let mut long = report(RunResult::Failed { code: Some(1) });
        long.label = Some(format!("!{}", "x".repeat(300)));
        let header = ResultBlock::new(&long, Path::new("/work"), 80).lines()[0].clone();

        assert_eq!(header.chars().count(), 79, "{header}");
        assert!(header.contains('…'), "{header}");
        assert!(header.ends_with("failed: exit 1 · 12.4s"), "{header}");
    }

    #[test]
    fn every_header_fills_the_same_width_as_the_closing_rule() {
        let block = ResultBlock::new(&report(RunResult::Success), Path::new("/work"), 61);
        let lines = block.lines();
        assert_eq!(lines[0].chars().count(), lines[3].chars().count());
    }

    /// A terminal too narrow for any of it must not panic or lose the result.
    #[test]
    fn a_tiny_terminal_still_lays_out() {
        for columns in [0, 1, 5, 20] {
            let lines =
                ResultBlock::new(&report(RunResult::Success), Path::new("/w"), columns).lines();
            assert!(lines[0].contains("ok · 12.4s"), "{columns}: {}", lines[0]);
        }
    }

    #[test]
    fn a_control_character_in_a_command_cannot_reach_the_terminal() {
        let mut pasted = report(RunResult::Success);
        pasted.label = Some("!echo \u{1b}[2Jhi".to_owned());
        let header = ResultBlock::new(&pasted, Path::new("/w"), 80).lines()[0].clone();
        assert!(!header.contains('\u{1b}'), "{header:?}");
    }

    #[test]
    fn the_status_line_reports_a_command_either_way() {
        assert_eq!(
            report(RunResult::Failed { code: Some(101) }).status_line(),
            Some("!cargo test failed: exit 101 (12.4s)".to_owned())
        );
        assert_eq!(
            report(RunResult::Success).status_line(),
            Some("!cargo test ok (12.4s)".to_owned())
        );
    }

    #[test]
    fn the_editor_reports_only_a_failure() {
        let mut editor = report(RunResult::Success);
        editor.label = None;
        editor.program = "nvim".to_owned();
        assert_eq!(editor.status_line(), None);

        editor.result = RunResult::Failed { code: Some(1) };
        assert_eq!(
            editor.status_line(),
            Some("nvim failed: exit 1 (12.4s)".to_owned())
        );
    }

    #[test]
    fn a_process_with_no_exit_code_still_reads_as_failed() {
        let outcome = report(RunResult::Failed { code: None }).outcome();
        assert_eq!(outcome, "failed: terminated");
    }

    #[test]
    fn durations_are_spelled_for_a_person() {
        assert_eq!(format_elapsed(Duration::from_millis(300)), "0.3s");
        assert_eq!(format_elapsed(Duration::from_millis(12_400)), "12.4s");
        assert_eq!(format_elapsed(Duration::from_secs(125)), "2m 05s");
        assert_eq!(format_elapsed(Duration::from_secs(3720)), "1h 02m");
    }
}
