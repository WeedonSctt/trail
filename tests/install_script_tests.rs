//! Runs the real `install.sh` against stand-ins for `uname` and `curl`, so what
//! it asks the network for and what it leaves behind can be checked offline.
//!
//! Unix only: the script is not for Windows, whose installer is `install.ps1`.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TAG: &str = "v9.8.7";
const ARCHIVE: &str = "trail-v9.8.7-x86_64-unknown-linux-gnu.tar.gz";

const FAKE_UNAME: &str = r#"#!/bin/sh
case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac
"#;

/// Logs the URL it was asked for. There is no checksums.txt, which the script
/// is allowed to carry on without.
const FAKE_CURL: &str = r#"#!/bin/sh
for url; do :; done
printf '%s\n' "$url" >> "$FAKE_URL_LOG"
case "$url" in
    */releases/latest) printf '{\n  "tag_name": "v9.8.7",\n  "name": "x"\n}\n' ;;
    */checksums.txt) exit 22 ;;
    *) cat "$FAKE_ARCHIVE" ;;
esac
"#;

struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    /// A directory holding the stand-ins, a release archive and an empty home.
    fn new() -> Sandbox {
        let dir = tempfile::tempdir().unwrap();
        let sandbox = Sandbox { dir };
        fs::create_dir(sandbox.path("bin")).unwrap();
        fs::create_dir(sandbox.path("home")).unwrap();
        fs::create_dir(sandbox.path("tmp")).unwrap();
        sandbox.script("uname", FAKE_UNAME);
        sandbox.script("curl", FAKE_CURL);

        let stem = ARCHIVE.trim_end_matches(".tar.gz");
        let shell = sandbox.path("release").join(stem).join("shell");
        fs::create_dir_all(&shell).unwrap();
        fs::write(shell.parent().unwrap().join("trail"), "binary").unwrap();
        for wrapper in ["trail.bash", "trail.zsh", "trail.fish"] {
            fs::write(shell.join(wrapper), "wrapper").unwrap();
        }
        let tarred = Command::new("tar")
            .arg("-czf")
            .arg(sandbox.path(ARCHIVE))
            .arg("-C")
            .arg(sandbox.path("release"))
            .arg(stem)
            .status()
            .unwrap();
        assert!(tarred.success());
        sandbox
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.path("bin").join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Runs the script with `version` as `VERSION`, or `None` for the default of
    /// the latest release.
    fn install(&self, script: &Path, version: Option<&str>) -> Output {
        let path = format!(
            "{}:{}",
            self.path("bin").display(),
            std::env::var("PATH").unwrap()
        );
        let mut command = Command::new("sh");
        command
            .arg(script)
            .env("PATH", path)
            .env("HOME", self.path("home"))
            .env("TMPDIR", self.path("tmp"))
            .env("FAKE_URL_LOG", self.path("urls"))
            .env("FAKE_ARCHIVE", self.path(ARCHIVE))
            .env_remove("INSTALL_DIR")
            .env_remove("SHELL_DIR")
            .env_remove("FISH_DIR")
            .env_remove("GITHUB_REPO")
            .env_remove("VERSION");
        if let Some(version) = version {
            command.env("VERSION", version);
        }
        command.output().unwrap()
    }

    fn requested_urls(&self) -> Vec<String> {
        fs::read_to_string(self.path("urls"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn assert_installed(&self, output: &Output) {
        assert!(
            output.status.success(),
            "the script failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let home = self.path("home");
        assert!(home.join(".local/bin/trail").is_file());
        assert!(home.join(".local/share/trail/shell/trail.bash").is_file());
        assert_eq!(
            fs::read_dir(self.path("tmp")).unwrap().count(),
            0,
            "the temporary directory was left behind"
        );
    }
}

/// `install.sh` at the root of the repository. The published crate leaves it
/// out, so its tests have nothing to run.
fn install_script() -> Option<PathBuf> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    script.exists().then_some(script)
}

fn release_url(file: &str) -> String {
    format!("https://github.com/WeedonSctt/trail/releases/download/{TAG}/{file}")
}

/// The documented one-liner sets no `VERSION`. A status message written to stdout
/// used to be captured with the version, which put it inside the download URL
/// and made curl refuse it.
#[test]
fn the_latest_release_is_resolved_and_downloaded() {
    let Some(script) = install_script() else {
        return;
    };
    let sandbox = Sandbox::new();

    let output = sandbox.install(&script, None);

    sandbox.assert_installed(&output);
    assert_eq!(
        sandbox.requested_urls(),
        [
            "https://api.github.com/repos/WeedonSctt/trail/releases/latest".to_owned(),
            release_url(ARCHIVE),
            release_url("checksums.txt"),
        ]
    );
}

#[test]
fn a_pinned_version_asks_for_nothing_else() {
    let Some(script) = install_script() else {
        return;
    };
    let sandbox = Sandbox::new();

    let output = sandbox.install(&script, Some("9.8.7"));

    sandbox.assert_installed(&output);
    assert_eq!(
        sandbox.requested_urls(),
        [release_url(ARCHIVE), release_url("checksums.txt")]
    );
}
