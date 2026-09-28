//! Snapshot tests for the three-panel layout using `ratatui::backend::TestBackend`.
//!
//! Each test renders an `AppState` fixture through `ui::render` and asserts
//! structural properties of the terminal buffer output.
//!
//! To add true insta snapshot assertions later, uncomment the `assert_snapshot!`
//! macro calls and run `cargo insta review` to review and accept the initial
//! snapshots.

use std::fs;

use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;

use trail::app::state::AppState;
use trail::preview;
use trail::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewRegistry};
use trail::workers::WorkerMsg;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Creates a deterministic temp directory for rendering tests.
///
/// Layout:
/// ```text
/// <tmp>/
///   alpha_dir/
///   b_file.txt   ("hello world")
/// ```
fn make_fixture_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("alpha_dir")).unwrap();
    fs::write(dir.path().join("b_file.txt"), b"hello world\n").unwrap();
    dir
}

/// Initialises state, runs the preview registry, then renders into a
/// `TestBackend` of `width × height` and returns the buffer content as a string.
async fn render_to_string(state: &mut AppState, width: u16, height: u16) -> String {
    let mut registry = PreviewRegistry::new();
    preview::register_defaults(&mut registry);

    // Compute preview for the current selection.
    if let Some(entry) = state.selected_entry().cloned() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let ctx = PreviewCtx {
            show_hidden: state.show_hidden,
            worker_tx: tx,
            generation: state.preview.generation,
            text_sync_threshold_bytes: state.config.general.text_sync_threshold_kb * 1024,
            max_preview_lines: state.config.preview.max_lines,
        };
        let content = match registry.preview_for(&entry, &ctx) {
            PreviewOutcome::Ready(c) => c,
            PreviewOutcome::Deferred => {
                if let Ok(Some(WorkerMsg::Preview { content, .. })) =
                    tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await
                {
                    content
                } else {
                    PreviewContent::Loading
                }
            }
        };
        state.preview.content = content;
        state.preview.for_path = entry.path.clone();
    }

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");

    trail::ui::render(&mut terminal, state).expect("render failed");

    // Convert the terminal buffer to a string for snapshot comparison.
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..height {
        for x in 0..width {
            out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
        }
        out.push('\n');
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Navigation Mode with a two-entry directory: the nav panel should list both
/// entries (dir first, then file), and the status bar should show "NORMAL".
#[tokio::test]
async fn navigation_mode_renders_listing() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let rendered = render_to_string(&mut state, 80, 24).await;

    assert!(
        !rendered.trim().is_empty(),
        "render output must not be empty"
    );

    // "NORMAL" mode badge must be present in the status bar.
    assert!(
        rendered.contains("NORMAL"),
        "status bar must show NORMAL mode badge"
    );

    // The navigation panel must contain the directory entry.
    assert!(
        rendered.contains("alpha_dir"),
        "nav panel must show alpha_dir"
    );

    // The navigation panel must contain the file entry.
    assert!(
        rendered.contains("b_file.txt"),
        "nav panel must show b_file.txt"
    );
}

/// When the selected entry is a directory, the preview panel should show the
/// directory summary (counts), not text content.
#[tokio::test]
async fn directory_preview_shown_for_dir_selection() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    // Selection starts at 0 → alpha_dir (a directory).
    assert_eq!(state.selected, 0);

    let rendered = render_to_string(&mut state, 100, 30).await;

    // Directory preview should show "dirs" or "files" summary text.
    assert!(
        rendered.contains("dirs") || rendered.contains("files"),
        "directory preview panel must contain dir/file count summary"
    );
}

/// When the selected entry is a text file, the preview panel should show the
/// file content with line numbers.
#[tokio::test]
async fn text_preview_shown_for_file_selection() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    // Move to b_file.txt (index 1, after alpha_dir).
    state.move_down();
    assert_eq!(state.selected, 1);

    let rendered = render_to_string(&mut state, 100, 30).await;

    // The text preview should contain the file contents.
    assert!(
        rendered.contains("hello"),
        "text preview panel must contain file content"
    );
    // Line number column must be present.
    assert!(
        rendered.contains('1'),
        "text preview must include line numbers"
    );
}

/// After navigating into a subdirectory, the nav panel title and status bar
/// must reflect the new cwd.
#[tokio::test]
async fn entering_dir_updates_cwd_display() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let subdir = dir.path().join("alpha_dir");
    state.enter_dir(subdir).unwrap();

    let rendered = render_to_string(&mut state, 300, 30).await;

    assert!(
        rendered.contains("alpha_dir"),
        "status bar / nav panel title must reflect the entered directory"
    );
}

/// Hidden files are not visible by default; toggling show_hidden causes
/// them to appear.
#[tokio::test]
async fn hidden_files_visible_after_toggle() {
    let dir = make_fixture_dir();
    // Add a hidden file.
    fs::write(dir.path().join(".secret"), b"").unwrap();

    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    // Hidden file should NOT appear before toggle.
    let before = render_to_string(&mut state, 100, 30).await;
    assert!(
        !before.contains(".secret"),
        "hidden file must not appear before toggle_hidden"
    );

    // Toggle hidden files on.
    state.toggle_hidden().unwrap();
    let after = render_to_string(&mut state, 100, 30).await;
    assert!(
        after.contains(".secret"),
        "hidden file must appear after toggle_hidden"
    );
}

/// Entry count in the status bar must match visible_count().
#[tokio::test]
async fn status_bar_shows_entry_count() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let count = state.visible_count();
    let rendered = render_to_string(&mut state, 100, 30).await;

    assert!(
        rendered.contains(&count.to_string()),
        "status bar must display the visible entry count ({count})"
    );
}

// ── Preview scrolling ─────────────────────────────────────────────────────────

/// Number of interior rows the preview pane has in an 80×24 terminal: 24 rows
/// less the status bar, less the panel's top and bottom borders.
const PREVIEW_ROWS: usize = 21;

/// Creates a directory holding one 200-line text file, each line tagged with its
/// own number so the rendered output says exactly which lines are on screen.
fn make_long_file_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let body: String = (1..=200).map(|i| format!("L{i:04} marker\n")).collect();
    fs::write(dir.path().join("long.txt"), body).unwrap();
    dir
}

#[tokio::test]
async fn preview_starts_at_the_first_line() {
    let dir = make_long_file_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let rendered = render_to_string(&mut state, 80, 24).await;

    assert!(
        rendered.contains("L0001 marker"),
        "an unscrolled preview starts at line 1:\n{rendered}"
    );
    assert!(
        rendered.contains(&format!("L{PREVIEW_ROWS:04} marker")),
        "the pane's last row shows the last line that fits:\n{rendered}"
    );
    assert!(
        !rendered.contains(&format!("L{:04} marker", PREVIEW_ROWS + 1)),
        "nothing past the pane's height is drawn:\n{rendered}"
    );
}

/// The preview pane is scrolled by slicing the loaded lines, and the line
/// numbers must keep counting from the file's start — a scrolled preview that
/// restarts its numbering at 1 is the regression this guards.
#[tokio::test]
async fn scrolled_preview_shows_later_lines_with_their_own_numbers() {
    let dir = make_long_file_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.preview.scroll = 40;

    let rendered = render_to_string(&mut state, 80, 24).await;

    assert!(
        rendered.contains("41  L0041 marker"),
        "line 41 must be drawn, numbered 41:\n{rendered}"
    );
    assert!(
        !rendered.contains("L0001 marker"),
        "the first screenful must have scrolled away:\n{rendered}"
    );
    // first–last/total, with last = 40 + 21 rows.
    assert!(
        rendered.contains("41–61/200"),
        "the footer must report the visible range:\n{rendered}"
    );
}

#[tokio::test]
async fn preview_that_fits_has_no_position_footer() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    // Select b_file.txt (one line) rather than the directory.
    state.move_down();

    let rendered = render_to_string(&mut state, 80, 24).await;

    assert!(
        rendered.contains("hello world"),
        "the file's content must be previewed:\n{rendered}"
    );
    assert!(
        !rendered.contains("1–1/1"),
        "content that fits needs no position footer:\n{rendered}"
    );
}

/// A preview cut short by `[preview] max_lines` must say so: the footer's `+`
/// is the only thing standing between the reader and a pane that looks like a
/// whole file but isn't.
#[tokio::test]
async fn truncated_preview_is_marked_in_the_footer() {
    let dir = make_long_file_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.preview.truncated = true;

    let rendered = render_to_string(&mut state, 80, 24).await;

    assert!(
        rendered.contains("1–21/200+"),
        "a truncated preview must carry the '+' marker:\n{rendered}"
    );
}

// ── Status bar ────────────────────────────────────────────────────────────────

/// Renders `state` and returns both the buffer text and where the cursor ended
/// up, which is the only way to observe that Command Mode asked for one.
async fn render_with_cursor(
    state: &mut AppState,
    width: u16,
    height: u16,
) -> (String, Option<(u16, u16)>) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
    trail::ui::render(&mut terminal, state).expect("render failed");

    let cursor = terminal.get_cursor_position().ok().map(|p| (p.x, p.y));
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..height {
        for x in 0..width {
            out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
        }
        out.push('\n');
    }
    (out, cursor)
}

/// The bug this guards: Command Mode tracked an insertion point that nothing
/// drew, so editing anywhere but the end of the line was blind.
#[tokio::test]
async fn command_mode_puts_the_cursor_at_the_insertion_point() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.mode = trail::app::mode::Mode::Command {
        buffer: "mkdir notes".to_owned(),
        cursor: 5, // just after "mkdir"
        history_index: None,
    };

    let (rendered, cursor) = render_with_cursor(&mut state, 80, 24).await;

    assert!(
        rendered.contains(":mkdir notes"),
        "the command line must show the prompt and the buffer:\n{rendered}"
    );
    // Column 0 is the ':' prompt, so byte 5 of the buffer is column 6, on the
    // last row of the frame.
    assert_eq!(cursor, Some((6, 23)));
}

/// A command longer than the terminal has to scroll, not disappear: the tail is
/// what the user is typing.
#[tokio::test]
async fn a_long_command_scrolls_to_keep_the_end_visible() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let long = format!("mv {}", "d".repeat(60));
    state.mode = trail::app::mode::Mode::Command {
        cursor: long.len(),
        buffer: long,
        history_index: None,
    };

    let (rendered, cursor) = render_with_cursor(&mut state, 40, 24).await;

    assert!(
        rendered.contains(&"d".repeat(38)),
        "the end of the command must be on screen:\n{rendered}"
    );
    assert!(
        !rendered.contains(":mv"),
        "the start has scrolled off, so the prompt is gone too:\n{rendered}"
    );
    assert_eq!(cursor, Some((39, 23)), "the cursor sits in the last column");
}

/// A message gets the counters' room as well, and says so when it still does
/// not fit — a silently cut error reads as an error about half a path.
#[tokio::test]
async fn a_long_error_is_elided_rather_than_cut() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.set_error("mv → destination already exists and cannot be overwritten here");

    let rendered = render_to_string(&mut state, 80, 24).await;

    assert!(
        rendered.contains("Error: mv → destination already exists"),
        "the message must have the room the counters were using:\n{rendered}"
    );
    assert!(
        rendered.contains('…'),
        "what does not fit must be marked as dropped:\n{rendered}"
    );
    assert!(
        !rendered.contains("items"),
        "the counters stand aside for a transient message:\n{rendered}"
    );
}

/// The bug this guards: `Tab` and `Shift-Tab` changed the focused tab with
/// nothing on screen to say so, which is indistinguishable from a dead key.
#[tokio::test]
async fn the_focused_tab_is_shown_once_there_is_more_than_one() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    let rendered = render_to_string(&mut state, 80, 24).await;
    assert!(
        !rendered.contains("[1/1]"),
        "one tab needs no indicator; the path wants the room:\n{rendered}"
    );

    state.open_tab(None).unwrap();
    let rendered = render_to_string(&mut state, 80, 24).await;
    assert!(
        rendered.contains("[2/2]"),
        "the second tab must be identified:\n{rendered}"
    );

    trail::actions::apply(trail::actions::Action::SwitchTabPrev, &mut state).unwrap();
    let rendered = render_to_string(&mut state, 80, 24).await;
    assert!(
        rendered.contains("[1/2]"),
        "switching back must be visible:\n{rendered}"
    );
}

/// A switch with one tab open is a no-op, so it says so instead of looking like
/// a binding that does not work.
#[tokio::test]
async fn switching_with_one_tab_explains_itself() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();

    trail::actions::apply(trail::actions::Action::SwitchTabNext, &mut state).unwrap();

    let rendered = render_to_string(&mut state, 80, 24).await;
    assert!(
        rendered.contains("only one tab open"),
        "the no-op must be explained:\n{rendered}"
    );
    assert!(state.error_text().is_none(), "it is a hint, not a failure");
}

/// The bug this guards: the full path was drawn twice — once as the nav panel's
/// border title and once in the status bar — so the title's width went on a
/// second copy of what was already on screen.
#[tokio::test]
async fn the_full_path_is_drawn_once_and_the_title_names_the_directory() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let subdir = state.cwd.join("alpha_dir");
    state.enter_dir(subdir).unwrap();

    // Wide enough that the status bar's half is not what truncates the path.
    let rendered = render_to_string(&mut state, 300, 30).await;
    let full_path = state.status.cwd_display.clone();

    assert_eq!(
        rendered.matches(&full_path).count(),
        1,
        "the full path belongs in the status bar only:\n{rendered}"
    );
    assert!(
        rendered.contains("╭ alpha_dir "),
        "the panel title should name the directory:\n{rendered}"
    );
}

/// The delete prompt names the outcome rather than just the act: with a recycle
/// bin in play, "Delete" and "Recycle" are different promises.
#[tokio::test]
async fn the_delete_prompt_says_where_the_entry_is_going() {
    let dir = make_fixture_dir();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    state.pending_delete = true;

    let rendered = render_to_string(&mut state, 100, 24).await;
    assert!(
        rendered.contains("Recycle 'alpha_dir'?"),
        "the default sends it to the recycle bin:\n{rendered}"
    );

    state.config.set_value("delete_mode", "permanent").unwrap();
    let rendered = render_to_string(&mut state, 100, 24).await;
    assert!(
        rendered.contains("Delete 'alpha_dir'?"),
        "a permanent delete must not read as recoverable:\n{rendered}"
    );
}
