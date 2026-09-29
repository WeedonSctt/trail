//! Declarative preview providers registered by plugins.
//!
//! `trail.register_previewer{ extensions = {"json"}, command = {"jq", ".", "{path}"} }`
//! describes a previewer; it does not implement one. Lua runs once, at load
//! time, to produce a [`PreviewerRule`], and from then on the preview path is
//! pure Rust: [`CommandPreviewProvider`] matches entries against the rules and
//! runs the command on the worker pool, delivering its output through
//! `WorkerMsg::Preview` and therefore through the generation guard. No Lua ever
//! runs on the preview path, so neither invariant 1 nor invariant 2 depends on
//! a plugin behaving. See `docs/plugin_api_plan.md` §4.6.

use std::path::Path;
use std::time::Duration;

use crate::app::state::{Entry, EntryKind};
use crate::plugin::request::CommandSpec;
use crate::preview::provider::{
    sanitize, PreviewContent, PreviewCtx, PreviewOutcome, PreviewProvider,
};
use crate::workers::WorkerMsg;

/// How long a previewer command may run before it is killed, when the rule
/// does not say.
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;

/// The placeholder in a previewer command that is replaced by the entry's path.
pub const PATH_PLACEHOLDER: &str = "{path}";

/// One previewer, as a plugin described it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewerRule {
    /// The plugin that registered it, for error messages.
    pub plugin: String,
    /// The previewer's own name, shown in its error messages.
    pub name: String,
    /// File extensions it handles, lowercase and without the dot.
    pub extensions: Vec<String>,
    /// Exact file names it handles (`Makefile`, `Cargo.lock`), compared
    /// case-insensitively.
    pub names: Vec<String>,
    /// What to run. [`PATH_PLACEHOLDER`] is replaced by the entry's path.
    pub command: CommandSpec,
    /// How long the command may run before it is killed.
    pub timeout: Duration,
}

impl PreviewerRule {
    /// Whether this rule previews `entry`: a regular file whose extension or
    /// name it lists.
    pub fn matches(&self, entry: &Entry) -> bool {
        if entry.kind != EntryKind::File {
            return false;
        }
        let name = entry.file_name.to_ascii_lowercase();
        if self.names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            return true;
        }
        Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| self.extensions.iter().any(|x| x == ext))
    }

    /// The command with [`PATH_PLACEHOLDER`] filled in.
    ///
    /// A shell command gets the path quoted, because the shell will split it;
    /// an argv command gets it verbatim, because nothing will.
    fn command_for(&self, path: &str) -> CommandSpec {
        match &self.command {
            CommandSpec::Argv(argv) => CommandSpec::Argv(
                argv.iter()
                    .map(|a| a.replace(PATH_PLACEHOLDER, path))
                    .collect(),
            ),
            CommandSpec::Shell(line) => {
                CommandSpec::Shell(line.replace(PATH_PLACEHOLDER, &format!("\"{path}\"")))
            }
        }
    }
}

/// The preview provider that runs every plugin previewer.
///
/// Registered ahead of the built-in providers, so a plugin previewer wins for
/// the files it names — which is the point of registering one.
pub struct CommandPreviewProvider {
    rules: Vec<PreviewerRule>,
    shell: String,
}

impl CommandPreviewProvider {
    /// A provider for `rules`, running shell-string commands through `shell`
    /// (the `[general] shell` value, empty for the platform default).
    pub fn new(rules: Vec<PreviewerRule>, shell: String) -> Self {
        Self { rules, shell }
    }
}

impl PreviewProvider for CommandPreviewProvider {
    fn can_handle(&self, entry: &Entry) -> bool {
        self.rules.iter().any(|r| r.matches(entry))
    }

    fn preview(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome {
        let Some(rule) = self.rules.iter().find(|r| r.matches(entry)) else {
            return PreviewOutcome::Ready(PreviewContent::Empty);
        };
        let command = rule.command_for(&crate::pathfmt::display(&entry.path));
        let label = format!("{} ({})", rule.name, rule.plugin);
        let timeout = rule.timeout;
        let shell = self.shell.clone();
        let path = entry.path.clone();
        let cwd = entry
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let generation = ctx.generation;
        let max_lines = ctx.max_preview_lines;
        let tx = ctx.worker_tx.clone();

        tokio::spawn(async move {
            let (content, truncated) = match run_with_timeout(&command, &shell, &cwd, timeout).await
            {
                Ok(stdout) => to_lines(&stdout, max_lines),
                Err(reason) => (
                    PreviewContent::Text(vec![format!("   !  previewer {label}: {reason}")]),
                    false,
                ),
            };
            let _ = tx
                .send(WorkerMsg::Preview {
                    generation,
                    path,
                    content,
                    truncated,
                    is_text: None,
                })
                .await;
        });
        PreviewOutcome::Deferred
    }
}

/// Runs `command` in `cwd`, returning its stdout, or why there is none.
///
/// `kill_on_drop` is what makes the timeout real: when `timeout` fires the
/// future holding the child is dropped, and the child with it.
async fn run_with_timeout(
    command: &CommandSpec,
    shell: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let argv = match command {
        CommandSpec::Argv(argv) => argv.clone(),
        CommandSpec::Shell(line) => crate::actions::shell_exec::shell_argv(shell, line),
    };
    let (program, args) = argv.split_first().ok_or("empty command")?;
    let child = tokio::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(timeout, child).await {
        Err(_) => Err(format!("timed out after {} ms", timeout.as_millis())),
        Ok(Err(e)) => Err(format!("could not start `{program}`: {e}")),
        Ok(Ok(out)) if !out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let first = stderr.lines().next().unwrap_or("").trim();
            Err(match out.status.code() {
                Some(code) if first.is_empty() => format!("exited with status {code}"),
                Some(code) => format!("exited with status {code}: {first}"),
                None => "was killed".to_owned(),
            })
        }
        Ok(Ok(out)) => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
    }
}

/// Formats command output the way the text preview formats a file: numbered,
/// sanitized, bounded by `max_lines`.
///
/// Colour is stripped rather than rendered. Tools like `jq -C` and `bat` emit
/// ANSI escapes, and `sanitize` would otherwise leave their parameters behind
/// as visible `[1;31m` noise once it had replaced the ESC.
fn to_lines(stdout: &str, max_lines: usize) -> (PreviewContent, bool) {
    let mut lines = stdout.lines();
    let shown: Vec<String> = lines
        .by_ref()
        .take(max_lines)
        .enumerate()
        .map(|(i, l)| format!("{:>4}  {}", i + 1, sanitize(&strip_ansi(l))))
        .collect();
    let truncated = lines.next().is_some();
    (PreviewContent::Text(shown), truncated)
}

/// Removes ANSI escape sequences: CSI (`ESC [ … final`) and OSC
/// (`ESC ] … BEL` or `ESC ] … ESC \`), plus any other two-byte `ESC x`.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                // Parameters and intermediates, then one final byte in @..~.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(extensions: &[&str], names: &[&str]) -> PreviewerRule {
        PreviewerRule {
            plugin: "test".to_owned(),
            name: "t".to_owned(),
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            names: names.iter().map(|s| s.to_string()).collect(),
            command: CommandSpec::Argv(vec!["cat".to_owned(), PATH_PLACEHOLDER.to_owned()]),
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        }
    }

    fn file(name: &str) -> Entry {
        Entry {
            path: std::path::PathBuf::from(name),
            file_name: name.to_owned(),
            kind: EntryKind::File,
            is_hidden: false,
            metadata: None,
            git_status: None,
            is_text: None,
        }
    }

    #[test]
    fn matches_by_extension_case_insensitively() {
        let r = rule(&["json"], &[]);
        assert!(r.matches(&file("a.json")));
        assert!(r.matches(&file("A.JSON")));
        assert!(!r.matches(&file("a.jsonl")));
        assert!(!r.matches(&file("json")));
    }

    #[test]
    fn matches_by_exact_name() {
        let r = rule(&[], &["Makefile"]);
        assert!(r.matches(&file("makefile")));
        assert!(!r.matches(&file("Makefile.am")));
    }

    #[test]
    fn a_directory_is_never_matched() {
        let mut dir = file("x.json");
        dir.kind = EntryKind::Dir;
        assert!(!rule(&["json"], &[]).matches(&dir));
    }

    #[test]
    fn the_path_placeholder_is_filled_in_and_quoted_only_for_a_shell() {
        let r = rule(&["json"], &[]);
        assert_eq!(
            r.command_for("a b.json"),
            CommandSpec::Argv(vec!["cat".to_owned(), "a b.json".to_owned()])
        );
        let shell = PreviewerRule {
            command: CommandSpec::Shell("jq . {path}".to_owned()),
            ..r
        };
        assert_eq!(
            shell.command_for("a b.json"),
            CommandSpec::Shell("jq . \"a b.json\"".to_owned())
        );
    }

    #[test]
    fn ansi_colour_is_stripped() {
        assert_eq!(strip_ansi("\u{1b}[1;31mred\u{1b}[0m plain"), "red plain");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}after"), "after");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
    }

    #[test]
    fn output_is_numbered_and_bounded() {
        let (content, truncated) = to_lines("a\nb\nc\n", 2);
        assert!(truncated);
        let PreviewContent::Text(lines) = content else {
            panic!("command output is a text preview");
        };
        assert_eq!(lines, vec!["   1  a".to_owned(), "   2  b".to_owned()]);
    }

    #[tokio::test]
    async fn a_command_that_overruns_is_killed() {
        let slow = if cfg!(windows) {
            CommandSpec::Shell("ping -n 5 127.0.0.1 > NUL".to_owned())
        } else {
            CommandSpec::Argv(vec!["sleep".to_owned(), "5".to_owned()])
        };
        let err = run_with_timeout(&slow, "", &std::env::temp_dir(), Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(err.contains("timed out"), "{err}");
    }
}
