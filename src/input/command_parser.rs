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
    /// Move every entry of `cwd` matching `pattern` into the directory `dest`.
    MvMatching {
        /// A name with `*` or `?` in it, matched against the current directory.
        pattern: String,
        /// Destination, which has to be an existing directory: several files
        /// cannot be moved onto one path.
        dest: String,
    },
    /// Copy every entry of `cwd` matching `pattern` into the directory `dest`.
    CpMatching {
        /// A name with `*` or `?` in it, matched against the current directory.
        pattern: String,
        /// Destination, which has to be an existing directory.
        dest: String,
    },
    /// Run a git subcommand string (e.g. `"init"`, `"status --short"`).
    /// The git worker will be wired in Phase 4; syntactically accepted now.
    Git(String),
    /// Set a runtime config key to a value.
    /// The config schema will be wired in Phase 7; syntactically accepted now.
    Set { key: String, value: String },
    /// Execute an arbitrary shell command string.
    Shell(String),
    /// Add a bookmark: `:bookmark <name>` saves `cwd` under `name`.
    /// If `name` is empty the current directory base-name is used.
    Bookmark(String),
    /// Jump to a previously saved bookmark: `:jump <name>`.
    Jump(String),
    /// Run a custom plugin action: `:plugin <name> [arg]`.
    Plugin { name: String, arg: String },
}

// ── Parse error ────────────────────────────────────────────────────────────────

/// Validation errors produced during command parsing.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// An unrecognised command verb was entered.
    #[error("unknown command '{0}' — try :mkdir, :touch, :rename, :mv, :cp, :git, :set, :bookmark, :jump, :plugin")]
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
            match split_pattern_and_dest("mv", rest)? {
                Some((pattern, dest)) => Ok(ParsedCommand::MvMatching { pattern, dest }),
                None => Ok(ParsedCommand::Mv(rest.to_owned())),
            }
        }

        "cp" => {
            if rest.is_empty() {
                return Err(ParseError::MissingArgument("cp".to_owned()));
            }
            match split_pattern_and_dest("cp", rest)? {
                Some((pattern, dest)) => Ok(ParsedCommand::CpMatching { pattern, dest }),
                None => Ok(ParsedCommand::Cp(rest.to_owned())),
            }
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
            Ok(ParsedCommand::Set {
                key: key.to_owned(),
                value: value.to_owned(),
            })
        }

        "bookmark" | "bm" => {
            // :bookmark [name] — name is optional; empty string means
            // "use the cwd base-name" (resolved at execution time).
            let name = rest.trim().to_owned();
            if name.contains('/') || name.contains('\\') {
                return Err(ParseError::InvalidArgument(
                    "bookmark: name must not contain path separators".to_owned(),
                ));
            }
            Ok(ParsedCommand::Bookmark(name))
        }

        "jump" | "j" => {
            // :jump <name> — name is required.
            let name = rest.trim();
            if name.is_empty() {
                return Err(ParseError::MissingArgument("jump".to_owned()));
            }
            Ok(ParsedCommand::Jump(name.to_owned()))
        }

        "plugin" => {
            // :plugin <action_name> [arg] — action name is required.
            let rest_trimmed = rest.trim();
            if rest_trimmed.is_empty() {
                return Err(ParseError::MissingArgument("plugin".to_owned()));
            }
            let (name, arg) = match rest_trimmed.split_once(' ') {
                Some((n, a)) => (n.trim(), a.trim()),
                None => (rest_trimmed, ""),
            };
            if name.is_empty() {
                return Err(ParseError::MissingArgument("plugin".to_owned()));
            }
            Ok(ParsedCommand::Plugin {
                name: name.to_owned(),
                arg: arg.to_owned(),
            })
        }

        other => Err(ParseError::UnknownVerb(other.to_owned())),
    }
}

/// Splits the argument of `:mv`/`:cp` into a wildcard pattern and a destination,
/// or `None` when it is an ordinary one-argument destination.
///
/// The first whitespace-separated token decides: a `*` or a `?` in it means the
/// user is naming several files, and everything after it is the destination.
/// Nothing else is treated as two arguments, which is what keeps a destination
/// containing a space (`:mv my folder`) working exactly as before -- `*` and `?`
/// cannot appear in a Windows filename at all.
///
/// # Errors
///
/// Returns [`ParseError::MissingArgument`] for a pattern with no destination
/// after it: there is no sensible default, and guessing `cwd` would move files
/// onto themselves.
fn split_pattern_and_dest(verb: &str, rest: &str) -> Result<Option<(String, String)>, ParseError> {
    let (first, remainder) = match rest.split_once(char::is_whitespace) {
        Some((first, remainder)) => (first, remainder.trim()),
        None => (rest, ""),
    };

    if !crate::actions::fs_ops::is_pattern(first) {
        return Ok(None);
    }
    if remainder.is_empty() {
        return Err(ParseError::MissingArgument(format!(
            "{verb} {first} needs a destination directory"
        )));
    }
    Ok(Some((first.to_owned(), remainder.to_owned())))
}

// ── Completion ────────────────────────────────────────────────────────────────

/// Returns a list of completion candidates for the current command `buffer`.
///
/// Completions are provided for:
/// - **Verb completion**: when the buffer is a prefix of a known verb (no
///   space typed yet), return the full verb with a trailing space.
/// - **Path completion**: for `:mv` and `:cp`, complete the destination
///   argument against filesystem entries. A destination that is a whole route
///   (`nested\`, `..\dst\re`, `C:\Users\me\Desk`) is completed against the
///   directory the route names, not against `cwd`. Directory candidates carry
///   a trailing separator, so a route can be walked one `Tab` at a time.
///
/// The returned `Vec` is empty when no completions apply. The caller should
/// cycle through candidates on repeated `Tab` presses.
///
/// `is_shell`: whether the buffer came from a `!`-prefixed input (no verb
/// completion applies — the shell handles its own completion).
#[allow(dead_code)]
pub fn completions(buffer: &str, cwd: &Path, is_shell: bool) -> Vec<String> {
    completions_with_plugins(buffer, cwd, is_shell, &[])
}

/// Same as [`completions`], but accepts registered plugin action names for `:plugin` completion.
pub fn completions_with_plugins(
    buffer: &str,
    cwd: &Path,
    is_shell: bool,
    plugin_actions: &[&str],
) -> Vec<String> {
    if is_shell {
        return Vec::new(); // shell completion not handled here
    }

    let trimmed = buffer.trim_start();

    // If there's no space yet, complete the verb.
    if !trimmed.contains(' ') {
        let prefix = trimmed;
        let verbs = [
            "mkdir", "touch", "rename", "mv", "cp", "git", "set", "bookmark", "jump", "plugin",
        ];
        return verbs
            .iter()
            .filter(|v| v.starts_with(prefix))
            .map(|v| format!("{v} "))
            .collect();
    }

    // Verb is typed — check sub-completions.
    let (verb, partial) = match trimmed.split_once(' ') {
        Some((v, p)) => (v, p),
        None => return Vec::new(),
    };

    if verb == "plugin" {
        let mut candidates: Vec<String> = plugin_actions
            .iter()
            .filter(|a| partial.is_empty() || a.starts_with(partial))
            .map(|a| a.to_string())
            .collect();
        candidates.sort();
        return candidates;
    }

    if !matches!(verb, "mv" | "cp") {
        return Vec::new();
    }

    // Complete `partial` against entries in cwd.
    path_completions(partial, cwd)
}

/// Returns filesystem-based completion candidates matching `partial`.
///
/// `partial` is the destination typed so far. It may be a bare name (`no`), a
/// route into a subdirectory (`nested\re`, `..\dst\`) or an absolute path
/// (`C:\Users\me\Desk`). Everything up to the last separator names the
/// directory to list; what follows is the prefix to match against its entries
/// (case-sensitive). A route that names no directory is completed against
/// `cwd`, as before.
///
/// Each candidate is the whole token to put back on the command line, route
/// included, because [`apply_completion`] replaces the entire space-delimited
/// token. Directories carry a trailing separator so a route can be walked one
/// `Tab` at a time, and the separator the user already typed is the one used —
/// a route typed with `\` stays spelled with `\`.
fn path_completions(partial: &str, cwd: &Path) -> Vec<String> {
    // `std::path::is_separator` is the platform's own answer: `/` and `\` on
    // Windows, `/` alone on Unix, where a backslash is an ordinary character in
    // a file name and must not split a route.
    let split_at = partial.rfind(std::path::is_separator).map_or(0, |i| i + 1);
    let (route, name_prefix) = partial.split_at(split_at);

    let search_dir = if route.is_empty() {
        cwd.to_owned()
    } else {
        // `join` is correct for every shape a route can take here, including a
        // Windows path that is rooted but has no drive (`\Users`), which
        // replaces everything but `cwd`'s drive letter.
        cwd.join(route)
    };

    let read = match fs::read_dir(&search_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let separator = route
        .chars()
        .next_back()
        .filter(|c| std::path::is_separator(*c))
        .unwrap_or(std::path::MAIN_SEPARATOR);

    let mut candidates: Vec<String> = read
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !name.starts_with(name_prefix) {
                return None;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                Some(format!("{route}{name}{separator}"))
            } else {
                Some(format!("{route}{name}"))
            }
        })
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
#[allow(dead_code)]
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
    feed_with_plugins(
        key,
        buffer,
        cursor,
        history_index,
        tab_state,
        history,
        cwd,
        is_shell,
        &[],
    )
}

/// Same as [`feed`], but also accepts registered plugin action names for tab completion.
#[allow(clippy::too_many_arguments)]
pub fn feed_with_plugins(
    key: crossterm::event::KeyEvent,
    buffer: &mut String,
    cursor: &mut usize,
    history_index: &mut Option<usize>,
    tab_state: &mut TabState,
    history: &CommandHistory,
    cwd: &Path,
    is_shell: bool,
    plugin_actions: &[&str],
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
            // Moving the cursor abandons the completion cycle: the next `Tab`
            // should complete where the cursor is now, not where it started.
            tab_state.reset();
            FeedResult::Updated
        }

        KeyCode::Right => {
            if *cursor < buffer.len() {
                *cursor = next_char_boundary(buffer, *cursor);
            }
            tab_state.reset();
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
            // Candidates come from the text that opened the cycle, not from the
            // buffer — the previous `Tab` has already overwritten that with a
            // candidate, and completing a completion is how the cycle used to
            // get stuck on its first answer.
            let prefix = tab_state.cycle_prefix(buffer);
            let candidates = completions_with_plugins(&prefix, cwd, is_shell, plugin_actions);
            if candidates.is_empty() {
                return FeedResult::Updated;
            }
            // Cycle to the next candidate.
            let idx = tab_state.advance(candidates.len());
            // Apply the completion: replace the completed token with the candidate.
            apply_completion(buffer, cursor, &prefix, &candidates[idx], is_shell);
            FeedResult::Completion {
                candidates,
                index: idx,
            }
        }

        KeyCode::Home => {
            *cursor = 0;
            tab_state.reset();
            FeedResult::Updated
        }

        KeyCode::End => {
            *cursor = buffer.len();
            tab_state.reset();
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
    /// The buffer text the cycle is completing, captured on the first `Tab`.
    ///
    /// Applying a completion overwrites the buffer, so the text the user
    /// actually typed is gone by the second `Tab`. Without it, candidates were
    /// recomputed from the completed buffer: `:m` + `Tab` gives `mkdir `, which
    /// contains a space, so the next `Tab` took the path-completion branch,
    /// found `mkdir` is not `mv`/`cp`, and returned nothing — `mv` could never
    /// be reached.
    prefix: Option<String>,
}

impl TabState {
    /// Creates a new, idle `TabState`.
    pub fn new() -> Self {
        TabState {
            index: None,
            prefix: None,
        }
    }

    /// Resets the completion cycle (called on any non-Tab keystroke).
    pub fn reset(&mut self) {
        self.index = None;
        self.prefix = None;
    }

    /// Returns the text this cycle completes, starting a cycle at `buffer` when
    /// none is running.
    ///
    /// Every `Tab` in the same cycle therefore sees the same candidate list, in
    /// the same order, which is what makes [`TabState::advance`] step through it
    /// rather than re-deciding it.
    pub fn cycle_prefix(&mut self, buffer: &str) -> String {
        self.prefix.get_or_insert_with(|| buffer.to_owned()).clone()
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

/// Rewrites `buffer` as `prefix` with its last token replaced by `candidate`.
///
/// `prefix` is what the user typed before the cycle started, not the current
/// buffer: the buffer already holds the previous candidate, and replacing that
/// one's last token would compound completions instead of offering the next.
///
/// For verb completion (no space in `prefix`) the candidate is the whole line.
/// For path completion everything up to the last space is kept.
fn apply_completion(
    buffer: &mut String,
    cursor: &mut usize,
    prefix: &str,
    candidate: &str,
    is_shell: bool,
) {
    if is_shell {
        return; // Shell completions not handled here.
    }
    *buffer = match prefix.rfind(' ') {
        Some(last_space) => format!("{}{candidate}", &prefix[..=last_space]),
        None => candidate.to_owned(),
    };
    *cursor = buffer.len();
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
    fn parse_plugin_valid() {
        let cmd = parse("plugin my_action", false).unwrap();
        assert_eq!(
            cmd,
            ParsedCommand::Plugin {
                name: "my_action".to_owned(),
                arg: "".to_owned(),
            }
        );
    }

    #[test]
    fn parse_plugin_with_args() {
        let cmd = parse("plugin log_note hello world", false).unwrap();
        assert_eq!(
            cmd,
            ParsedCommand::Plugin {
                name: "log_note".to_owned(),
                arg: "hello world".to_owned(),
            }
        );
    }

    #[test]
    fn parse_plugin_no_action_is_error() {
        let err = parse("plugin", false).unwrap_err();
        assert!(matches!(err, ParseError::MissingArgument(_)));
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
        assert_eq!(candidates.len(), 10);
    }

    #[test]
    fn plugin_action_completion() {
        let dir = tempfile::tempdir().unwrap();
        let candidates =
            completions_with_plugins("plugin ", dir.path(), false, &["log_note", "bookmark_add"]);
        assert_eq!(candidates, vec!["bookmark_add", "log_note"]);
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

    // ── feed (tab completion) ──────────────────────────────────────────────────

    /// Presses `Tab` `times` times against `buffer` and returns what the buffer
    /// became, driving the same `TabState` throughout as a real session does.
    fn tab_cycle(buffer: &str, cwd: &Path, times: usize) -> Vec<String> {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let mut buf = buffer.to_owned();
        let mut cursor = buf.len();
        let mut hist_idx = None;
        let mut tab = TabState::new();
        let h = CommandHistory::new();
        let key = KeyEvent {
            code: KeyCode::Tab,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        (0..times)
            .map(|_| {
                feed(
                    key,
                    &mut buf,
                    &mut cursor,
                    &mut hist_idx,
                    &mut tab,
                    &h,
                    cwd,
                    false,
                );
                buf.clone()
            })
            .collect()
    }

    /// The bug this guards: applying a completion overwrote the buffer, and the
    /// next `Tab` recomputed candidates from *that*. `:m` completed to `mkdir `,
    /// which contains a space, so the second `Tab` looked for path completions
    /// for a verb that takes none and found nothing — `mv` was unreachable.
    #[test]
    fn tab_cycles_through_verb_completions_and_wraps() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            tab_cycle("m", dir.path(), 3),
            vec!["mkdir ", "mv ", "mkdir "],
            "Tab must step through every verb sharing the prefix, then wrap"
        );
    }

    #[test]
    fn tab_cycles_through_path_completions() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("alpha.txt"), b"").unwrap();
        fs::write(dir.path().join("alpine.txt"), b"").unwrap();

        assert_eq!(
            tab_cycle("mv al", dir.path(), 3),
            vec!["mv alpha.txt", "mv alpine.txt", "mv alpha.txt"],
            "a destination with two matches must offer both"
        );
    }

    /// Typing anything ends the cycle, so the next `Tab` completes what is on
    /// the line now rather than continuing an abandoned list.
    #[test]
    fn editing_restarts_the_completion_cycle() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let dir = tempfile::tempdir().unwrap();
        let mut buf = "m".to_owned();
        let mut cursor = buf.len();
        let mut hist_idx = None;
        let mut tab = TabState::new();
        let h = CommandHistory::new();
        let press = |code| KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        let mut send = |code, buf: &mut String, cursor: &mut usize| {
            feed(
                press(code),
                buf,
                cursor,
                &mut hist_idx,
                &mut tab,
                &h,
                dir.path(),
                false,
            );
        };

        send(KeyCode::Tab, &mut buf, &mut cursor);
        assert_eq!(buf, "mkdir ");
        // Backspace leaves "mkdir", which is a complete verb on its own.
        send(KeyCode::Backspace, &mut buf, &mut cursor);
        send(KeyCode::Tab, &mut buf, &mut cursor);
        assert_eq!(
            buf, "mkdir ",
            "the new cycle completes the edited text, not the old prefix"
        );
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
