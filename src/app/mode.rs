//! Mode enum: Navigation, Search, and Command.
//!
//! Determines how keystrokes are dispatched and what the status bar displays.
//! All three variants are defined here so the state shape is complete from
//! Phase 1 onward; only `Navigation` is functionally wired this phase.

/// The current interaction mode of the application.
///
/// - `Navigation` — default; `j`/`k`/`l`/`h` browse the directory tree.
/// - `Search` — entered via `/`; keystrokes build a fuzzy query (Phase 2).
/// - `Command` — entered via `:`; keystrokes build a command line (Phase 3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mode {
    /// Default navigation mode — arrow keys / hjkl move the selection.
    #[default]
    Navigation,

    /// Fuzzy-search mode. `query` is the current filter string; `matches`
    /// holds indices into `AppState::entries` sorted by score.
    ///
    /// Functionally wired in Phase 2; the variant is defined here so
    /// `AppState` carries the correct shape from the start.
    Search {
        /// Current filter query typed by the user.
        query: String,
        /// Indices into `AppState::entries`, ordered by match score.
        matches: Vec<usize>,
    },

    /// Command-line mode. `buffer` is the text typed after `:` or `!`;
    /// `cursor` is the byte offset of the insertion point.
    ///
    /// Functionally wired in Phase 3; the variant is defined here so
    /// `AppState` carries the correct shape from the start.
    Command {
        /// Text content of the command line (excluding the leading `:` or `!`).
        buffer: String,
        /// Byte offset of the cursor within `buffer`.
        cursor: usize,
        /// Index into the command history when scrolling through past commands.
        history_index: Option<usize>,
    },
}

impl Mode {
    /// Returns a short, human-readable label for the status bar.
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Navigation => "NORMAL",
            Mode::Search { .. } => "SEARCH",
            Mode::Command { .. } => "COMMAND",
        }
    }
}
