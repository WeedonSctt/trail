//! External previewer provider: runs a `[[preview.tool]]` program for a file
//! and shows its output.
//!
//! Two halves. [`resolve`] is a pure function that decides, from the rules,
//! the session's `P` toggles and the pane size, whether an entry gets an
//! external preview and exactly what to run — it is called by the event loop
//! before the registry, and its answer travels in
//! [`PreviewCtx::external`]. [`ExternalProvider`] then takes any file that
//! answer covers and hands it to `workers::external_preview`, so nothing is
//! spawned or waited on here, on the UI thread.
//!
//! Registered second, after directories and before images, so a rule can take
//! an image type when the user asks for it.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use crate::app::state::{Entry, EntryKind};
use crate::config::preview_tool::{self, Placeholders, PreviewMode, ToolCommand};
use crate::config::TrailConfig;
use crate::preview::provider::{PreviewCtx, PreviewOutcome, PreviewProvider};
use crate::workers::external_preview::{spawn_external_preview, ExternalJob};

/// Pane size assumed before the first frame has recorded the real one.
///
/// Only the renderer knows the pane's size, and the first preview is requested
/// before the first frame. A tool told this draws for a standard terminal, and
/// every later preview gets the measured size.
pub const ASSUMED_PANE: (u16, u16) = (80, 24);

/// Everything needed to run one external preview, resolved on the UI thread
/// from data it already holds — no `PATH` lookup, no I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalSpec {
    /// The program and its arguments, placeholders already substituted. For a
    /// string-form rule this is `[general] shell`'s argv with the script as the
    /// final argument.
    pub argv: Vec<OsString>,
    /// The program's name, for the pane's border and its error lines.
    pub tool: String,
    /// Pane width in columns, exported as `TRAIL_PREVIEW_WIDTH`.
    pub width: u16,
    /// Pane height in rows, exported as `TRAIL_PREVIEW_HEIGHT`.
    pub height: u16,
    /// How long the program may run, from `[preview] external_timeout_ms`.
    pub timeout: Duration,
}

/// The normalised extension of `path` — the key rules and toggles use — or
/// `None` for a file without one.
pub fn extension_of(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    Some(preview_tool::normalize_extension(ext))
}

/// Decides whether `path` gets an external preview, and what to run.
///
/// The rule for the file's extension applies in the session's mode for that
/// extension — `overrides`, which `P` flips — or, with no override, in the
/// rule's own `default`. Returns `None` when there is no rule, or the mode is
/// `builtin`, and the built-in preview should be used.
///
/// `pane` is the preview pane's interior size in columns and rows, substituted
/// for `{width}` and `{height}`.
pub fn resolve(
    path: &Path,
    config: &TrailConfig,
    overrides: &HashMap<String, PreviewMode>,
    pane: (u16, u16),
) -> Option<ExternalSpec> {
    let ext = extension_of(path)?;
    let rule = preview_tool::find_rule(&config.preview.tool, &ext)?;
    let mode = overrides.get(&ext).copied().unwrap_or(rule.default);
    if mode == PreviewMode::Builtin {
        return None;
    }

    let (width, height) = pane;
    let argv = match &rule.command {
        ToolCommand::Argv(template) => {
            let values = Placeholders {
                path,
                width,
                height,
            };
            let mut argv = Vec::with_capacity(template.len());
            for arg in template {
                match preview_tool::expand_arg(arg, &values) {
                    Ok(expanded) => argv.push(expanded),
                    // `validate` rejected every template that can fail here at
                    // load, so this is unreachable short of a bug; fall back to
                    // the built-in preview rather than run half a command.
                    Err(e) => {
                        tracing::warn!("previewer rule for .{ext} did not expand: {e}");
                        return None;
                    }
                }
            }
            argv
        }
        ToolCommand::Shell(script) => {
            crate::actions::shell_exec::shell_argv(&config.general.shell, script)
                .into_iter()
                .map(OsString::from)
                .collect()
        }
    };

    Some(ExternalSpec {
        argv,
        tool: tool_label(&rule.command),
        width,
        height,
        timeout: Duration::from_millis(config.preview.external_timeout_ms),
    })
}

/// The name a rule's tool is shown under, on the pane's border and in notices:
/// the program's file stem, so `C:\bin\glow.exe` is `glow`.
///
/// For the string form that is the script's first word — the tool, as far as
/// the user is concerned — not the shell that runs it.
pub fn tool_label(command: &ToolCommand) -> String {
    let program = match command {
        ToolCommand::Argv(argv) => argv.first().cloned().unwrap_or_default(),
        ToolCommand::Shell(script) => crate::actions::shell_exec::split_shell_spec(script)
            .and_then(|tokens| tokens.into_iter().next())
            .unwrap_or_default(),
    };
    tool_name(&program)
}

/// What `P` did, for the notice that reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Toggled {
    /// The file type now previews through its tool.
    ToTool {
        /// The normalised extension that was switched.
        ext: String,
        /// The tool's name.
        tool: String,
    },
    /// The file type now gets Trail's own preview.
    ToBuiltin {
        /// The normalised extension that was switched.
        ext: String,
    },
    /// Nothing changed: no rule covers this file. Carries the notice text.
    NoRule(String),
}

impl Toggled {
    /// The status-bar notice for this outcome.
    pub fn notice(&self) -> String {
        match self {
            Toggled::ToTool { ext, tool } => format!(".{ext}: previewed with {tool}"),
            Toggled::ToBuiltin { ext } => format!(".{ext}: built-in preview"),
            Toggled::NoRule(why) => why.clone(),
        }
    }
}

/// Switches the file type of `path` between its tool and the built-in preview
/// for the rest of the session — what `P` does.
///
/// The switch is recorded in `overrides` against the extension, so every file
/// of that type follows it until `P` again or quit. With no rule for the
/// extension, nothing is recorded and the outcome says why.
pub fn toggle_mode(
    path: &Path,
    config: &TrailConfig,
    overrides: &mut HashMap<String, PreviewMode>,
) -> Toggled {
    let Some(ext) = extension_of(path) else {
        return Toggled::NoRule("no previewer configured for files without an extension".into());
    };
    let Some(rule) = preview_tool::find_rule(&config.preview.tool, &ext) else {
        return Toggled::NoRule(format!("no previewer configured for .{ext}"));
    };
    let current = overrides.get(&ext).copied().unwrap_or(rule.default);
    let next = current.toggled();
    overrides.insert(ext.clone(), next);
    match next {
        PreviewMode::External => Toggled::ToTool {
            ext,
            tool: tool_label(&rule.command),
        },
        PreviewMode::Builtin => Toggled::ToBuiltin { ext },
    }
}

/// The name a program is shown under: its file stem.
fn tool_name(program: &str) -> String {
    Path::new(program)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(program)
        .to_owned()
}

/// Preview provider for files a `[[preview.tool]]` rule covers.
///
/// Takes a regular file only when [`PreviewCtx::external`] is set, and always
/// defers: the program runs in `workers::external_preview`, and the returned
/// [`crate::preview::provider::PreviewTask`] kills it when the selection
/// moves on.
pub struct ExternalProvider;

impl PreviewProvider for ExternalProvider {
    fn can_handle(&self, entry: &Entry, ctx: &PreviewCtx) -> bool {
        entry.kind == EntryKind::File && ctx.external.is_some()
    }

    fn preview(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome {
        let Some(spec) = ctx.external.clone() else {
            // `can_handle` guarantees a spec; answering anyway keeps this total.
            return PreviewOutcome::Ready(crate::preview::provider::PreviewContent::Empty);
        };
        let task = spawn_external_preview(
            ExternalJob {
                path: entry.path.clone(),
                spec,
                generation: ctx.generation,
                max_lines: ctx.max_preview_lines,
                max_bytes: ctx.text_sync_threshold_bytes,
                metadata: entry.metadata.clone(),
            },
            ctx.worker_tx.clone(),
        );
        PreviewOutcome::Spawned(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(rules_toml: &str) -> TrailConfig {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trail.toml");
        std::fs::write(&path, rules_toml).unwrap();
        crate::config::load(Some(&path)).unwrap()
    }

    const RULES: &str = r#"
[preview]
external_timeout_ms = 1500

[[preview.tool]]
extensions = ["pdf"]
command = ["pdftotext", "-layout", "{path}", "-"]

[[preview.tool]]
extensions = ["png"]
command = ["C:/tools/chafa.exe", "--size={width}x{height}", "{path}"]
default = "builtin"

[[preview.tool]]
extensions = ["md"]
command = "glow -w $TRAIL_PREVIEW_WIDTH \"$TRAIL_PREVIEW_PATH\""
"#;

    #[test]
    fn a_rule_resolves_to_an_expanded_argv() {
        let config = config_with(RULES);
        let path = Path::new("/docs/Report Final.PDF");
        let spec = resolve(path, &config, &HashMap::new(), (100, 30)).expect("pdf has a rule");
        assert_eq!(
            spec.argv,
            [
                OsString::from("pdftotext"),
                "-layout".into(),
                path.as_os_str().to_owned(),
                "-".into()
            ]
        );
        assert_eq!(spec.tool, "pdftotext");
        assert_eq!((spec.width, spec.height), (100, 30));
        assert_eq!(spec.timeout, Duration::from_millis(1500));
    }

    #[test]
    fn no_rule_or_no_extension_means_the_builtin_preview() {
        let config = config_with(RULES);
        let none = HashMap::new();
        assert!(resolve(Path::new("a.txt"), &config, &none, ASSUMED_PANE).is_none());
        assert!(resolve(Path::new("Makefile"), &config, &none, ASSUMED_PANE).is_none());
    }

    #[test]
    fn the_default_applies_without_an_override() {
        let config = config_with(RULES);
        let none = HashMap::new();
        assert!(
            resolve(Path::new("a.png"), &config, &none, ASSUMED_PANE).is_none(),
            "png defaults to builtin"
        );
        assert!(resolve(Path::new("a.pdf"), &config, &none, ASSUMED_PANE).is_some());
    }

    #[test]
    fn an_override_beats_the_default_both_ways() {
        let config = config_with(RULES);
        let overrides = HashMap::from([
            ("png".to_owned(), PreviewMode::External),
            ("pdf".to_owned(), PreviewMode::Builtin),
        ]);
        let png = resolve(Path::new("a.png"), &config, &overrides, (40, 12)).unwrap();
        assert_eq!(png.tool, "chafa", "the stem, not the path");
        assert_eq!(png.argv[1], OsString::from("--size=40x12"));
        assert!(resolve(Path::new("a.pdf"), &config, &overrides, ASSUMED_PANE).is_none());
    }

    #[test]
    fn the_string_form_runs_through_the_configured_shell_unexpanded() {
        let mut config = config_with(RULES);
        config.general.shell = "bash -c".to_owned();
        let spec = resolve(
            Path::new("notes.md"),
            &config,
            &HashMap::new(),
            ASSUMED_PANE,
        )
        .unwrap();
        assert_eq!(spec.tool, "glow");
        assert_eq!(
            spec.argv,
            [
                OsString::from("bash"),
                "-c".into(),
                "glow -w $TRAIL_PREVIEW_WIDTH \"$TRAIL_PREVIEW_PATH\"".into()
            ],
            "the script reaches the shell as written; the path travels in the environment"
        );
    }

    #[test]
    fn toggling_switches_the_whole_file_type_and_back() {
        let config = config_with(RULES);
        let mut overrides = HashMap::new();

        let first = toggle_mode(Path::new("a.pdf"), &config, &mut overrides);
        assert_eq!(first, Toggled::ToBuiltin { ext: "pdf".into() });
        assert_eq!(first.notice(), ".pdf: built-in preview");
        // Another file of the same type follows the switch.
        assert!(resolve(Path::new("other.PDF"), &config, &overrides, ASSUMED_PANE).is_none());

        let second = toggle_mode(Path::new("b.pdf"), &config, &mut overrides);
        assert_eq!(second.notice(), ".pdf: previewed with pdftotext");
        assert!(resolve(Path::new("a.pdf"), &config, &overrides, ASSUMED_PANE).is_some());
    }

    #[test]
    fn toggling_starts_from_the_rules_default() {
        let config = config_with(RULES);
        let mut overrides = HashMap::new();
        // png defaults to builtin, so the first P goes to the tool.
        assert_eq!(
            toggle_mode(Path::new("a.png"), &config, &mut overrides),
            Toggled::ToTool {
                ext: "png".into(),
                tool: "chafa".into()
            }
        );
    }

    #[test]
    fn toggling_without_a_rule_changes_nothing_and_says_so() {
        let config = config_with(RULES);
        let mut overrides = HashMap::new();
        assert_eq!(
            toggle_mode(Path::new("a.xyz"), &config, &mut overrides).notice(),
            "no previewer configured for .xyz"
        );
        assert_eq!(
            toggle_mode(Path::new("Makefile"), &config, &mut overrides).notice(),
            "no previewer configured for files without an extension"
        );
        assert!(overrides.is_empty());
    }

    #[test]
    fn extensions_match_case_insensitively() {
        assert_eq!(extension_of(Path::new("A.PdF")).as_deref(), Some("pdf"));
        assert_eq!(extension_of(Path::new("noext")), None);
    }
}
