//! The Tokio runtime Trail runs on, sized for a file manager rather than a server.
//!
//! `#[tokio::main]` starts one async worker thread per CPU core, and lets the
//! blocking pool grow to 512 more. Trail's async tasks do little but hand work to
//! `spawn_blocking` and wait for it, so a few workers serve it as well as dozens.
//! One per core is also enough to stop Trail starting at all where an account may
//! only start a few dozen threads in all: on a 48-core shared-hosting server,
//! the runtime's own workers left no thread for the rest of startup.

use std::io;
use std::num::NonZeroUsize;

use tokio::runtime::{Builder, Runtime};

/// The most async worker threads Trail starts, however many cores the machine has.
pub const MAX_WORKER_THREADS: usize = 4;

/// The most threads the blocking pool grows to (Tokio's default is 512).
///
/// Previews, git status and copies each hold one while they run; past this,
/// further work queues until one frees up.
pub const MAX_BLOCKING_THREADS: usize = 32;

/// How many async worker threads to start on a machine with `cores` cores.
///
/// At least one, at most [`MAX_WORKER_THREADS`]; one when the core count is
/// unknown.
///
/// ```
/// use std::num::NonZeroUsize;
/// use trail::runtime::{worker_threads, MAX_WORKER_THREADS};
///
/// assert_eq!(worker_threads(NonZeroUsize::new(48)), MAX_WORKER_THREADS);
/// assert_eq!(worker_threads(NonZeroUsize::new(2)), 2);
/// assert_eq!(worker_threads(None), 1);
/// ```
pub fn worker_threads(cores: Option<NonZeroUsize>) -> usize {
    cores.map_or(1, NonZeroUsize::get).min(MAX_WORKER_THREADS)
}

/// Builds the runtime `main` runs on.
///
/// # Errors
///
/// Returns the OS error if the runtime cannot be created. A worker thread the
/// OS refuses once others are running does not fail this: Tokio runs with the
/// ones it got.
pub fn build() -> io::Result<Runtime> {
    Builder::new_multi_thread()
        .worker_threads(worker_threads(std::thread::available_parallelism().ok()))
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .thread_name("trail-worker")
        .enable_all()
        .build()
}
