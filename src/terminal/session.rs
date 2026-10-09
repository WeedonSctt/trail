//! One shell in the terminal panel: a pseudo-terminal, the process in it, and
//! the screen its output has drawn.
//!
//! A session owns three threads, none of which touch the UI:
//!
//! - **reader** — reads the shell's output, feeds it to the `vt100` parser, and
//!   tells the UI thread there is something new to draw;
//! - **writer** — writes keystrokes to the shell, so a shell that stops reading
//!   input can never stall the UI thread;
//! - **waiter** — waits for the shell to exit and reports it. On Windows the
//!   reader never sees end-of-file while the pseudo-console is open, so the
//!   reader cannot be the one to notice.
//!
//! They are plain threads rather than tokio tasks because every call they make
//! blocks, which is exactly what a tokio worker must not do.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex, MutexGuard};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc;

use crate::workers::WorkerMsg;

/// Rows of output a session keeps above the visible screen.
///
/// Bounded because it is memory per shell: a row is a few bytes per column, so
/// 2 000 rows of a 200-column panel is a few megabytes, not a few hundred.
pub const SCROLLBACK_ROWS: usize = 2_000;

/// A shell running in the panel.
pub struct Session {
    id: u64,
    label: String,
    pid: Option<u32>,
    parser: Arc<Mutex<vt100::Parser<Replies>>>,
    /// Set by the reader when it has told the UI about new output, cleared by
    /// the UI once that output is on screen. Coalesces a flood of output into
    /// one message per frame instead of one per read.
    output_signalled: Arc<AtomicBool>,
    input: std_mpsc::Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// `Option` only so `Drop` can move it to another thread.
    master: Option<Box<dyn MasterPty + Send>>,
    size: (u16, u16),
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("pid", &self.pid)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Starts `argv` in `cwd` inside a pseudo-terminal of `rows` × `cols`.
    ///
    /// Output and exit are reported on `notify` as [`WorkerMsg::TerminalOutput`]
    /// and [`WorkerMsg::TerminalExited`], tagged with `id`.
    ///
    /// # Errors
    ///
    /// Returns a description of what failed — the pseudo-terminal could not be
    /// created, or the program could not be started (most often: not found).
    pub fn spawn(
        id: u64,
        label: &str,
        argv: &[String],
        cwd: &Path,
        (rows, cols): (u16, u16),
        notify: mpsc::Sender<WorkerMsg>,
    ) -> Result<Self, String> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| "the profile has no command".to_owned())?;
        let rows = rows.max(1);
        let cols = cols.max(1);

        let pair = portable_pty::native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("could not create a pseudo-terminal: {e}"))?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.cwd(cwd);
        // What the screen parser understands. Windows shells ignore it; Git
        // Bash and WSL read it to decide which escape sequences to send.
        cmd.env("TERM", "xterm-256color");

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("could not start '{program}': {e}"))?;
        // The slave end belongs to the child now; holding it would keep the
        // pseudo-terminal alive after the shell exits on Unix.
        drop(pair.slave);

        let pid = child.process_id();
        let killer = child.clone_killer();
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("could not read from the pseudo-terminal: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("could not write to the pseudo-terminal: {e}"))?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            rows,
            cols,
            SCROLLBACK_ROWS,
            Replies::default(),
        )));
        let output_signalled = Arc::new(AtomicBool::new(false));
        let (input, input_rx) = std_mpsc::channel::<Vec<u8>>();

        spawn_writer(writer, input_rx);
        spawn_reader(
            id,
            reader,
            Arc::clone(&parser),
            Arc::clone(&output_signalled),
            input.clone(),
            notify.clone(),
        );
        std::thread::Builder::new()
            .name(format!("trail-term-wait-{id}"))
            .spawn(move || {
                let code = child.wait().ok().map(|status| status.exit_code());
                // The UI may already be gone on quit; nothing to tell then.
                let _ = notify.blocking_send(WorkerMsg::TerminalExited { id, code });
            })
            .map_err(|e| format!("could not start the exit watcher: {e}"))?;

        Ok(Self {
            id,
            label: label.to_owned(),
            pid,
            parser,
            output_signalled,
            input,
            killer,
            master: Some(pair.master),
            size: (rows, cols),
        })
    }

    /// The identifier output and exit messages carry.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The profile name the session was started from.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The shell's process id, when the platform reported one.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Queues `bytes` for the shell. Never blocks.
    pub fn write(&self, bytes: Vec<u8>) {
        // A closed channel means the writer thread is gone because the shell
        // is; the exit message is already on its way.
        let _ = self.input.send(bytes);
    }

    /// Locks the screen for reading or for a scroll change.
    ///
    /// Held only for as long as one frame takes to copy cells out, so the
    /// reader thread is never kept waiting for longer than that.
    pub fn screen(&self) -> MutexGuard<'_, vt100::Parser<Replies>> {
        self.parser.lock().unwrap_or_else(|poisoned| {
            // A reader thread that panicked mid-parse leaves a screen that is
            // at worst partly drawn, which is still worth showing.
            poisoned.into_inner()
        })
    }

    /// Lets the reader announce new output again, once the last batch is on
    /// screen.
    pub fn output_drawn(&self) {
        self.output_signalled.store(false, Ordering::Release);
    }

    /// Resizes the pseudo-terminal and the screen to `rows` × `cols`, if they
    /// changed. Cheap enough to call every frame.
    pub fn resize(&mut self, (rows, cols): (u16, u16)) {
        let size = (rows.max(1), cols.max(1));
        if size == self.size {
            return;
        }
        self.size = size;
        if let Some(master) = &self.master {
            if let Err(e) = master.resize(PtySize {
                rows: size.0,
                cols: size.1,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                tracing::debug!("terminal resize failed: {e}");
            }
        }
        self.screen().screen_mut().set_size(size.0, size.1);
    }

    /// Scrolls the view `delta` rows into the scrollback (positive is back in
    /// time), clamped to what the scrollback holds.
    pub fn scroll(&self, delta: isize) {
        let mut parser = self.screen();
        let screen = parser.screen_mut();
        let target = screen.scrollback().saturating_add_signed(delta);
        screen.set_scrollback(target);
    }

    /// Returns the view to the live screen.
    pub fn scroll_to_bottom(&self) {
        self.screen().screen_mut().set_scrollback(0);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Err(e) = self.killer.kill() {
            // Already exited is the common case.
            tracing::debug!("terminal session {} kill: {e}", self.id);
        }
        // Closing a Windows pseudo-console waits for its output to drain, so it
        // is done off the UI thread. The reader is still draining it.
        if let Some(master) = self.master.take() {
            let spawned = std::thread::Builder::new()
                .name("trail-term-close".to_owned())
                .spawn(move || drop(master));
            if let Err(e) = spawned {
                tracing::debug!("could not close the pseudo-terminal off-thread: {e}");
            }
        }
    }
}

fn spawn_writer(mut writer: Box<dyn Write + Send>, input: std_mpsc::Receiver<Vec<u8>>) {
    let spawned = std::thread::Builder::new()
        .name("trail-term-write".to_owned())
        .spawn(move || {
            for bytes in input {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("could not start the terminal writer: {e}");
    }
}

fn spawn_reader(
    id: u64,
    mut reader: Box<dyn Read + Send>,
    parser: Arc<Mutex<vt100::Parser<Replies>>>,
    signalled: Arc<AtomicBool>,
    input: std_mpsc::Sender<Vec<u8>>,
    notify: mpsc::Sender<WorkerMsg>,
) {
    let spawned = std::thread::Builder::new()
        .name(format!("trail-term-read-{id}"))
        .spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let replies = {
                    let mut parser = parser.lock().unwrap_or_else(|p| p.into_inner());
                    parser.process(&buf[..n]);
                    std::mem::take(&mut parser.callbacks_mut().pending)
                };
                if !replies.is_empty() {
                    let _ = input.send(replies);
                }
                if !signalled.swap(true, Ordering::AcqRel)
                    && notify.try_send(WorkerMsg::TerminalOutput { id }).is_err()
                {
                    // The channel is full, so the UI is busy and will draw
                    // soon anyway; let the next read signal again.
                    signalled.store(false, Ordering::Release);
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("could not start the terminal reader: {e}");
    }
}

/// Answers the questions a program asks its terminal.
///
/// A real terminal replies to "where is the cursor?" and "what are you?";
/// programs that ask (prompt frameworks, `vim`, some installers) wait for the
/// answer, and without one they stall for seconds or draw in the wrong place.
#[derive(Debug, Default)]
pub struct Replies {
    pending: Vec<u8>,
}

impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first = params.first().and_then(|p| p.first()).copied();
        match (c, i1, first) {
            // DSR: cursor position report.
            ('n', None, Some(6)) => {
                let (row, col) = screen.cursor_position();
                self.pending
                    .extend(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes());
            }
            // DSR: "are you working?" — yes.
            ('n', None, Some(5)) => self.pending.extend(b"\x1b[0n"),
            // DA1: identify as a VT220-class terminal, which is what the
            // parser implements.
            ('c', None, None | Some(0)) => self.pending.extend(b"\x1b[?62;22c"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parser_answers_cursor_and_device_queries() {
        let mut parser = vt100::Parser::new_with_callbacks(5, 20, 0, Replies::default());
        parser.process(b"ab\x1b[6n");
        assert_eq!(parser.callbacks().pending, b"\x1b[1;3R");

        parser.callbacks_mut().pending.clear();
        parser.process(b"\x1b[c\x1b[5n");
        assert_eq!(parser.callbacks().pending, b"\x1b[?62;22c\x1b[0n");
    }

    /// The whole lifecycle against a real shell: start, type, read the output
    /// back from the screen, and see the exit reported.
    #[test]
    fn a_real_shell_runs_a_command_and_reports_its_exit() {
        let (tx, mut rx) = mpsc::channel(64);
        let dir = tempfile::tempdir().unwrap();
        let argv: Vec<String> = if cfg!(windows) {
            vec!["cmd.exe".into(), "/Q".into(), "/K".into()]
        } else {
            vec!["/bin/sh".into()]
        };
        let session = Session::spawn(1, "test", &argv, dir.path(), (10, 60), tx).unwrap();
        session.write(b"echo trail-marker-42\r".to_vec());

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            assert!(std::time::Instant::now() < deadline, "no output seen");
            if session
                .screen()
                .screen()
                .contents()
                .contains("trail-marker-42\n")
                || session
                    .screen()
                    .screen()
                    .contents()
                    .matches("trail-marker-42")
                    .count()
                    >= 2
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        session.write(b"exit\r".to_vec());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            assert!(std::time::Instant::now() < deadline, "no exit seen");
            match rx.try_recv() {
                Ok(WorkerMsg::TerminalExited { id, .. }) => {
                    assert_eq!(id, 1);
                    break;
                }
                Ok(_) | Err(mpsc::error::TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => panic!("channel closed: {e}"),
            }
        }
    }
}
