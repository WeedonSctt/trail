//! `[[preview.tool]]` rules: which external program previews which file types.
//!
//! Each rule names some extensions and a command. The command comes in two
//! forms, and the difference between them is a security boundary:
//!
//! - **A list** is spawned directly, with no shell. `{path}`, `{width}` and
//!   `{height}` are substituted *within* an argument, so a file name can never
//!   become a second argument, let alone a second command.
//! - **A string** runs through `[general] shell`, so pipes work. Placeholders
//!   are a config error there: pasting a file name into shell text is
//!   injection (a file called `x & del /s *.pdf`), and the quoting rules differ
//!   between cmd, PowerShell and sh. The string reads the path from the
//!   environment instead — see [`ENV_PATH`].
//!
//! The rules are validated at load; resolving one for an entry and spawning it
//! is [`crate::preview::external`]'s job.

use std::ffi::OsString;
use std::path::Path;

use serde::{Deserialize, Deserializer};

use crate::config::SetConfigError;

/// Environment variable holding the previewed file's path, set for both forms.
pub const ENV_PATH: &str = "TRAIL_PREVIEW_PATH";
/// Environment variable holding the preview pane's width in columns.
pub const ENV_WIDTH: &str = "TRAIL_PREVIEW_WIDTH";
/// Environment variable holding the preview pane's height in rows.
pub const ENV_HEIGHT: &str = "TRAIL_PREVIEW_HEIGHT";

/// Smallest accepted `[preview] external_timeout_ms`.
///
/// Below this, starting the process can take longer than the budget on a cold
/// cache, and every preview would time out.
pub const EXTERNAL_TIMEOUT_MS_MIN: u64 = 100;
/// Largest accepted `[preview] external_timeout_ms`: a minute is past any
/// preview worth waiting for, so this constrains only typos.
pub const EXTERNAL_TIMEOUT_MS_MAX: u64 = 60_000;

/// Explanation attached to a rejected `[preview] external_timeout_ms`.
pub const EXTERNAL_TIMEOUT_REASON: &str = "must be between 100 and 60000";

/// Why `:set` refuses `preview.tool`.
pub const TOOL_NOT_SETTABLE: &str =
    "the previewer rules are a list of tables, which `:set key value` \
     cannot express; edit them in the config file, and press P to switch a file type's \
     previewer for this session";

/// Which preview a file type gets: the configured tool's, or Trail's own.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PreviewMode {
    /// Run the rule's command and show its output.
    #[default]
    External,
    /// Use the preview Trail would show with no rule at all.
    Builtin,
}

impl PreviewMode {
    /// The other mode — what `P` switches to.
    #[must_use]
    pub fn toggled(self) -> Self {
        match self {
            Self::External => Self::Builtin,
            Self::Builtin => Self::External,
        }
    }
}

/// One `[[preview.tool]]` table.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreviewToolRule {
    /// Extensions this rule covers, lowercased and without a leading `.` —
    /// normalised as they are read, so `".PDF"` and `"pdf"` are the same rule.
    #[serde(deserialize_with = "normalized_extensions")]
    pub extensions: Vec<String>,
    /// What to run.
    pub command: ToolCommand,
    /// Which preview the file type starts in. Omitted means `external`; `P`
    /// switches it for the session.
    #[serde(default)]
    pub default: PreviewMode,
}

/// A rule's command, in one of its two forms. See the module docs for why the
/// difference matters.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ToolCommand {
    /// Spawned directly: the program, then its arguments, each of which may
    /// contain placeholders.
    Argv(Vec<String>),
    /// Run through `[general] shell`. No placeholders; the path is in
    /// [`ENV_PATH`].
    Shell(String),
}

/// The values placeholders expand to.
#[derive(Debug, Clone, Copy)]
pub struct Placeholders<'a> {
    /// The file being previewed.
    pub path: &'a Path,
    /// The preview pane's interior width in columns.
    pub width: u16,
    /// The preview pane's interior height in rows.
    pub height: u16,
}

/// Lowercases `ext` and strips any leading `.`, the form rules are stored and
/// looked up in.
#[must_use]
pub fn normalize_extension(ext: &str) -> String {
    ext.trim().trim_start_matches('.').to_lowercase()
}

fn normalized_extensions<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let raw = Vec::<String>::deserialize(d)?;
    Ok(raw.iter().map(|e| normalize_extension(e)).collect())
}

/// The rule covering `ext` (already normalised), if any.
///
/// [`validate`] guarantees an extension appears in at most one rule, so the
/// first match is the only one.
#[must_use]
pub fn find_rule<'a>(rules: &'a [PreviewToolRule], ext: &str) -> Option<&'a PreviewToolRule> {
    rules.iter().find(|r| r.extensions.iter().any(|e| e == ext))
}

/// Expands the placeholders in one list-form argument.
///
/// `{path}`, `{width}` and `{height}` are substituted; `{{` and `}}` are a
/// literal brace, for the tools that want one. A `}` with no `{` before it is
/// taken literally too, since it cannot be misread.
///
/// # Errors
///
/// Returns a description of the problem for any other `{…}`, or a `{` that is
/// never closed — at config load, through [`validate`], rather than at the
/// first preview.
pub fn expand_arg(arg: &str, values: &Placeholders<'_>) -> Result<OsString, String> {
    let mut out = OsString::new();
    let mut literal = String::new();
    let mut chars = arg.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => {
                            return Err(format!(
                                "'{{' is never closed in {arg:?}; write '{{{{' for a literal brace"
                            ))
                        }
                    }
                }
                out.push(&literal);
                literal.clear();
                match name.as_str() {
                    "path" => out.push(values.path.as_os_str()),
                    "width" => out.push(values.width.to_string()),
                    "height" => out.push(values.height.to_string()),
                    _ => {
                        return Err(format!(
                            "unknown placeholder {{{name}}} in {arg:?}; the placeholders are \
                             {{path}}, {{width}} and {{height}}, and '{{{{' is a literal brace"
                        ))
                    }
                }
            }
            c => literal.push(c),
        }
    }
    out.push(&literal);
    Ok(out)
}

/// Checks every rule, so a mistake is reported at startup with the rule it is
/// in rather than as a failed preview later.
///
/// # Errors
///
/// Returns [`SetConfigError::InvalidValue`] for a rule with no extensions or a
/// blank one, an extension claimed by two rules, an empty command or program,
/// an unknown or unclosed placeholder in the list form, or any placeholder in
/// the string form. Rules are numbered from 1 in the order they appear.
pub fn validate(rules: &[PreviewToolRule]) -> Result<(), SetConfigError> {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        let n = index + 1;
        let key = |field: &str| format!("preview.tool #{n} {field}");

        if rule.extensions.is_empty() {
            return Err(invalid(
                &key("extensions"),
                "[]",
                "must name at least one extension",
            ));
        }
        for ext in &rule.extensions {
            if ext.is_empty() {
                return Err(invalid(
                    &key("extensions"),
                    "",
                    "an extension must not be blank",
                ));
            }
            if let Some((_, first)) = seen.iter().find(|(e, _)| e == ext) {
                return Err(invalid(
                    &key("extensions"),
                    ext,
                    &format!("already covered by rule #{first}; an extension belongs to one rule"),
                ));
            }
            seen.push((ext, n));
        }

        match &rule.command {
            ToolCommand::Argv(argv) => {
                let Some(program) = argv.first() else {
                    return Err(invalid(&key("command"), "[]", "must not be empty"));
                };
                if program.trim().is_empty() {
                    return Err(invalid(
                        &key("command"),
                        program,
                        "the program must not be blank",
                    ));
                }
                let dummy = Placeholders {
                    path: Path::new("x"),
                    width: 1,
                    height: 1,
                };
                for arg in argv {
                    expand_arg(arg, &dummy)
                        .map_err(|reason| invalid(&key("command"), arg, &reason))?;
                }
            }
            ToolCommand::Shell(script) => {
                if script.trim().is_empty() {
                    return Err(invalid(&key("command"), script, "must not be empty"));
                }
                if let Some(found) = ["{path}", "{width}", "{height}"]
                    .iter()
                    .find(|p| script.contains(*p))
                {
                    return Err(invalid(
                        &key("command"),
                        script,
                        &format!(
                            "{found} is not substituted in the string form -- pasting a file \
                             name into shell text is injection. Read ${ENV_PATH} \
                             (sh, PowerShell) or %{ENV_PATH}% (cmd) instead, or write the \
                             command as a list"
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn invalid(key: &str, value: &str, reason: &str) -> SetConfigError {
    SetConfigError::InvalidValue {
        key: key.to_owned(),
        value: value.to_owned(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(toml_text: &str) -> Vec<PreviewToolRule> {
        #[derive(Deserialize)]
        struct Wrapper {
            tool: Vec<PreviewToolRule>,
        }
        toml::from_str::<Wrapper>(toml_text).unwrap().tool
    }

    fn values() -> Placeholders<'static> {
        Placeholders {
            path: Path::new("/tmp/a b.pdf"),
            width: 80,
            height: 24,
        }
    }

    #[test]
    fn both_command_forms_parse() {
        let r = rules(
            r#"
[[tool]]
extensions = ["pdf"]
command = ["pdftotext", "{path}", "-"]

[[tool]]
extensions = ["md"]
command = "glow -w $TRAIL_PREVIEW_WIDTH \"$TRAIL_PREVIEW_PATH\""
default = "builtin"
"#,
        );
        assert!(matches!(&r[0].command, ToolCommand::Argv(a) if a[0] == "pdftotext"));
        assert_eq!(
            r[0].default,
            PreviewMode::External,
            "omitted means external"
        );
        assert!(matches!(&r[1].command, ToolCommand::Shell(s) if s.starts_with("glow")));
        assert_eq!(r[1].default, PreviewMode::Builtin);
        validate(&r).unwrap();
    }

    #[test]
    fn extensions_are_lowercased_and_lose_a_leading_dot() {
        let r = rules("[[tool]]\nextensions = [\".PDF\", \"Md\"]\ncommand = [\"x\"]\n");
        assert_eq!(r[0].extensions, ["pdf", "md"]);
        assert!(find_rule(&r, "pdf").is_some());
        assert!(find_rule(&r, "txt").is_none());
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct Wrapper {
            tool: Vec<PreviewToolRule>,
        }
        let err = toml::from_str::<Wrapper>(
            "[[tool]]\nextensions = [\"pdf\"]\ncommand = [\"x\"]\ntimeout = 5\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("timeout"), "{err}");
    }

    #[test]
    fn a_bad_default_is_rejected_with_the_options() {
        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct Wrapper {
            tool: Vec<PreviewToolRule>,
        }
        let err = toml::from_str::<Wrapper>(
            "[[tool]]\nextensions = [\"pdf\"]\ncommand = [\"x\"]\ndefault = \"sometimes\"\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("external"), "{err}");
    }

    #[test]
    fn empty_commands_and_extensions_are_rejected() {
        for bad in [
            "[[tool]]\nextensions = [\"pdf\"]\ncommand = []\n",
            "[[tool]]\nextensions = [\"pdf\"]\ncommand = [\"  \", \"x\"]\n",
            "[[tool]]\nextensions = [\"pdf\"]\ncommand = \"  \"\n",
            "[[tool]]\nextensions = []\ncommand = [\"x\"]\n",
            "[[tool]]\nextensions = [\".\"]\ncommand = [\"x\"]\n",
        ] {
            assert!(validate(&rules(bad)).is_err(), "should be rejected:\n{bad}");
        }
    }

    #[test]
    fn an_extension_in_two_rules_is_rejected_naming_both() {
        let r = rules(
            "[[tool]]\nextensions = [\"png\"]\ncommand = [\"a\"]\n\
             [[tool]]\nextensions = [\"jpg\", \".PNG\"]\ncommand = [\"b\"]\n",
        );
        let err = validate(&r).unwrap_err().to_string();
        assert!(
            err.contains("#2") && err.contains("#1") && err.contains("png"),
            "{err}"
        );
    }

    #[test]
    fn unknown_or_unclosed_placeholders_are_rejected_in_the_list_form() {
        for bad in ["{file}", "{}", "--size={width}x{height", "{PATH}"] {
            let r = rules(&format!(
                "[[tool]]\nextensions = [\"pdf\"]\ncommand = [\"x\", {bad:?}]\n"
            ));
            assert!(validate(&r).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn any_placeholder_is_rejected_in_the_string_form() {
        for script in ["glow {path}", "chafa --size {width}x24 f", "x {height}"] {
            let r = rules(&format!(
                "[[tool]]\nextensions = [\"md\"]\ncommand = {script:?}\n"
            ));
            let err = validate(&r).unwrap_err().to_string();
            assert!(
                err.contains(ENV_PATH),
                "the error points at the env var: {err}"
            );
        }
        // Braces that are not placeholders are shell syntax and stay allowed.
        let r = rules(
            "[[tool]]\nextensions = [\"md\"]\ncommand = \"cat \\\"${TRAIL_PREVIEW_PATH}\\\"\"\n",
        );
        validate(&r).unwrap();
    }

    #[test]
    fn placeholders_expand_inside_one_argument() {
        let v = values();
        assert_eq!(
            expand_arg("{path}", &v).unwrap(),
            OsString::from("/tmp/a b.pdf")
        );
        assert_eq!(
            expand_arg("--size={width}x{height}", &v).unwrap(),
            OsString::from("--size=80x24")
        );
        assert_eq!(
            expand_arg("{{literal}}", &v).unwrap(),
            OsString::from("{literal}")
        );
        assert_eq!(expand_arg("a}b", &v).unwrap(), OsString::from("a}b"));
        assert_eq!(expand_arg("plain", &v).unwrap(), OsString::from("plain"));
    }

    #[test]
    fn toggling_a_mode_twice_returns_to_it() {
        assert_eq!(PreviewMode::External.toggled(), PreviewMode::Builtin);
        assert_eq!(
            PreviewMode::External.toggled().toggled(),
            PreviewMode::External
        );
    }
}
