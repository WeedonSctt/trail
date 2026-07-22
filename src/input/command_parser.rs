//! Command Mode grammar: parsing, history, completion, and validation.
//!
//! Handles the `:` and `!`-prefixed command grammar including `:mkdir`,
//! `:touch`, `:rename`, `:mv`, `:cp`, `:git`, `:set`, and `!<shell>`.
//!
//! # Grammar
//!
//! ```text
//! command ::= ":" verb (" " arg)*
//!           | "!" shell_string
//!
//! verb    ::= "mkdir" | "touch" | "rename" | "mv" | "cp" | "git" | "set"
//! ```
//!
//! `:git` and `:set` are syntactically accepted and validated here; their
//! real backing (the git worker, the config schema) lands in Phases 4 and 7.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

// ── Parsed command ─────────────────────────────────────────────────────────────

/// A successfully parsed, validated Command Mode input.
///
/// Each variant carries exactly the arguments it needs; the parser rejects
/// inputs that don't match the expected arity or content constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    /// Create a directory with the given name inside the current directory.
    Mkdir(String),
    /// Create an empty file with the given name inside the current directory.
    Touch(String),
    /// Rename the currently selected entry to `new_name`.
    Rename(String),
    /// Move the selected entry to `dest` (relative or absolute path).
    Mv(String),
    /// Copy the selected entry to `dest` (relative or absolute path).
    Cp(String),
    /// Run a git subcommand string (e.g. `"init"`, `"status --short"`).
    /// The git worker will be wired in Phase 4; syntactically accepted now.
    Git(String),
    /// Set a runtime config key to a value.
    /// The config schema will be wired in Phase 7; syntactically accepted now.
    Set { key: String, value: String },
    /// Execute an arbitrary shell command string.
    Shell(String),
}

// ── Parse error ────────────────────────────────────────────────────────────────

/// Validation errors produced during command parsing.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// An unrecognised command verb was entered.
    #[error("unknown command '{0}' — try :mkdir, :touch, :rename, :mv, :cp, :git, :set")]
    UnknownVerb(String),
    /// A required argument was not provided.
    #[error("{0} requires an argument")]
    MissingArgument(String),
    /// The argument failed a content constraint (e.g. `/` in a rename).
    #[error("{0}")]
    InvalidArgument(String),
    /// The command buffer is empty.
    #[error("empty command")]
    Empty,
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse `buffer` (the text typed after `:` or `!`) into a [`ParsedCommand`].
///
/// The buffer should **not** include the leading `:` or `!` sentinel — the
/// caller strips it before calling here. The `!` prefix is preserved as a
/// flag argument to distinguish shell execution from `:` commands.
///
/// # Errors
///
/// Returns [`ParseError`] if the buffer is empty, contains an unknown verb,
/// is missing a required argument, or fails content validation.
pub fn parse(buffer: &str, is_shell: bool) -> Result<ParsedCommand, ParseError> {
    if is_shell {
        let cmd = buffer.trim();
        if cmd.is_empty() {
            return Err(ParseError::MissingArgument("!".to_owned()));
        }
        return Ok(ParsedCommand::Shell(cmd.to_owned()));
    }

    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        return Err(ParseError::Empty);
    }

    // Split into verb + rest. The rest after the first word is the argument(s).
    let (verb, rest) = match trimmed.split_once(' ') {
        Some((v, r)) => (v, r.trim()),
        None => (trimmed, ""),
    };

    match verb {
        "mkdir" => {
            if rest.is_empty() {
                return Err(ParseError::MissingArgument("mkdir".to_owned()));
            }
            // Directory names cannot be empty; reject embedded path separators
            // to prevent accidental deep creation.
            let name = rest.to_owned();
            if name.contains('/') || name.contains('\\') {
                return Err(ParseError::InvalidArgument(
                    "mkdir: name must not contain path separators".to_owned(),
                ));
            }
            Ok(ParsedCommand::Mkdir(name))
        }

        "touch" => {
            if rest.is_empty() {
                return Err(ParseError::MissingArgument("touch".to_owned()));
            }
            let name = rest.to_owned();
            if name.contains('/') || name.contains('\\') {
                return Err(ParseError::InvalidArgument(
                    "touch: name must not contain path separators".to_owned(),
                ));
            }
            Ok(ParsedCommand::Touch(name))
        }

        "rename" | "ren" => {
            if rest.is_empty() {
                return Err(ParseError::MissingArgument("rename".to_owned()));
            }
            let new_name = rest.to_owned();
            if new_name.contains('/') || new_name.contains('\\') {
                return Err(ParseError::InvalidArgument(
                    "rename: new name must not contain path separators".to_owned(),
                ));
            }
            Ok(ParsedCommand::Rename(new_name))
        }

        "mv" => {
            if rest.is_empty() {
                return Err(ParseError::MissingArgument("mv".to_owned()));
            }
            Ok(ParsedCommand::Mv(rest.to_owned()))
        }

        "cp" => {
            if rest.is_empty() {
                return Err(ParseError::MissingArgument("cp".to_owned()));
            }
            Ok(ParsedCommand::Cp(rest.to_owned()))
        }

        "git" => {
            if rest.is_empty() {
                return Err(ParseError::MissingArgument("git".to_owned()));
            }
            // TODO(phase-4): Wire to the git worker.
            Ok(ParsedCommand::Git(rest.to_owned()))
        }

        "set" => {
            // Expect: set <key> <value>
            let (key, value) = match rest.split_once(' ') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => {
                    return Err(ParseError::MissingArgument(
                        "set requires <key> <value>".to_owned(),
                    ));
                }
            };
            if key.is_empty() {
                return Err(ParseError::MissingArgument("set: key is empty".to_owned()));
            }
            if value.is_empty() {
                return Err(ParseError::MissingArgument(
                    "set: value is empty".to_owned(),
                ));
            }
            // TODO(phase-7): Validate key against config schema.
            Ok(ParsedCommand::Set {
                key: key.to_owned(),
                value: value.to_owned(),
            })
        }

        other => Err(ParseError::UnknownVerb(other.to_owned())),
    }
}

// ── Completion ────────────────────────────────────────────────────────────────

/// Returns a list of completion candidates for the current command `buffer`.
///
/// Completions are provided for:
/// - **Verb completion**: when the buffer is a prefix of a known verb (no
///   space typed yet), return the full verb with a trailing space.
/// - **Path completion**: for `:mv` and `:cp`, complete the destination
///   argument against filesystem entries in `cwd`.
///
/// The returned `Vec` is empty when no completions apply. The caller should
/// cycle through candidates on repeated `Tab` presses.
///
/// `is_shell`: whether the buffer came from a `!`-prefixed input (no verb
/// completion applies — the shell handles its own completion).
pub fn completions(buffer: &str, cwd: &Path, is_shell: bool) -> Vec<String> {
    if is_shell {
        return Vec::new(); // shell completion not handled here
    }

    let trimmed = buffer.trim_start();

    // If there's no space yet, complete the verb.
    if !trimmed.contains(' ') {
        let prefix = trimmed;
        let verbs = ["mkdir", "touch", "rename", "mv", "cp", "git", "set"];
        return verbs
            .iter()
            .filter(|v| v.starts_with(prefix))
            .map(|v| format!("{v} "))
            .collect();
    }

    // Verb is typed — attempt path completion for mv/cp.
    let (verb, partial) = match trimmed.split_once(' ') {
        Some((v, p)) => (v, p),
        None => return Vec::new(),
    };

    if !matches!(verb, "mv" | "cp") {
        return Vec::new();
    }

    // Complete `partial` against entries in cwd.
    path_completions(partial, cwd)
}

/// Returns filesystem-based completion candidates matching `partial` in `cwd`.
///
/// If `partial` is empty, all entries in `cwd` are returned. Otherwise,
/// entries whose names start with `partial` (case-sensitive) are returned.
fn path_completions(partial: &str, cwd: &Path) -> Vec<String> {
    let read = match fs::read_dir(cwd) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut candidates: Vec<String> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| partial.is_empty() || name.starts_with(partial))
        .collect();

    candidates.sort();
    candidates
}

// ── Command history ───────────────────────────────────────────────────────────

/// Maximum number of entries kept in the command history ring buffer.
const HISTORY_CAPACITY: usize = 100;

/// A ring buffer of past commands, persisted to a small cache file.
///
/// The buffer stores the most recent `HISTORY_CAPACITY` unique commands.
/// Duplicate consecutive entries are deduplicated. History is persisted in a
/// newline-delimited text file: one command per line, most-recent last.
#[derive(Debug, Default)]
pub struct CommandHistory {
    /// Entries, oldest first, most-recent last.
    entries: Vec<String>,
    /// Path to the persistence file; `None` means in-memory only.
    path: Option<PathBuf>,
}

impl CommandHistory {
    /// Creates a new, in-memory-only history buffer.
    pub fn new() -> Self {
        CommandHistory {
            entries: Vec::new(),
            path: None,
        }
    }

    /// Creates a history buffer backed by `path`.
    ///
    /// Existing entries are loaded immediately. If the file does not exist yet
    /// the history starts empty (it will be created on the first `push`). If
    /// the file exists but cannot be read, the history starts empty and will
    /// overwrite the file on the next `push`.
    // clippy: dead_code — called from command_parser_tests.rs and will be
    // wired into AppState::new() once the config dir path is resolved (Phase 7).
    #[allow(dead_code)]
    pub fn with_path(path: PathBuf) -> Self {
        let entries = load_history(&path).unwrap_or_default();
        CommandHistory {
            entries,
            path: Some(path),
        }
    }

    /// Appends `command` to the history, deduplicating consecutive identical
    /// entries and evicting the oldest entry when the ring buffer is full.
    ///
    /// Persists to disk if a path was provided. Errors are silently logged at
    /// `debug` level — history persistence is best-effort, not load-bearing.
    pub fn push(&mut self, command: String) {
        if command.is_empty() {
            return;
        }
        // Deduplicate consecutive identical entries.
        if self.entries.last().map(|s| s.as_str()) == Some(command.as_str()) {
            return;
        }
        // Evict oldest if at capacity.
        if self.entries.len() >= HISTORY_CAPACITY {
            self.entries.remove(0);
        }
        self.entries.push(command);

        if let Some(ref path) = self.path {
            if let Err(e) = save_history(path, &self.entries) {
                tracing::debug!("failed to save command history to {path:?}: {e}");
            }
        }
    }

    /// Returns the number of history entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if there are no history entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the entry at position `index` (0 = oldest, len-1 = newest).
    ///
    /// Returns `None` if `index` is out of range.
    pub fn get(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(String::as_str)
    }

    /// Returns the entry at `offset` positions from the most-recent end.
    ///
    /// `prev(0)` → most-recent; `prev(1)` → second-most-recent; etc.
    /// Returns `None` when `offset` exceeds the number of entries.
    // clippy: dead_code — used in command_parser_tests.rs integration tests.
    #[allow(dead_code)]
    pub fn prev(&self, offset: usize) -> Option<&str> {
        let n = self.entries.len();
        n.checked_sub(offset + 1).and_then(|i| self.get(i))
    }
}

/// Reads a history file. Each non-empty line is one entry.
// clippy: dead_code — called by with_path which is used in tests and
// will be wired into AppState::new() in Phase 7.
#[allow(dead_code)]
fn load_history(path: &Path) -> Option<Vec<String>> {
    let content = fs::read_to_string(path).ok()?;
    let entries: Vec<String> = content
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_owned())
        .collect();
    // Trim to capacity in case the file was hand-edited.
    let start = entries.len().saturating_sub(HISTORY_CAPACITY);
    Some(entries[start..].to_vec())
}

/// Writes all history entries to `path`, one per line.
fn save_history(path: &Path, entries: &[String]) -> std::io::Result<()> {
    // Ensure the parent directory exists.
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    let content = entries.join("\n");
    fs::write(path, content.as_bytes())?;
    Ok(())
}

// ── Command mode feed (key-level entry point) ─────────────────────────────────

/// The result of feeding one keystroke to Command Mode.
///
/// The caller in `input/mod.rs` dispatches on this to decide which `Action`
/// to queue, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedResult {
    /// The buffer was updated but no action should fire yet.
    Updated,
    /// The user pressed `Enter`; `buffer` is the completed input.
    Submit(String),
    /// The user pressed `Esc`; exit Command Mode without executing.
    Cancel,
    /// The user pressed `Tab`; `candidates` is the list of completions and
    /// `index` is which one was cycled to this press.
    Completion {
        candidates: Vec<String>,
        index: usize,
    },
}

/// Feeds a single keystroke into Command Mode state.
///
/// `buffer`: the current typed text (after `:` or `!`).
/// `cursor`: byte offset of the insertion point.
/// `history_index`: current history scroll position (`None` = live buffer).
/// `tab_state`: tracks the current completion cycle.
/// `history`: the command history.
/// `cwd`: current directory, used for path completions.
/// `is_shell`: whether the leading sentinel was `!` (not `:`).
///
/// Mutates `buffer`, `cursor`, and `history_index` in-place. Returns a
/// [`FeedResult`] describing what the caller should do next.
#[allow(clippy::too_many_arguments)]
pub fn feed(
    key: crossterm::event::KeyEvent,
    buffer: &mut String,
    cursor: &mut usize,
    history_index: &mut Option<usize>,
    tab_state: &mut TabState,
    history: &CommandHistory,
    cwd: &Path,
    is_shell: bool,
) -> FeedResult {
    use crossterm::event::{KeyCode, KeyModifiers};

    match key.code {
        KeyCode::Esc => FeedResult::Cancel,

        KeyCode::Enter => {
            // Collect the buffer before returning.
            FeedResult::Submit(buffer.clone())
        }

        KeyCode::Backspace => {
            // Delete the character immediately before the cursor.
            if *cursor > 0 {
                // Walk back one UTF-8 character boundary.
                let new_cursor = prev_char_boundary(buffer, *cursor);
                buffer.drain(new_cursor..*cursor);
                *cursor = new_cursor;
                tab_state.reset();
            }
            FeedResult::Updated
        }

        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Delete the character immediately before the cursor.
            if *cursor > 0 {
                // Walk back one UTF-8 character boundary.
                let new_cursor = prev_char_boundary(buffer, *cursor);
                buffer.drain(new_cursor..*cursor);
                *cursor = new_cursor;
                tab_state.reset();
            }
            FeedResult::Updated
        }

        KeyCode::Delete => {
            // Delete the character at the cursor (forward delete).
            if *cursor < buffer.len() {
                let next = next_char_boundary(buffer, *cursor);
                buffer.drain(*cursor..next);
                tab_state.reset();
            }
            FeedResult::Updated
        }

        KeyCode::Left => {
            if *cursor > 0 {
                *cursor = prev_char_boundary(buffer, *cursor);
            }
            FeedResult::Updated
        }

        KeyCode::Right => {
            if *cursor < buffer.len() {
                *cursor = next_char_boundary(buffer, *cursor);
            }
            FeedResult::Updated
        }

        KeyCode::Up => {
            // Scroll back through history.
            let next_idx = match *history_index {
                None => {
                    if history.is_empty() {
                        return FeedResult::Updated;
                    }
                    history.len() - 1
                }
                Some(i) => i.saturating_sub(1),
            };
            if let Some(entry) = history.get(next_idx) {
                *buffer = entry.to_owned();
                *cursor = buffer.len();
                *history_index = Some(next_idx);
            }
            tab_state.reset();
            FeedResult::Updated
        }

        KeyCode::Down => {
            // Scroll forward through history.
            match *history_index {
                None => FeedResult::Updated,
                Some(i) => {
                    if i + 1 < history.len() {
                        let next_idx = i + 1;
                        if let Some(entry) = history.get(next_idx) {
                            *buffer = entry.to_owned();
                            *cursor = buffer.len();
                            *history_index = Some(next_idx);
                        }
                    } else {
                        // Past the end of history — restore blank buffer.
                        buffer.clear();
                        *cursor = 0;
                        *history_index = None;
                    }
                    tab_state.reset();
                    FeedResult::Updated
                }
            }
        }

        KeyCode::Tab => {
            let candidates = completions(buffer, cwd, is_shell);
            if candidates.is_empty() {
                return FeedResult::Updated;
            }
            // Cycle to the next candidate.
            let idx = tab_state.advance(candidates.len());
            // Apply the completion: replace the current token with the candidate.
            apply_completion(buffer, cursor, &candidates[idx], is_shell);
            FeedResult::Completion {
                candidates,
                index: idx,
            }
        }

        KeyCode::Home => {
            *cursor = 0;
            FeedResult::Updated
        }

        KeyCode::End => {
            *cursor = buffer.len();
            FeedResult::Updated
        }

        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Insert the character at the cursor position.
            buffer.insert(*cursor, ch);
            *cursor += ch.len_utf8();
            *history_index = None;
            tab_state.reset();
            FeedResult::Updated
        }

        _ => FeedResult::Updated,
    }
}

/// Tracks the current tab-completion cycle.
///
/// Resets when the buffer changes (any non-Tab keystroke).
#[derive(Debug, Default)]
pub struct TabState {
    /// Current position in the candidate list (cycling).
    index: Option<usize>,
}

impl TabState {
    /// Creates a new, idle `TabState`.
    pub fn new() -> Self {
        TabState { index: None }
    }

    /// Resets the completion cycle (called on any non-Tab keystroke).
    pub fn reset(&mut self) {
        self.index = None;
    }

    /// Advances to the next completion and returns the new index.
    ///
    /// Wraps around when `len` is exceeded.
    pub fn advance(&mut self, len: usize) -> usize {
        let next = match self.index {
            None => 0,
            Some(i) => (i + 1) % len,
        };
        self.index = Some(next);
        next
    }
}

/// Applies a completion candidate to the command buffer.
///
/// For verb completion (no space in buffer): replaces the whole buffer.
/// For path completion (space present): replaces only the last token.
fn apply_completion(buffer: &mut String, cursor: &mut usize, candidate: &str, is_shell: bool) {
    if is_shell {
        return; // Shell completions not handled here.
    }
    if !buffer.contains(' ') {
        // Verb completion — replace entire buffer.
        *buffer = candidate.to_owned();
        *cursor = buffer.len();
    } else {
        // Path completion — replace the last space-delimited token.
        if let Some(last_space) = buffer.rfind(' ') {
            let prefix_end = last_space + 1;
            buffer.truncate(prefix_end);
            buffer.push_str(candidate);
            *cursor = buffer.len();
        }
    }
}

// ── UTF-8 cursor helpers ─────────────────────────────────────────────────────

/// Returns the byte offset of the start of the character immediately before
/// `cursor` in `s`. Panics if `cursor == 0`; callers must guard.
fn prev_char_boundary(s: &str, cursor: usize) -> usize {
    let mut pos = cursor.saturating_sub(1);
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Returns the byte offset of the start of the next character after `cursor`.
fn next_char_boundary(s: &str, cursor: usize) -> usize {
    let mut pos = cursor + 1;
    while pos < s.len() && !s.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse ──────────────────────────────────────────────────────────────────

    #[test]
    fn parse_mkdir_valid() {
        let cmd = parse("mkdir foo", false).unwrap();
        assert_eq!(cmd, ParsedCommand::Mkdir("foo".to_owned()));
    }

    #[test]
    fn parse_mkdir_empty_arg_is_error() {
        let err = parse("mkdir", false).unwrap_err();
        assert!(matches!(err, ParseError::MissingArgument(_)));
    }

    #[test]
    fn parse_mkdir_with_separator_is_error() {
        let err = parse("mkdir foo/bar", false).unwrap_err();
        assert!(matches!(err, ParseError::InvalidArgument(_)));
    }

    #[test]
    fn parse_touch_valid() {
        let cmd = parse("touch new_file.txt", false).unwrap();
        assert_eq!(cmd, ParsedCommand::Touch("new_file.txt".to_owned()));
    }

    #[test]
    fn parse_rename_valid() {
        let cmd = parse("rename new_name", false).unwrap();
        assert_eq!(cmd, ParsedCommand::Rename("new_name".to_owned()));
    }

    #[test]
    fn parse_rename_alias_ren() {
        let cmd = parse("ren new_name", false).unwrap();
        assert_eq!(cmd, ParsedCommand::Rename("new_name".to_owned()));
    }

    #[test]
    fn parse_rename_with_slash_is_error() {
        let err = parse("rename foo/bar", false).unwrap_err();
        assert!(matches!(err, ParseError::InvalidArgument(_)));
    }

    #[test]
    fn parse_mv_valid() {
        let cmd = parse("mv ../somewhere", false).unwrap();
        assert_eq!(cmd, ParsedCommand::Mv("../somewhere".to_owned()));
    }

    #[test]
    fn parse_cp_valid() {
        let cmd = parse("cp backup.txt", false).unwrap();
        assert_eq!(cmd, ParsedCommand::Cp("backup.txt".to_owned()));
    }

    #[test]
    fn parse_git_valid() {
        let cmd = parse("git status --short", false).unwrap();
        assert_eq!(cmd, ParsedCommand::Git("status --short".to_owned()));
    }

    #[test]
    fn parse_git_no_subcommand_is_error() {
        let err = parse("git", false).unwrap_err();
        assert!(matches!(err, ParseError::MissingArgument(_)));
    }

    #[test]
    fn parse_set_valid() {
        let cmd = parse("set git_status_enabled true", false).unwrap();
        assert_eq!(
            cmd,
            ParsedCommand::Set {
                key: "git_status_enabled".to_owned(),
                value: "true".to_owned(),
            }
        );
    }

    #[test]
    fn parse_set_no_value_is_error() {
        let err = parse("set key_only", false).unwrap_err();
        assert!(matches!(err, ParseError::MissingArgument(_)));
    }

    #[test]
    fn parse_empty_is_error() {
        let err = parse("", false).unwrap_err();
        assert!(matches!(err, ParseError::Empty));
    }

    #[test]
    fn parse_unknown_verb_is_error() {
        let err = parse("zap foo", false).unwrap_err();
        assert!(matches!(err, ParseError::UnknownVerb(_)));
    }

    #[test]
    fn parse_shell_command() {
        let cmd = parse("ls -la", true).unwrap();
        assert_eq!(cmd, ParsedCommand::Shell("ls -la".to_owned()));
    }

    #[test]
    fn parse_shell_empty_is_error() {
        let err = parse("", true).unwrap_err();
        assert!(matches!(err, ParseError::MissingArgument(_)));
    }

    // ── completions ────────────────────────────────────────────────────────────

    #[test]
    fn verb_completion_prefix_mk() {
        let dir = tempfile::tempdir().unwrap();
        let candidates = completions("mk", dir.path(), false);
        assert!(
            candidates.contains(&"mkdir ".to_owned()),
            "expected 'mkdir ' in candidates: {candidates:?}"
        );
    }

    #[test]
    fn verb_completion_empty_returns_all_verbs() {
        let dir = tempfile::tempdir().unwrap();
        let candidates = completions("", dir.path(), false);
        // All seven verbs should be present.
        assert_eq!(candidates.len(), 7);
    }

    #[test]
    fn verb_completion_no_match_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let candidates = completions("zzz", dir.path(), false);
        assert!(candidates.is_empty());
    }

    #[test]
    fn path_completion_for_mv() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("alpha.txt"), b"").unwrap();
        fs::write(dir.path().join("beta.txt"), b"").unwrap();

        let candidates = completions("mv al", dir.path(), false);
        assert!(
            candidates.contains(&"alpha.txt".to_owned()),
            "expected 'alpha.txt' in candidates: {candidates:?}"
        );
        assert!(
            !candidates.contains(&"beta.txt".to_owned()),
            "beta.txt should not match prefix 'al'"
        );
    }

    #[test]
    fn no_path_completion_for_mkdir() {
        let dir = tempfile::tempdir().unwrap();
        let candidates = completions("mkdir so", dir.path(), false);
        assert!(
            candidates.is_empty(),
            "path completion should not apply to mkdir"
        );
    }

    // ── CommandHistory ─────────────────────────────────────────────────────────

    #[test]
    fn history_push_and_retrieve() {
        let mut h = CommandHistory::new();
        h.push("mkdir foo".to_owned());
        h.push("touch bar".to_owned());
        assert_eq!(h.len(), 2);
        assert_eq!(h.prev(0), Some("touch bar"));
        assert_eq!(h.prev(1), Some("mkdir foo"));
    }

    #[test]
    fn history_deduplicates_consecutive_identical() {
        let mut h = CommandHistory::new();
        h.push("mkdir foo".to_owned());
        h.push("mkdir foo".to_owned());
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn history_allows_non_consecutive_duplicate() {
        let mut h = CommandHistory::new();
        h.push("mkdir foo".to_owned());
        h.push("touch bar".to_owned());
        h.push("mkdir foo".to_owned());
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn history_evicts_oldest_at_capacity() {
        let mut h = CommandHistory::new();
        for i in 0..HISTORY_CAPACITY + 5 {
            h.push(format!("cmd{i}"));
        }
        assert_eq!(h.len(), HISTORY_CAPACITY);
        // The oldest entries should be gone; the most recent should survive.
        assert_eq!(
            h.prev(0),
            Some(format!("cmd{}", HISTORY_CAPACITY + 4).as_str())
        );
    }

    #[test]
    fn history_persist_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.txt");

        let mut h = CommandHistory::with_path(path.clone());
        h.push("mkdir foo".to_owned());
        h.push("touch bar".to_owned());

        // Reload from the same path.
        let h2 = CommandHistory::with_path(path);
        assert_eq!(h2.len(), 2);
        assert_eq!(h2.prev(0), Some("touch bar"));
    }

    // ── feed (cursor movement) ─────────────────────────────────────────────────

    #[test]
    fn feed_char_appends_to_buffer() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let dir = tempfile::tempdir().unwrap();
        let mut buf = String::new();
        let mut cursor = 0usize;
        let mut hist_idx = None;
        let mut tab = TabState::new();
        let h = CommandHistory::new();

        let key = KeyEvent {
            code: KeyCode::Char('m'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let result = feed(
            key,
            &mut buf,
            &mut cursor,
            &mut hist_idx,
            &mut tab,
            &h,
            dir.path(),
            false,
        );
        assert_eq!(result, FeedResult::Updated);
        assert_eq!(buf, "m");
        assert_eq!(cursor, 1);
    }

    #[test]
    fn feed_enter_submits_buffer() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let dir = tempfile::tempdir().unwrap();
        let mut buf = "mkdir foo".to_owned();
        let mut cursor = buf.len();
        let mut hist_idx = None;
        let mut tab = TabState::new();
        let h = CommandHistory::new();

        let key = KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let result = feed(
            key,
            &mut buf,
            &mut cursor,
            &mut hist_idx,
            &mut tab,
            &h,
            dir.path(),
            false,
        );
        assert_eq!(result, FeedResult::Submit("mkdir foo".to_owned()));
    }

    #[test]
    fn feed_esc_cancels() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let dir = tempfile::tempdir().unwrap();
        let mut buf = "mkdir foo".to_owned();
        let mut cursor = buf.len();
        let mut hist_idx = None;
        let mut tab = TabState::new();
        let h = CommandHistory::new();

        let key = KeyEvent {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let result = feed(
            key,
            &mut buf,
            &mut cursor,
            &mut hist_idx,
            &mut tab,
            &h,
            dir.path(),
            false,
        );
        assert_eq!(result, FeedResult::Cancel);
    }

    #[test]
    fn feed_backspace_removes_last_char() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let dir = tempfile::tempdir().unwrap();
        let mut buf = "abc".to_owned();
        let mut cursor = 3usize;
        let mut hist_idx = None;
        let mut tab = TabState::new();
        let h = CommandHistory::new();

        let key = KeyEvent {
            code: KeyCode::Backspace,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        feed(
            key,
            &mut buf,
            &mut cursor,
            &mut hist_idx,
            &mut tab,
            &h,
            dir.path(),
            false,
        );
        assert_eq!(buf, "ab");
        assert_eq!(cursor, 2);
    }
}
