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
    // TODO(phase-6): Consumed by session.rs on normal exit.
    #[arg(long)]
    pub cwd_file: Option<PathBuf>,

    /// Path to a TOML configuration file.
    ///
    /// Overrides the default config location. If omitted, Trail uses
    /// built-in defaults (and, once Phase 7 lands, the platform-standard
    /// config directory).
    // TODO(phase-7): Consumed by config loading at startup.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Starting directory. Defaults to the current working directory.
    #[arg(default_value = ".")]
    pub start_path: PathBuf,
}
