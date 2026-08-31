//! CLI argument definitions for Trail.
//!
//! Defines the `clap`-based command-line interface: `--cwd-file` (consumed by
//! Phase 6's shell integration), `--config` (consumed by Phase 7's config
//! loading), and an optional positional start path.

use clap::Parser;
use std::path::PathBuf;

/// Trail — a terminal file manager.
#[derive(Parser, Debug)]
#[command(name = "trail", version, about = "A terminal file manager")]
pub struct Cli {
    /// Path to write the final working directory on normal exit.
    ///
    /// Used by shell wrapper functions to `cd` into the last-browsed
    /// directory after Trail exits. If omitted, no file is written.
    #[arg(long)]
    pub cwd_file: Option<PathBuf>,

    /// Path to a TOML configuration file.
    ///
    /// The path is remembered, so a later `trail` with no `--config` reloads
    /// the same file. If nothing has ever been loaded, Trail uses its
    /// built-in defaults.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Ignore the remembered `--config` file and use the built-in defaults.
    ///
    /// Affects this run only; the remembered path is left in place, so a
    /// later bare `trail` picks it up again.
    #[arg(long, conflicts_with = "config")]
    pub no_config: bool,

    /// Print every file and directory Trail uses, then exit.
    ///
    /// Reports the binary, config and data directories, the log file and any
    /// shell wrappers found, each marked with whether it exists. The locations
    /// are platform-dependent and not guessable, so this is what the uninstall
    /// documentation and scripts point at.
    #[arg(long)]
    pub paths: bool,

    /// Starting directory. Defaults to the current working directory.
    #[arg(default_value = ".")]
    pub start_path: PathBuf,
}
