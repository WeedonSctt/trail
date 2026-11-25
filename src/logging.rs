//! The log file writer: Trail's `tracing` output, written by a thread of its own.
//!
//! `tracing_appender::non_blocking`, which this replaces, panics when the OS
//! refuses its thread, and so took Trail down on a host that limits threads.
//! A log file is optional — Trail runs the same without one — so here a refused
//! thread is an error the caller can ignore.
//!
//! Logging never blocks the thread that logs: lines go to the writer thread over
//! a bounded queue, and a line that finds the queue full is dropped.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc::{self, SyncSender};
use std::thread;

use tracing_subscriber::fmt::MakeWriter;

/// Lines queued for the writer thread before new ones are dropped.
pub const QUEUE_LINES: usize = 8192;

/// Where log lines go: a handle to the writer thread, given to
/// `tracing_subscriber::fmt().with_writer`.
#[derive(Clone, Debug)]
pub struct LogSink {
    tx: SyncSender<Vec<u8>>,
}

impl LogSink {
    /// Opens `path` for appending, creating it if needed, and starts the thread
    /// that writes to it.
    ///
    /// The thread lives for the rest of the process; it stops early only if a
    /// write to the file fails, after which lines are dropped.
    ///
    /// # Errors
    ///
    /// Returns the error if the file cannot be opened or the OS refuses the thread.
    pub fn spawn(path: &Path) -> io::Result<Self> {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(QUEUE_LINES);
        thread::Builder::new()
            .name("trail-log".into())
            .spawn(move || {
                for line in rx {
                    if file.write_all(&line).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self { tx })
    }
}

/// The writer for one log event; queues what it is given and returns at once.
#[derive(Debug)]
pub struct LogWriter {
    tx: SyncSender<Vec<u8>>,
}

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // A full queue or a stopped writer drops the line rather than wait.
        let _ = self.tx.try_send(buf.to_vec());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogSink {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter {
            tx: self.tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn lines_reach_the_file_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trail.log");
        let sink = LogSink::spawn(&path).unwrap();
        sink.make_writer().write_all(b"first\n").unwrap();
        sink.make_writer().write_all(b"second\n").unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut text = String::new();
        while Instant::now() < deadline {
            text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains("second") {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(text, "first\nsecond\n");
    }

    #[test]
    fn appends_to_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trail.log");
        std::fs::write(&path, "earlier\n").unwrap();
        let sink = LogSink::spawn(&path).unwrap();
        sink.make_writer().write_all(b"later\n").unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if std::fs::read_to_string(&path).unwrap().contains("later") {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "earlier\nlater\n");
    }

    #[test]
    fn a_full_queue_drops_the_line_instead_of_blocking() {
        // Nothing reads `_rx`, so the one slot fills and stays full.
        let (tx, _rx) = mpsc::sync_channel(1);
        let mut writer = LogWriter { tx };
        assert_eq!(writer.write(b"kept").unwrap(), 4);
        assert_eq!(writer.write(b"dropped").unwrap(), 7);
    }

    #[test]
    fn a_stopped_writer_does_not_fail_the_caller() {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        let mut writer = LogWriter { tx };
        assert_eq!(writer.write(b"line").unwrap(), 4);
    }
}
