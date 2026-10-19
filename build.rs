//! Decides, at compile time, whether this binary is a release or a dev build.
//!
//! A binary is a **release** when it was built from the commit tagged
//! `v<Cargo.toml version>` with no uncommitted changes, or when the release
//! workflow says so through `TRAIL_RELEASE=v<version>` (cross-compiled targets
//! build inside a container that may have no `git`). Source with no git
//! repository of its own — a crates.io or packaged tarball — is also taken as
//! a release, because that is the only place such source comes from.
//!
//! Anything else is a **dev build**, and its version says so:
//! `1.10.1-dev.3+f7863b6` is three commits past v1.10.1, at f7863b6, and a
//! trailing `.dirty` means the tree had uncommitted changes when it was built.
//!
//! Emits `TRAIL_VERSION` (the label), `TRAIL_LONG_VERSION` (the label plus a
//! sentence for `--version`) and `TRAIL_BUILD_KIND` (`release` or `dev`), read
//! by `src/version.rs`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What the git repository says about the commit being built.
struct GitState {
    /// The nearest `v*` tag and how many commits HEAD is past it.
    tag: Option<(String, u32)>,
    hash: String,
    dirty: bool,
}

fn main() {
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let release_tag = format!("v{version}");

    println!("cargo:rerun-if-env-changed=TRAIL_RELEASE");

    let declared_release = env::var("TRAIL_RELEASE").is_ok_and(|tag| tag == release_tag);
    let (label, note) = if declared_release {
        (version.clone(), None)
    } else {
        match git_state(&manifest_dir) {
            Some(git) => describe(&version, &release_tag, &git),
            None if manifest_dir.join(".git").exists() => (
                format!("{version}-dev"),
                Some("dev build — commit unknown, git could not be run".to_owned()),
            ),
            None => (version.clone(), None),
        }
    };

    let kind = if note.is_none() { "release" } else { "dev" };
    let long = match &note {
        Some(note) => format!("{label} ({note})"),
        None => label.clone(),
    };
    println!("cargo:rustc-env=TRAIL_VERSION={label}");
    println!("cargo:rustc-env=TRAIL_LONG_VERSION={long}");
    println!("cargo:rustc-env=TRAIL_BUILD_KIND={kind}");
}

/// The label and, for a dev build, the sentence explaining it.
fn describe(version: &str, release_tag: &str, git: &GitState) -> (String, Option<String>) {
    let dirty = if git.dirty { ".dirty" } else { "" };
    let changes = if git.dirty {
        ", with uncommitted changes"
    } else {
        ""
    };
    match &git.tag {
        Some((tag, 0)) if tag == release_tag && !git.dirty => (version.to_owned(), None),
        Some((tag, count)) if tag == release_tag => (
            format!("{version}-dev.{count}+{}{dirty}", git.hash),
            Some(format!(
                "dev build — {count} commit{} past {tag}{changes}",
                if *count == 1 { "" } else { "s" }
            )),
        ),
        // Cargo.toml already names a version that has not been tagged yet.
        Some((tag, count)) => (
            format!("{version}-dev+{}{dirty}", git.hash),
            Some(format!(
                "dev build — v{version} is not tagged; {count} commit{} past {tag}{changes}",
                if *count == 1 { "" } else { "s" }
            )),
        ),
        None => (
            format!("{version}-dev+{}{dirty}", git.hash),
            Some(format!("dev build — no release tag found{changes}")),
        ),
    }
}

/// Asks git about HEAD, or `None` when this source is not its own git
/// repository or git cannot be run.
fn git_state(manifest_dir: &Path) -> Option<GitState> {
    let toplevel = git(manifest_dir, &["rev-parse", "--show-toplevel"])?;
    // Source unpacked somewhere inside an unrelated repository is not ours to
    // describe.
    if Path::new(&toplevel).canonicalize().ok()? != manifest_dir.canonicalize().ok()? {
        return None;
    }
    watch_git(manifest_dir);

    let described = git(
        manifest_dir,
        &[
            "describe",
            "--tags",
            "--long",
            "--dirty",
            "--always",
            "--abbrev=7",
            "--match",
            "v[0-9]*",
        ],
    )?;
    let (described, dirty) = match described.strip_suffix("-dirty") {
        Some(rest) => (rest, true),
        None => (described.as_str(), false),
    };

    // `v1.10.1-3-gf7863b6` when a tag is reachable, a bare hash otherwise.
    let mut parts = described.rsplitn(3, '-');
    let tagged = match (parts.next(), parts.next(), parts.next()) {
        (Some(hash), Some(count), Some(tag)) => hash
            .strip_prefix('g')
            .zip(count.parse::<u32>().ok())
            .map(|(hash, count)| (tag.to_owned(), count, hash.to_owned())),
        _ => None,
    };
    Some(match tagged {
        Some((tag, count, hash)) => GitState {
            tag: Some((tag, count)),
            hash,
            dirty,
        },
        None => GitState {
            tag: None,
            hash: described.to_owned(),
            dirty,
        },
    })
}

/// Reruns this script when HEAD moves, the index changes, or the sources do —
/// the last so that `.dirty` follows an edit rather than lagging a commit.
fn watch_git(manifest_dir: &Path) {
    let git_dir = git(manifest_dir, &["rev-parse", "--absolute-git-dir"]);
    let common_dir =
        git(manifest_dir, &["rev-parse", "--git-common-dir"]).map(|dir| manifest_dir.join(dir));
    let mut watched: Vec<PathBuf> = ["src", "Cargo.toml", "Cargo.lock"]
        .iter()
        .map(|path| manifest_dir.join(path))
        .collect();
    if let Some(git_dir) = git_dir.map(PathBuf::from) {
        watched.push(git_dir.join("HEAD"));
        watched.push(git_dir.join("index"));
    }
    if let Some(common_dir) = common_dir {
        watched.push(common_dir.join("refs"));
        watched.push(common_dir.join("packed-refs"));
    }
    // A missing path counts as changed on every build, so only real ones.
    for path in watched.iter().filter(|path| path.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// Runs git in `dir` and returns its trimmed stdout, or `None` on any failure.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        // Without this `describe --dirty` refreshes the index, which touches
        // the file this script watches and makes every build rerun it.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_owned())
}
