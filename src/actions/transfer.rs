//! Copies and moves that run off the UI thread.
//!
//! A copy takes as long as the data does: one file picked by hand is quick, a
//! folder of several gigabytes is not, and run on the UI thread it froze Trail
//! for the duration (invariant 1). Every `:cp`, `:mv` and paste is therefore a
//! [`TransferJob`] handed to the blocking pool, which reports progress and a
//! [`TransferReport`] back over the worker channel.
//!
//! The job's destinations are resolved before it starts, on the UI thread —
//! that is only a `stat` — but the "is the name already taken" check happens in
//! the worker, immediately before each entry, because the answer can change
//! while earlier entries are being copied.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use crate::actions::fs_ops;
use crate::app::state::AppState;
use crate::workers::WorkerMsg;

/// Whether a transfer leaves the source behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    /// The source stays; a copy appears at the destination.
    Copy,
    /// The source goes; it appears at the destination instead.
    Move,
}

impl TransferKind {
    /// The past tense used in reports: `copied`, `moved`.
    fn past(self) -> &'static str {
        match self {
            TransferKind::Copy => "copied",
            TransferKind::Move => "moved",
        }
    }

    /// The progressive used while it runs: `copying`, `moving`.
    pub fn progressive(self) -> &'static str {
        match self {
            TransferKind::Copy => "copying",
            TransferKind::Move => "moving",
        }
    }
}

/// One entry to transfer, with its destination already resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferItem {
    /// The entry being copied or moved.
    pub source: PathBuf,
    /// Exactly where it should end up — not the directory it goes into.
    pub dest: PathBuf,
}

/// A batch of copies or moves, run in order on the blocking pool.
#[derive(Debug, Clone)]
pub struct TransferJob {
    /// Identifies the job in progress messages and its report.
    pub id: u64,
    /// Copy or move.
    pub kind: TransferKind,
    /// The command that started it (`cp`, `mv`, `paste`), for the report.
    pub label: String,
    /// What to transfer, in order.
    pub items: Vec<TransferItem>,
}

/// What became of a [`TransferJob`].
///
/// Every entry ends up in exactly one of the four lists. Nothing is ever
/// overwritten: an occupied destination is `skipped`, never replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferReport {
    /// The job this answers.
    pub id: u64,
    /// Copy or move.
    pub kind: TransferKind,
    /// The command that started it.
    pub label: String,
    /// Entries transferred, as `(source, dest)`.
    pub done: Vec<(PathBuf, PathBuf)>,
    /// Entries left alone because their destination name was taken.
    pub skipped: Vec<PathBuf>,
    /// Entries that no longer existed when their turn came.
    pub vanished: Vec<PathBuf>,
    /// Entries that failed, with the reason.
    pub failed: Vec<(PathBuf, String)>,
}

impl TransferReport {
    /// One status-bar line: `paste: 2 copied, 1 skipped: exists`.
    ///
    /// A single entry that failed or was skipped is reported by its reason
    /// alone, which is what `:mv` and `:cp` have always said.
    pub fn summary(&self) -> String {
        let label = &self.label;
        let total = self.done.len() + self.skipped.len() + self.vanished.len() + self.failed.len();
        if total == 1 {
            if let Some((source, reason)) = self.failed.first() {
                return format!("{label}: {}: {reason}", display_name(source));
            }
            if let Some(source) = self.skipped.first() {
                return format!(
                    "{label}: {} already exists at the destination",
                    display_name(source)
                );
            }
            if let Some(source) = self.vanished.first() {
                return format!("{label}: {} no longer exists", display_name(source));
            }
        }

        let mut parts = vec![format!("{} {}", self.done.len(), self.kind.past())];
        if !self.skipped.is_empty() {
            parts.push(format!("{} skipped: exists", self.skipped.len()));
        }
        if !self.vanished.is_empty() {
            parts.push(format!("{} gone", self.vanished.len()));
        }
        if let Some((source, reason)) = self.failed.first() {
            parts.push(format!(
                "{} failed ({}: {reason})",
                self.failed.len(),
                display_name(source)
            ));
        }
        format!("{label}: {}", parts.join(", "))
    }

    /// Whether to report this as an error: something failed, or nothing at all
    /// was transferred. A batch with some entries skipped and others done is a
    /// success that mentions the skips.
    pub fn has_failures(&self) -> bool {
        !self.failed.is_empty() || self.done.is_empty()
    }
}

/// The file name of `path`, or the whole path when it has none.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| crate::pathfmt::display(path))
}

/// Runs `job` to completion, calling `progress(done, total)` after each entry.
///
/// Blocking: this does the actual copying. Keeps going after a failure, so one
/// unreadable file does not strand the rest of a paste, and the report says
/// what happened to each.
pub fn run(job: &TransferJob, mut progress: impl FnMut(usize, usize)) -> TransferReport {
    let mut report = TransferReport {
        id: job.id,
        kind: job.kind,
        label: job.label.clone(),
        done: Vec::new(),
        skipped: Vec::new(),
        vanished: Vec::new(),
        failed: Vec::new(),
    };
    let total = job.items.len();

    for (index, item) in job.items.iter().enumerate() {
        // `symlink_metadata` rather than `exists`: a dangling link is still an
        // entry that can be moved, and still a name that is taken.
        if item.source.symlink_metadata().is_err() {
            report.vanished.push(item.source.clone());
        } else if item.dest.symlink_metadata().is_ok() {
            report.skipped.push(item.source.clone());
        } else {
            let result = match job.kind {
                TransferKind::Copy => fs_ops::copy_path(&item.source, &item.dest),
                TransferKind::Move => fs_ops::move_path(&item.source, &item.dest),
            };
            match result {
                Ok(()) => report.done.push((item.source.clone(), item.dest.clone())),
                Err(e) => report.failed.push((item.source.clone(), e.to_string())),
            }
        }
        progress(index + 1, total);
    }
    report
}

/// A transfer that has started and not yet reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTransfer {
    /// The job's id.
    pub id: u64,
    /// Copy or move.
    pub kind: TransferKind,
    /// Entries finished so far.
    pub done: usize,
    /// Entries in the job.
    pub total: usize,
}

/// The transfers in flight, and the channel they report on.
///
/// Without a channel — in tests, which drive [`AppState`] directly — a job
/// runs to completion inside [`start`], so a test sees the outcome as soon as
/// the action returns.
#[derive(Debug, Default)]
pub struct Transfers {
    sender: Option<mpsc::Sender<WorkerMsg>>,
    next_id: u64,
    active: Vec<ActiveTransfer>,
}

impl Transfers {
    /// Connects transfers to the worker channel, so they run in the background.
    pub fn attach(&mut self, sender: mpsc::Sender<WorkerMsg>) {
        self.sender = Some(sender);
    }

    /// The transfers still running, oldest first.
    pub fn active(&self) -> &[ActiveTransfer] {
        &self.active
    }

    /// Records a progress report for job `id`. Returns whether it was known.
    pub fn progress(&mut self, id: u64, done: usize, total: usize) -> bool {
        match self.active.iter_mut().find(|t| t.id == id) {
            Some(transfer) => {
                transfer.done = done;
                transfer.total = total;
                true
            }
            None => false,
        }
    }
}

/// Starts copying or moving `items`, reporting under `label`.
///
/// Runs on the blocking pool when a worker channel is attached and a tokio
/// runtime is current, and inline otherwise. Either way the outcome arrives
/// through [`finish`].
pub fn start(state: &mut AppState, kind: TransferKind, label: &str, items: Vec<TransferItem>) {
    if items.is_empty() {
        return;
    }
    let id = state.transfers.next_id;
    state.transfers.next_id += 1;
    let job = TransferJob {
        id,
        kind,
        label: label.to_owned(),
        items,
    };

    let background = state
        .transfers
        .sender
        .clone()
        .zip(tokio::runtime::Handle::try_current().ok());
    let Some((tx, runtime)) = background else {
        let report = run(&job, |_, _| {});
        finish(state, report);
        return;
    };

    let total = job.items.len();
    state.transfers.active.push(ActiveTransfer {
        id,
        kind,
        done: 0,
        total,
    });
    state.dirty = true;
    runtime.spawn_blocking(move || {
        let report = run(&job, |done, total| {
            // A dropped progress message only means a stale counter for one
            // frame; the report itself is sent with back-pressure below.
            let _ = tx.try_send(WorkerMsg::TransferProgress { id, done, total });
        });
        if tx
            .blocking_send(WorkerMsg::TransferDone { report })
            .is_err()
        {
            tracing::debug!(id, "transfer finished after the UI went away");
        }
    });
}

/// Applies a finished transfer: says what happened and re-lists the directory.
pub fn finish(state: &mut AppState, report: TransferReport) {
    state.transfers.active.retain(|t| t.id != report.id);

    let summary = report.summary();
    if report.has_failures() {
        state.set_error(summary);
    } else {
        state.notify(summary);
    }

    if let Err(e) = state.refresh() {
        tracing::warn!("refresh after transfer failed: {e}");
    }
    state.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn job(kind: TransferKind, items: Vec<TransferItem>) -> TransferJob {
        TransferJob {
            id: 1,
            kind,
            label: "paste".to_owned(),
            items,
        }
    }

    fn item(source: &Path, dest: &Path) -> TransferItem {
        TransferItem {
            source: source.to_owned(),
            dest: dest.to_owned(),
        }
    }

    #[test]
    fn a_taken_name_is_skipped_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"new b").unwrap();
        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();
        fs::write(out.join("a.txt"), b"old").unwrap();

        let report = run(
            &job(
                TransferKind::Copy,
                vec![item(&a, &out.join("a.txt")), item(&b, &out.join("b.txt"))],
            ),
            |_, _| {},
        );

        assert_eq!(report.done.len(), 1);
        assert_eq!(report.skipped, vec![a]);
        assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"old");
        assert_eq!(report.summary(), "paste: 1 copied, 1 skipped: exists");
        assert!(!report.has_failures());
    }

    #[test]
    fn a_vanished_source_is_counted_and_the_rest_still_run() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone.txt");
        let here = dir.path().join("here.txt");
        fs::write(&here, b"x").unwrap();
        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();

        let mut steps = Vec::new();
        let report = run(
            &job(
                TransferKind::Move,
                vec![
                    item(&gone, &out.join("gone.txt")),
                    item(&here, &out.join("here.txt")),
                ],
            ),
            |done, total| steps.push((done, total)),
        );

        assert_eq!(report.vanished, vec![gone]);
        assert!(!here.exists());
        assert!(out.join("here.txt").exists());
        assert_eq!(steps, vec![(1, 2), (2, 2)]);
        assert_eq!(report.summary(), "paste: 1 moved, 1 gone");
    }

    #[test]
    fn a_single_entry_reports_its_reason_alone() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        fs::write(&a, b"x").unwrap();
        let mut report = run(&job(TransferKind::Copy, vec![item(&a, &a)]), |_, _| {});
        report.label = "cp".to_owned();
        assert_eq!(
            report.summary(),
            "cp: a.txt already exists at the destination"
        );
        assert!(report.has_failures(), "nothing happened, so it is an error");
    }

    /// The path the running app takes: the job leaves the UI thread, the status
    /// bar has something to show meanwhile, and the report comes back over the
    /// worker channel.
    #[tokio::test(flavor = "multi_thread")]
    async fn with_a_channel_the_transfer_runs_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        fs::write(&a, b"x").unwrap();
        let mut state = AppState::new(dir.path().to_owned()).unwrap();
        let (tx, mut rx) = crate::workers::channel();
        state.transfers.attach(tx);

        start(
            &mut state,
            TransferKind::Copy,
            "cp",
            vec![item(&a, &dir.path().join("b.txt"))],
        );
        assert_eq!(state.transfers.active().len(), 1, "running, not finished");

        loop {
            let msg = rx.recv().await.expect("the worker reports");
            let done = matches!(msg, WorkerMsg::TransferDone { .. });
            crate::workers::merge(msg, &mut state);
            if done {
                break;
            }
        }
        assert!(state.transfers.active().is_empty());
        assert!(dir.path().join("b.txt").exists());
        assert!(
            state.entries.iter().any(|e| e.file_name == "b.txt"),
            "the listing is refreshed when the report lands"
        );
        assert_eq!(state.error_text(), None);
    }

    #[test]
    fn nothing_done_is_a_failure_even_without_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        let report = run(
            &job(TransferKind::Copy, vec![item(&gone, &dir.path().join("x"))]),
            |_, _| {},
        );
        assert!(report.has_failures());
    }
}
