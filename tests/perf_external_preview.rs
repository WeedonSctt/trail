//! Performance budgets for external previewers — the release gate for them.
//!
//! Every test is `#[ignore]`d: timing assertions are flaky on shared CI
//! runners, so these gate the release locally instead. Run them in release
//! mode, where the budgets are set:
//!
//! ```text
//! cargo test --release --test perf_external_preview -- --ignored --test-threads=1
//! ```
//!
//! | # | Measures | Budget |
//! |---|---|---|
//! | B1 | UI-thread cost of previewing an entry with an external rule | < 1 ms median |
//! | B2 | Config load with 20 rules vs none | < 1 ms difference, no process spawned |
//! | B3 | 200 selection changes 10 ms apart, tool sleeps 5 s | <= 1 live child 500 ms after the last |
//! | B4 | A tool printing 10 MB | stops at the byte cap, result within 200 ms of it |
//! | B5 | ANSI parse of 2000 lines (~200 KB) of `bat`-style output | < 20 ms median |
//! | B6 | One frame of a 2000-line external preview | < 2 ms median |

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use trail::app::state::AppState;
use trail::preview::external::ExternalSpec;
use trail::preview::provider::{PreviewContent, PreviewCtx, PreviewRegistry, StyledSpan};
use trail::workers::external_preview::{live_children, spawn_external_preview, ExternalJob};
use trail::workers::WorkerMsg;

/// The median of `samples`, which it sorts.
fn median(samples: &mut [Duration]) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// Times `f` `runs` times and returns the median.
fn median_of(runs: usize, mut f: impl FnMut()) -> Duration {
    let mut samples: Vec<Duration> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    median(&mut samples)
}

fn write_config(dir: &Path, rules: usize) -> PathBuf {
    let mut toml = String::new();
    for i in 0..rules {
        toml.push_str(&format!(
            "[[preview.tool]]\nextensions = [\"ext{i}\", \"alt{i}\"]\n\
             command = [\"tool{i}\", \"--width={{width}}\", \"{{path}}\", \"-\"]\n\n"
        ));
    }
    let path = dir.join(format!("trail-{rules}.toml"));
    std::fs::write(&path, toml).unwrap();
    path
}

/// A previewer job running `argv` for a file in the temp directory.
fn job(argv: Vec<OsString>, timeout: Duration, max_bytes: usize) -> ExternalJob {
    ExternalJob {
        path: std::env::temp_dir().join("trail-perf.txt"),
        spec: ExternalSpec {
            argv,
            tool: "perf".to_owned(),
            width: 80,
            height: 24,
            timeout,
        },
        generation: 1,
        max_lines: 100_000,
        max_bytes,
        metadata: None,
    }
}

/// Runs for about five seconds, spawned directly so killing it kills it.
fn sleeper() -> Vec<OsString> {
    if cfg!(windows) {
        ["ping", "-n", "6", "127.0.0.1"]
            .map(OsString::from)
            .to_vec()
    } else {
        ["sleep", "5"].map(OsString::from).to_vec()
    }
}

/// Prints `file` to stdout with the platform's own tool.
fn printer(file: &Path) -> Vec<OsString> {
    if cfg!(windows) {
        vec!["cmd".into(), "/C".into(), "type".into(), file.into()]
    } else {
        vec!["cat".into(), file.into()]
    }
}

/// B1: everything the UI thread does to start an external preview — resolve
/// the rule, ask the registry, have the provider spawn its task — is cheap,
/// because it only spawns.
#[test]
#[ignore = "performance budget; run in release with --ignored"]
fn b1_starting_an_external_preview_costs_the_ui_thread_under_a_millisecond() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("doc.ext3"), b"x").unwrap();
    let config = trail::config::load(Some(&write_config(dir.path(), 20))).unwrap();
    let state = AppState::with_config(dir.path().to_owned(), config).unwrap();
    let entry = state.selected_entry().cloned().unwrap();
    let mut registry = PreviewRegistry::new();
    trail::preview::register_defaults(&mut registry);

    // A current-thread runtime that is never driven: the spawned tasks are
    // aborted before they run, so no process starts and only the UI thread's
    // share is measured.
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let _guard = rt.enter();
    let (tx, _rx) = tokio::sync::mpsc::channel(4);
    let overrides = HashMap::new();

    let cost = median_of(1000, || {
        let ctx = PreviewCtx {
            show_hidden: false,
            worker_tx: tx.clone(),
            generation: 1,
            text_sync_threshold_bytes: 256 * 1024,
            max_preview_lines: 2000,
            external: trail::preview::external::resolve(
                &entry.path,
                &state.config,
                &overrides,
                (80, 24),
            ),
        };
        let outcome = registry.preview_for(&entry, &ctx);
        assert!(matches!(
            outcome,
            trail::preview::provider::PreviewOutcome::Spawned(_)
        ));
    });
    println!("B1 median: {cost:?}");
    assert!(cost < Duration::from_millis(1), "B1: {cost:?}");
}

/// B2: rules are parsed and validated, never probed — no `PATH` search, no
/// process — so twenty of them cost nothing noticeable at startup.
#[test]
#[ignore = "performance budget; run in release with --ignored"]
fn b2_twenty_rules_add_under_a_millisecond_to_config_load() {
    let dir = tempfile::tempdir().unwrap();
    let none = write_config(dir.path(), 0);
    let twenty = write_config(dir.path(), 20);
    let before = live_children();

    let base = median_of(50, || {
        trail::config::load(Some(&none)).unwrap();
    });
    let with_rules = median_of(50, || {
        assert_eq!(
            trail::config::load(Some(&twenty))
                .unwrap()
                .preview
                .tool
                .len(),
            20
        );
    });
    let extra = with_rules.saturating_sub(base);
    println!("B2 none {base:?}, twenty {with_rules:?}, extra {extra:?}");
    assert!(extra < Duration::from_millis(1), "B2: {extra:?}");
    assert_eq!(live_children(), before, "B2: loading spawned a process");
}

/// B3: holding `j` through a folder of slow previews leaves at most one tool
/// running, because each new preview kills the last.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "performance budget; run in release with --ignored"]
async fn b3_rapid_selection_changes_leave_at_most_one_tool_running() {
    let before = live_children();
    let processes_before = os_sleeper_count();
    let (tx, _rx) = tokio::sync::mpsc::channel(256);

    let mut task = None;
    for _ in 0..200 {
        // What `refresh_preview` does: drop the old task, start the next.
        drop(task.take());
        task = Some(spawn_external_preview(
            job(sleeper(), Duration::from_secs(30), 64 * 1024),
            tx.clone(),
        ));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    let live = live_children() - before;
    let processes = os_sleeper_count().map(|n| n - processes_before.unwrap_or(0));
    println!("B3 live children {live}, OS sleeper processes {processes:?}");
    assert!(live <= 1, "B3: {live} children held");
    if let Some(n) = processes {
        assert!(n <= 1, "B3: {n} sleeper processes still running");
    }
    drop(task);
}

/// On Windows, how many `PING.EXE` processes exist — the operating system's
/// count, not Trail's, so a kill that only dropped a handle would show up.
/// `None` elsewhere, where the live-child count stands alone.
fn os_sleeper_count() -> Option<usize> {
    if !cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq PING.EXE", "/NH"])
        .output()
        .ok()?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.to_ascii_uppercase().starts_with("PING.EXE"))
            .count(),
    )
}

/// B4: a tool that prints far more than the cap costs the cap, not its
/// output: the read stops, the tool is killed, and the result arrives promptly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "performance budget; run in release with --ignored"]
async fn b4_ten_megabytes_of_output_stop_at_the_byte_cap() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.txt");
    let line = "0123456789".repeat(10) + "\n";
    std::fs::write(&big, line.repeat(10 * 1024 * 1024 / line.len())).unwrap();
    let small = dir.path().join("small.txt");
    std::fs::write(&small, "x\n").unwrap();
    let cap = 256 * 1024;

    // What starting the tool costs on this machine, so the budget measures
    // the cap and the kill rather than process creation.
    let mut baseline = Vec::new();
    for _ in 0..5 {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let start = Instant::now();
        let _task = spawn_external_preview(job(printer(&small), Duration::from_secs(10), cap), tx);
        rx.recv().await.unwrap();
        baseline.push(start.elapsed());
    }
    let baseline = median(&mut baseline);

    let before = live_children();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let start = Instant::now();
    let _task = spawn_external_preview(job(printer(&big), Duration::from_secs(10), cap), tx);
    let msg = rx.recv().await.unwrap();
    let elapsed = start.elapsed();

    let WorkerMsg::Preview {
        content, truncated, ..
    } = msg
    else {
        panic!("expected a preview");
    };
    let PreviewContent::External { lines, .. } = content else {
        panic!("expected output, got {content:?}");
    };
    let kept: usize = lines
        .iter()
        .map(|l| l.iter().map(|s| s.text.len()).sum::<usize>() + 1)
        .sum();
    let over = elapsed.saturating_sub(baseline);
    println!("B4 elapsed {elapsed:?}, start-up {baseline:?}, over {over:?}, kept {kept} B");
    assert!(truncated, "B4: the cap must be reported");
    assert!(
        kept <= cap + 1,
        "B4: kept {kept} bytes past a {cap}-byte cap"
    );
    assert!(over < Duration::from_millis(200), "B4: {over:?}");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(live_children(), before, "B4: the tool was not killed");
}

/// `bat --color=always`-style output: 24-bit foregrounds around every token,
/// a reset at the end of each line.
fn bat_like(lines: usize) -> String {
    let mut out = String::new();
    for i in 0..lines {
        let (r, g, b) = (i % 256, (i * 7) % 256, (i * 13) % 256);
        out.push_str(&format!(
            "\x1b[38;2;{r};{g};{b}mfn\x1b[0m \x1b[38;2;143;161;179mfunction_{i}\x1b[0m\
             (\x1b[38;2;191;97;106margument\x1b[0m: \x1b[1;38;2;235;203;139mu64\x1b[0m) -> \
             \x1b[38;2;163;190;140m\"a string literal\"\x1b[0m;\n"
        ));
    }
    out
}

/// B5: parsing a long coloured output is a small fraction of a frame.
#[test]
#[ignore = "performance budget; run in release with --ignored"]
fn b5_parsing_two_thousand_coloured_lines_takes_under_20_ms() {
    let text = bat_like(2000);
    println!("B5 input {} KB", text.len() / 1024);
    assert!(text.len() > 150 * 1024, "the input is representative");

    let cost = median_of(20, || {
        assert_eq!(trail::preview::ansi::parse(&text).len(), 2000);
    });
    println!("B5 median: {cost:?}");
    assert!(cost < Duration::from_millis(20), "B5: {cost:?}");
}

/// B6: a frame draws only the visible slice of a long external preview.
#[test]
#[ignore = "performance budget; run in release with --ignored"]
fn b6_one_frame_of_a_long_external_preview_takes_under_2_ms() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("doc.pdf"), b"x").unwrap();
    let mut state = AppState::new(dir.path().to_owned()).unwrap();
    let lines: Vec<Vec<StyledSpan>> = trail::preview::ansi::parse(&bat_like(2000));
    state.preview.content = PreviewContent::External {
        lines,
        tool: "bat".to_owned(),
    };
    state.preview.scroll = 1000;
    let mut terminal = Terminal::new(TestBackend::new(160, 50)).unwrap();

    let cost = median_of(50, || {
        trail::ui::render(&mut terminal, &mut state).unwrap();
    });
    println!("B6 median: {cost:?}");
    assert!(cost < Duration::from_millis(2), "B6: {cost:?}");
}
