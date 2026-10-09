//! Which program a new panel shell runs: a named `[[terminal.profile]]`, or the
//! platform's own interactive shell when none is configured.

use crate::config::TerminalConfig;

/// A resolved profile: the name shown on the panel's tab, and the argv to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// The profile's name, shown as `<n>:<name>` in the tab strip.
    pub name: String,
    /// Program followed by its arguments. Never empty.
    pub argv: Vec<String>,
}

/// Resolves the profile a new shell should use.
///
/// `requested` is the name given to `:term new <profile>`; `None` means the
/// default — `[terminal] default_profile`, else the first configured profile,
/// else the built-in one.
///
/// # Errors
///
/// Returns a message naming the available profiles when `requested`, or the
/// configured default, names a profile that does not exist.
pub fn resolve(config: &TerminalConfig, requested: Option<&str>) -> Result<Profile, String> {
    let wanted = requested
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| Some(config.default_profile.trim()).filter(|name| !name.is_empty()));

    let Some(wanted) = wanted else {
        return Ok(match config.profile.first() {
            Some(first) => Profile {
                name: first.name.clone(),
                argv: first.command.clone(),
            },
            None => builtin(),
        });
    };

    if let Some(found) = config
        .profile
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(wanted))
    {
        return Ok(Profile {
            name: found.name.clone(),
            argv: found.command.clone(),
        });
    }
    let builtin = builtin();
    if config.profile.is_empty() && builtin.name.eq_ignore_ascii_case(wanted) {
        return Ok(builtin);
    }

    let names: Vec<&str> = if config.profile.is_empty() {
        vec![builtin_name()]
    } else {
        config.profile.iter().map(|p| p.name.as_str()).collect()
    };
    Err(format!(
        "no terminal profile '{wanted}' (available: {})",
        names.join(", ")
    ))
}

/// The shell used when no profile is configured: PowerShell 7 if it is on
/// `PATH`, else Windows PowerShell; on other platforms `$SHELL`, else `/bin/sh`.
#[must_use]
pub fn builtin() -> Profile {
    if cfg!(windows) {
        let program = if on_path("pwsh.exe") {
            "pwsh.exe"
        } else {
            "powershell.exe"
        };
        Profile {
            name: builtin_name().to_owned(),
            argv: vec![program.to_owned(), "-NoLogo".to_owned()],
        }
    } else {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "/bin/sh".to_owned());
        let name = std::path::Path::new(&shell)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("sh")
            .to_owned();
        Profile {
            name,
            argv: vec![shell],
        }
    }
}

fn builtin_name() -> &'static str {
    if cfg!(windows) {
        "pwsh"
    } else {
        "shell"
    }
}

/// Whether `program` is a file in one of `PATH`'s directories. A handful of
/// `stat` calls — cheap enough for the moment a shell is opened.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TerminalProfile;

    fn config(default: &str, profiles: &[(&str, &[&str])]) -> TerminalConfig {
        let mut cfg = crate::config::load(None).unwrap().terminal;
        cfg.default_profile = default.to_owned();
        cfg.profile = profiles
            .iter()
            .map(|(name, argv)| TerminalProfile {
                name: (*name).to_owned(),
                command: argv.iter().map(|s| (*s).to_owned()).collect(),
            })
            .collect();
        cfg
    }

    #[test]
    fn no_profiles_means_the_builtin_shell() {
        let resolved = resolve(&config("", &[]), None).unwrap();
        assert_eq!(resolved, builtin());
        assert!(!resolved.argv.is_empty());
    }

    #[test]
    fn the_default_is_the_named_profile_else_the_first() {
        let profiles: &[(&str, &[&str])] = &[("pwsh", &["pwsh"]), ("gitbash", &["bash", "-i"])];
        assert_eq!(resolve(&config("", profiles), None).unwrap().name, "pwsh");
        assert_eq!(
            resolve(&config("gitbash", profiles), None).unwrap().argv,
            vec!["bash", "-i"]
        );
    }

    #[test]
    fn a_requested_profile_wins_and_an_unknown_one_lists_the_choices() {
        let profiles: &[(&str, &[&str])] = &[("pwsh", &["pwsh"]), ("gitbash", &["bash"])];
        let cfg = config("pwsh", profiles);
        assert_eq!(resolve(&cfg, Some("GitBash")).unwrap().name, "gitbash");
        let err = resolve(&cfg, Some("zsh")).unwrap_err();
        assert!(err.contains("pwsh, gitbash"), "{err}");
    }
}
