//! What this binary was built as: a release, or a dev build and which commit.
//!
//! Every string here is a compile-time constant set by `build.rs`, which asks
//! git whether the commit being built is the one tagged `v<version>` with a
//! clean tree. Nothing is read or computed at run time.

/// The version label, without a leading `v`.
///
/// A release is the bare Cargo version, `1.10.1`. A dev build carries the
/// commits past that release and the commit it was built from,
/// `1.10.1-dev.3+f7863b6`, with `.dirty` appended when the tree had
/// uncommitted changes — so a build under test can never pass for a release.
pub const VERSION: &str = env!("TRAIL_VERSION");

/// [`VERSION`] followed, for a dev build, by a sentence saying what it is —
/// what `trail --version` prints after the program name.
pub const LONG_VERSION: &str = env!("TRAIL_LONG_VERSION");

/// Whether this binary is a release build. See [`VERSION`].
pub fn is_release() -> bool {
    env!("TRAIL_BUILD_KIND") == "release"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_starts_with_the_cargo_version() {
        assert!(VERSION.starts_with(env!("CARGO_PKG_VERSION")), "{VERSION}");
        assert!(LONG_VERSION.starts_with(VERSION), "{LONG_VERSION}");
    }

    #[test]
    fn a_dev_build_says_so_and_a_release_does_not() {
        if is_release() {
            assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
            assert_eq!(LONG_VERSION, VERSION);
        } else {
            assert!(VERSION.contains("-dev"), "{VERSION}");
            assert!(LONG_VERSION.contains("dev build"), "{LONG_VERSION}");
        }
    }
}
