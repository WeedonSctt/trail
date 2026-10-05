//! External previewer worker: runs a `[[preview.tool]]` program and reports
//! its output as a `WorkerMsg::Preview`.
//!
//! The program is a stranger, so everything about running it is bounded:
//!
//! - **Isolated from the terminal.** stdin is null and stdout and stderr are
//!   piped. A child that inherited them would draw over the alternate screen
//!   (invariant 7) and read the user's keystrokes. On Windows it is created
//!   with `CREATE_NO_WINDOW`, so no console flashes up.
//! - **Bounded in output.** stdout is read until `max_lines` lines or
//!   `max_bytes` bytes, and then the child is killed — a `cat`-like tool on a
//!   2 GB file costs the cap, not the file.
//! - **Bounded in time.** The rule's timeout kills a tool that hangs.
//! - **Cancelled when stale.** The returned [`PreviewTask`] aborts the task when
//!   the next preview replaces it, and the child is spawned `kill_on_drop`, so
//!   aborting the task kills the process. The generation guard in
//!   `workers::merge` would drop the late result anyway; this stops the work.
//!
//! The string form's limit: what is killed is the shell, and a program the
//! shell started stops at its next write to the now-closed pipe rather than at
//! once.

use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config::preview_tool::{ENV_HEIGHT, ENV_PATH, ENV_WIDTH};
use crate::preview::external::ExternalSpec;
use crate::preview::provider::{PreviewContent, PreviewTask};
use crate::workers::WorkerMsg;

/// `CREATE_NO_WINDOW`: start a console program without giving it a console
/// window. Without it, each preview of a console tool flashes one up.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How much of stderr is kept for the failure message. The rest is read and
/// discarded, so a chatty tool cannot block on a full pipe.
const STDERR_KEEP_BYTES: usize = 4096;

/// How many lines of stderr the failure message shows under the exit status.
const STDERR_SHOWN_LINES: usize = 5;

/// How long to wait for stderr to finish after the tool has exited non-zero.
///
/// A grandchild started by a shell can hold the pipe open after the shell
/// exits; the message is worth a short wait, not an indefinite one.
const STDERR_GRACE: std::time::Duration = std::time::Duration::from_millis(200);

/// External previewer processes Trail currently holds. Read by the
/// performance tests to prove that a stale preview's process is gone.
static LIVE_CHILDREN: AtomicUsize = AtomicUsize::new(0);

/// How many previewer processes are running right now — spawned, and not yet
/// exited, killed or abandoned to a kill.
// clippy: dead_code — read by the tests and `tests/perf_external_preview.rs`,
// never by the binary.
#[allow(dead_code)]
pub fn live_children() -> usize {
    LIVE_CHILDREN.load(Ordering::SeqCst)
}

/// Counts one live child for as long as it is held.
struct LiveChild;

impl LiveChild {
    fn new() -> Self {
        LIVE_CHILDREN.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for LiveChild {
    fn drop(&mut self) {
        LIVE_CHILDREN.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Aborts a helper task when the worker that owns it goes away, so an aborted
/// preview does not leave its stderr reader behind.
struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// One request for an external preview.
#[derive(Debug)]
pub struct ExternalJob {
    /// The file being previewed.
    pub path: PathBuf,
    /// What to run, already resolved.
    pub spec: ExternalSpec,
    /// The preview generation this answers, for the merge guard.
    pub generation: u64,
    /// Most lines of output kept, from `[preview] max_lines`.
    pub max_lines: usize,
    /// Most bytes of output kept, from `[general] text_sync_threshold_kb`.
    pub max_bytes: usize,
    /// The listing's metadata for the file, shown under a failure.
    pub metadata: Option<std::fs::Metadata>,
}

/// Runs `job` off the UI thread and sends the result to `tx`.
///
/// Returns the task, which kills the program when dropped. Must be called
/// from within the tokio runtime.
pub fn spawn_external_preview(job: ExternalJob, tx: mpsc::Sender<WorkerMsg>) -> PreviewTask {
    let handle = tokio::spawn(async move {
        let tool = job.spec.tool.clone();
        let (content, truncated) = match run(&job).await {
            Ok(captured) => (
                PreviewContent::External {
                    lines: captured.lines,
                    tool,
                },
                captured.truncated,
            ),
            Err(error) => (
                PreviewContent::ExternalFailed {
                    error: error.lines(&tool),
                    metadata: crate::preview::binary::file_metadata_lines(
                        &job.path,
                        job.metadata.as_ref(),
                    ),
                    tool,
                },
                false,
            ),
        };
        let msg = WorkerMsg::Preview {
            generation: job.generation,
            path: job.path,
            content,
            truncated,
            // What a tool printed says nothing about whether the file is text.
            is_text: None,
        };
        // If the channel is closed the UI thread has exited; ignore the error.
        let _ = tx.send(msg).await;
    });
    PreviewTask::new(handle.abort_handle())
}

/// A successful run's output.
#[derive(Debug)]
struct Captured {
    lines: Vec<crate::preview::provider::HighlightedLine>,
    /// Whether the output continued past a cap.
    truncated: bool,
}

/// Why a run produced no preview.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// The program does not exist; `on_path` says whether it was looked up
    /// on `PATH` (a bare name) or named by path.
    NotFound {
        on_path: bool,
    },
    /// Spawning failed for another reason.
    Start(String),
    TimedOut {
        ms: u128,
    },
    /// The program exited unsuccessfully; `stderr` is the start of what it
    /// said there.
    Exited {
        status: String,
        stderr: String,
    },
    /// The program exited successfully and printed nothing on stdout.
    NoOutput,
}

impl Failure {
    /// The error lines the pane shows, the first naming the tool.
    fn lines(&self, tool: &str) -> Vec<String> {
        match self {
            Failure::NotFound { on_path: true } => vec![format!("{tool}: not found on PATH")],
            Failure::NotFound { on_path: false } => vec![format!("{tool}: not found")],
            Failure::Start(e) => vec![format!("{tool}: could not start: {e}")],
            Failure::TimedOut { ms } => vec![format!("{tool}: timed out after {ms} ms")],
            Failure::Exited { status, stderr } => {
                let mut lines = vec![format!("{tool}: exited with {status}")];
                lines.extend(
                    stderr
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .take(STDERR_SHOWN_LINES)
                        .map(|l| format!("  {}", crate::preview::provider::sanitize(l))),
                );
                lines
            }
            Failure::NoOutput => vec![format!("{tool}: produced no output")],
        }
    }
}

/// How a run ended, before it is judged.
enum Ended {
    /// Output hit a cap; the child is still running and must be killed.
    Capped(Vec<u8>),
    /// stdout closed and the child exited.
    Exited(Vec<u8>, std::io::Result<ExitStatus>),
}

/// Spawns the program, reads its output within the caps and the timeout, and
/// judges the result.
async fn run(job: &ExternalJob) -> Result<Captured, Failure> {
    let spec = &job.spec;
    let Some((program, args)) = spec.argv.split_first() else {
        return Err(Failure::Start("empty command".to_owned()));
    };

    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .env(ENV_PATH, &job.path)
        .env(ENV_WIDTH, spec.width.to_string())
        .env(ENV_HEIGHT, spec.height.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = job.path.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let named_by_path = std::path::Path::new(program).components().count() > 1;
            return Err(Failure::NotFound {
                on_path: !named_by_path,
            });
        }
        Err(e) => return Err(Failure::Start(e.to_string())),
    };
    let _live = LiveChild::new();

    let stdout = child.stdout.take();
    let stderr_task = child
        .stderr
        .take()
        .map(|stderr| AbortOnDrop(tokio::spawn(read_head_then_drain(stderr))));

    let ended = tokio::time::timeout(spec.timeout, async {
        let (bytes, capped) = match stdout {
            Some(out) => read_capped(out, job.max_lines, job.max_bytes).await,
            None => (Vec::new(), false),
        };
        if capped {
            Ended::Capped(bytes)
        } else {
            Ended::Exited(bytes, child.wait().await)
        }
    })
    .await;

    match ended {
        Err(_) => {
            let _ = child.start_kill();
            Err(Failure::TimedOut {
                ms: spec.timeout.as_millis(),
            })
        }
        Ok(Ended::Capped(bytes)) => {
            // Everything past the cap is unwanted; stop the tool producing it.
            let _ = child.start_kill();
            Ok(captured(&bytes, job.max_lines, true))
        }
        Ok(Ended::Exited(bytes, status)) => {
            let status = status.map_err(|e| Failure::Start(e.to_string()))?;
            if !status.success() {
                let stderr = match stderr_task {
                    Some(mut task) => tokio::time::timeout(STDERR_GRACE, &mut task.0)
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .unwrap_or_default(),
                    None => Vec::new(),
                };
                return Err(Failure::Exited {
                    status: describe(status),
                    stderr: String::from_utf8_lossy(&stderr).into_owned(),
                });
            }
            let captured = captured(&bytes, job.max_lines, false);
            if captured
                .lines
                .iter()
                .all(|l| l.iter().all(|s| s.text.trim().is_empty()))
            {
                return Err(Failure::NoOutput);
            }
            Ok(captured)
        }
    }
}

/// `status 3`, or the signal on Unix — the part of an `ExitStatus` a person
/// needs.
fn describe(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("status {code}"),
        None => status.to_string(),
    }
}

/// Parses the bytes read into styled lines.
fn captured(bytes: &[u8], max_lines: usize, truncated: bool) -> Captured {
    let mut lines = crate::preview::ansi::parse(&String::from_utf8_lossy(bytes));
    lines.truncate(max_lines);
    Captured { lines, truncated }
}

/// Reads `out` until it closes or a cap is reached, returning what was kept
/// and whether there was more.
///
/// "More" means at least one byte arrived past the cap — output that ends
/// exactly on it is not truncated. A read error ends the read like EOF does;
/// the exit status will say whether the tool failed.
async fn read_capped<R: AsyncRead + Unpin>(
    mut out: R,
    max_lines: usize,
    max_bytes: usize,
) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut lines = 0usize;
    let mut chunk = [0u8; 8192];
    loop {
        let n = match out.read(&mut chunk).await {
            Ok(0) | Err(_) => return (kept, false),
            Ok(n) => n,
        };
        for &byte in &chunk[..n] {
            if lines >= max_lines || kept.len() >= max_bytes {
                return (kept, true);
            }
            kept.push(byte);
            if byte == b'\n' {
                lines += 1;
            }
        }
    }
}

/// Keeps the first [`STDERR_KEEP_BYTES`] of `err` and reads the rest to EOF,
/// so a tool writing a lot of stderr never blocks on a full pipe.
async fn read_head_then_drain<R: AsyncRead + Unpin>(mut err: R) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match err.read(&mut chunk).await {
            Ok(0) | Err(_) => return kept,
            Ok(n) => {
                let room = STDERR_KEEP_BYTES.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::time::{Duration, Instant};

    /// Every test here spawns processes and some read [`live_children`], a
    /// process-wide count; running them one at a time keeps that count
    /// meaningful.
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Runs `script` in the platform's default shell — cmd on Windows, sh
    /// elsewhere — so the same test drives a real process on every CI target.
    fn shell(windows: &str, unix: &str) -> Vec<OsString> {
        let script = if cfg!(windows) { windows } else { unix };
        crate::actions::shell_exec::shell_argv("", script)
            .into_iter()
            .map(OsString::from)
            .collect()
    }

    /// A program that runs for half a minute, spawned directly rather than
    /// through a shell — killing a shell would leave its child running, which
    /// is the string form's documented limit and not what these tests measure.
    fn sleeper() -> Vec<OsString> {
        if cfg!(windows) {
            ["ping", "-n", "30", "127.0.0.1"]
                .map(OsString::from)
                .to_vec()
        } else {
            ["sleep", "30"].map(OsString::from).to_vec()
        }
    }

    fn job(argv: Vec<OsString>, timeout_ms: u64) -> ExternalJob {
        let file = std::env::temp_dir().join("trail-external-preview-test.txt");
        ExternalJob {
            path: file,
            spec: ExternalSpec {
                argv,
                tool: "probe".to_owned(),
                width: 80,
                height: 24,
                timeout: Duration::from_millis(timeout_ms),
            },
            generation: 7,
            max_lines: 50,
            max_bytes: 64 * 1024,
            metadata: None,
        }
    }

    fn text(captured: &Captured) -> Vec<String> {
        captured
            .lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    #[tokio::test]
    async fn normal_output_comes_back_as_lines() {
        let _serial = SERIAL.lock().await;
        let out = run(&job(
            shell("echo one& echo two", "echo one; echo two"),
            5000,
        ))
        .await
        .expect("echo succeeds");
        assert_eq!(text(&out), ["one", "two"]);
        assert!(!out.truncated);
    }

    #[tokio::test]
    async fn the_path_and_pane_size_are_in_the_environment() {
        let _serial = SERIAL.lock().await;
        let out = run(&job(
            shell(
                "echo %TRAIL_PREVIEW_PATH%#%TRAIL_PREVIEW_WIDTH%x%TRAIL_PREVIEW_HEIGHT%",
                "echo \"$TRAIL_PREVIEW_PATH#${TRAIL_PREVIEW_WIDTH}x$TRAIL_PREVIEW_HEIGHT\"",
            ),
            5000,
        ))
        .await
        .unwrap();
        let line = &text(&out)[0];
        assert!(
            line.ends_with("trail-external-preview-test.txt#80x24"),
            "{line}"
        );
    }

    #[tokio::test]
    async fn a_missing_program_says_so() {
        let _serial = SERIAL.lock().await;
        let err = run(&job(vec!["trail-no-such-previewer-xyz".into()], 5000))
            .await
            .unwrap_err();
        assert_eq!(err, Failure::NotFound { on_path: true });
        assert_eq!(err.lines("pdftotext"), ["pdftotext: not found on PATH"]);
    }

    #[tokio::test]
    async fn a_hung_program_times_out_and_is_killed() {
        let _serial = SERIAL.lock().await;
        let before = live_children();
        let started = Instant::now();
        let err = run(&job(sleeper(), 300)).await.unwrap_err();
        assert_eq!(err, Failure::TimedOut { ms: 300 });
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the timeout bit"
        );
        assert_eq!(
            live_children(),
            before,
            "the child is not held after a timeout"
        );
    }

    #[tokio::test]
    async fn output_past_the_line_cap_is_cut_and_reported() {
        let _serial = SERIAL.lock().await;
        let out = run(&job(
            shell(
                "for /L %i in (1,1,5000) do @echo line %i",
                "i=1; while [ $i -le 5000 ]; do echo line $i; i=$((i+1)); done",
            ),
            20_000,
        ))
        .await
        .unwrap();
        assert_eq!(out.lines.len(), 50, "max_lines");
        assert!(out.truncated);
        assert_eq!(text(&out)[49], "line 50");
    }

    #[tokio::test]
    async fn a_failing_program_reports_its_status_and_stderr() {
        let _serial = SERIAL.lock().await;
        let err = run(&job(
            shell("echo boom 1>&2& exit /b 3", "echo boom >&2; exit 3"),
            5000,
        ))
        .await
        .unwrap_err();
        let lines = err.lines("probe");
        assert_eq!(lines[0], "probe: exited with status 3");
        assert!(lines[1].contains("boom"), "{lines:?}");
    }

    #[tokio::test]
    async fn silence_is_a_failure_not_an_empty_preview() {
        let _serial = SERIAL.lock().await;
        let err = run(&job(shell("rem", "true"), 5000)).await.unwrap_err();
        assert_eq!(err, Failure::NoOutput);
    }

    #[tokio::test]
    async fn colour_survives_and_other_escapes_do_not() {
        let _serial = SERIAL.lock().await;
        // Built in the test rather than echoed: cmd and sh disagree on how to
        // print an ESC, but `read_capped` and `captured` are what is under test.
        let bytes = b"\x1b[2J\x1b[31mred\x1b[0m\n";
        let (kept, capped) = read_capped(&bytes[..], 10, 1024).await;
        assert!(!capped);
        let out = captured(&kept, 10, false);
        assert_eq!(text(&out), ["red"]);
        assert_eq!(out.lines[0][0].fg, Some(ratatui::style::Color::Red));
    }

    #[tokio::test]
    async fn the_byte_cap_reads_no_further_than_one_byte_past() {
        let data = vec![b'x'; 10_000];
        let (kept, capped) = read_capped(&data[..], 1000, 4096).await;
        assert_eq!(kept.len(), 4096);
        assert!(capped);

        let exact = vec![b'x'; 4096];
        let (_, capped) = read_capped(&exact[..], 1000, 4096).await;
        assert!(!capped, "output ending exactly on the cap is all of it");
    }

    /// Dropping the task — what the next preview does — kills the program.
    #[tokio::test]
    async fn aborting_the_task_kills_the_program() {
        let _serial = SERIAL.lock().await;
        let before = live_children();
        let (tx, mut rx) = mpsc::channel(4);
        let task = spawn_external_preview(job(sleeper(), 60_000), tx);

        let deadline = Instant::now() + Duration::from_secs(5);
        while live_children() == before && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(live_children(), before + 1, "the program started");

        drop(task);
        let deadline = Instant::now() + Duration::from_secs(5);
        while live_children() > before && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(live_children(), before, "the program is gone");
        assert!(rx.try_recv().is_err(), "an aborted preview sends nothing");
    }

    #[tokio::test]
    async fn a_failure_carries_the_metadata_and_the_generation() {
        let _serial = SERIAL.lock().await;
        let (tx, mut rx) = mpsc::channel(4);
        let _task =
            spawn_external_preview(job(vec!["trail-no-such-previewer-xyz".into()], 5000), tx);
        let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let WorkerMsg::Preview {
            generation,
            content,
            ..
        } = msg
        else {
            panic!("expected a preview");
        };
        assert_eq!(generation, 7);
        match content {
            PreviewContent::ExternalFailed {
                tool,
                error,
                metadata,
            } => {
                assert_eq!(tool, "probe");
                assert_eq!(error, ["probe: not found on PATH"]);
                assert!(!metadata.is_empty());
            }
            other => panic!("expected ExternalFailed, got {other:?}"),
        }
    }
}
