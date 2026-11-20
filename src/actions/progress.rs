//! How far a copy or move has got: the totals measured before it starts, the
//! running counts, and the status-bar line that shows them.
//!
//! A job is counted in three units. Entries are what the user picked, and a
//! folder is one entry however much it holds. Files are what is inside them.
//! Bytes are what takes the time, so the percentage and the bar go by bytes.
//! Everything here is pure, except [`measure`] and [`same_volume`], which read
//! the filesystem and run in the transfer worker.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::actions::transfer::TransferKind;

/// How a job is getting on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    /// Still measuring the sources, so nothing else is known yet.
    pub counting: bool,
    /// Entries finished.
    pub entries_done: usize,
    /// Entries in the job.
    pub entries_total: usize,
    /// Files copied.
    pub files_done: u64,
    /// Files to copy. Zero for a job that only renames.
    pub files_total: u64,
    /// Bytes written.
    pub bytes_done: u64,
    /// Bytes to write. Zero for a job that only renames, which then shows the
    /// entry count instead of a bar.
    pub bytes_total: u64,
}

impl Progress {
    /// Takes an entry's totals back out, when it was skipped or failed and its
    /// data is no longer going to be written.
    pub fn forget(&mut self, totals: Totals) {
        self.files_total = self.files_total.saturating_sub(totals.files);
        self.bytes_total = self.bytes_total.saturating_sub(totals.bytes);
    }

    /// Bytes still to write.
    pub fn bytes_left(&self) -> u64 {
        self.bytes_total.saturating_sub(self.bytes_done)
    }

    /// The percentage done by bytes, rounded down, so `100` means finished.
    pub fn percent(&self) -> u64 {
        if self.bytes_total == 0 {
            return 0;
        }
        (u128::from(self.bytes_done.min(self.bytes_total)) * 100 / u128::from(self.bytes_total))
            as u64
    }
}

/// The files and bytes under one source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    /// Files, counting each file inside a folder.
    pub files: u64,
    /// Their sizes added up.
    pub bytes: u64,
}

/// Adds up the files and bytes `source` would copy.
///
/// Follows the same rule as the copy: a folder is walked, anything else is a
/// file read through its link. What cannot be read is left out, since the copy
/// will fail on it and say so.
pub fn measure(source: &Path) -> Totals {
    match std::fs::metadata(source) {
        Ok(metadata) if metadata.is_dir() => {
            let mut totals = Totals::default();
            add_folder(source, &mut totals);
            totals
        }
        Ok(metadata) => Totals {
            files: 1,
            bytes: metadata.len(),
        },
        Err(_) => Totals::default(),
    }
}

fn add_folder(folder: &Path, totals: &mut Totals) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            add_folder(&entry.path(), totals);
        } else if let Ok(metadata) = std::fs::metadata(entry.path()) {
            totals.files += 1;
            totals.bytes += metadata.len();
        }
    }
}

/// Whether `source` and the folder `dest_dir` are on the same volume, so a
/// move between them is a rename that copies nothing.
///
/// On Windows this compares the drive or share the paths start with, which a
/// folder mounted inside another drive can fool. Being wrong costs only the
/// bar: a move thought to cross drives is measured for nothing, and one thought
/// not to shows its entry count instead.
pub fn same_volume(source: &Path, dest_dir: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (
            std::fs::symlink_metadata(source),
            std::fs::metadata(dest_dir),
        ) {
            (Ok(a), Ok(b)) => a.dev() == b.dev(),
            _ => true,
        }
    }
    #[cfg(not(unix))]
    {
        use std::path::Component;
        let prefix = |path: &Path| match path.components().next() {
            Some(Component::Prefix(prefix)) => {
                Some(prefix.as_os_str().to_string_lossy().to_ascii_lowercase())
            }
            _ => None,
        };
        match (prefix(source), prefix(dest_dir)) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }
}

/// How often a worker sends progress while bytes are being written.
pub const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Lets through at most one progress message per interval, so a fast copy
/// cannot flood the worker channel with one message per chunk.
#[derive(Debug)]
pub struct Throttle {
    every: Duration,
    last: Option<Instant>,
}

impl Throttle {
    /// A throttle allowing one message per `every`.
    pub fn new(every: Duration) -> Self {
        Self { every, last: None }
    }

    /// Whether a message may go at `now`. The first always may.
    pub fn allow(&mut self, now: Instant) -> bool {
        // `map_or` rather than `is_none_or`, which is newer than the crate's
        // minimum Rust version.
        let due = self.last.map_or(true, |last| {
            now.saturating_duration_since(last) >= self.every
        });
        if due {
            self.last = Some(now);
        }
        due
    }
}

/// The bar's width in cells, between its end marks.
const BAR_CELLS: u64 = 10;

/// The status-bar line for a job, in at most `width` columns:
/// `copying ▕██████░░░░▏ 58% · 1.7 / 2.9 GB · 214 files left`.
///
/// Narrower, it drops the files left, then the bytes, then the bar, down to
/// `copying 58%`. While the sources are measured it says `copying · counting…`,
/// and a job that copies nothing, such as a move on one drive, keeps the entry
/// count it always had: `moving 3/10`.
pub fn describe(kind: TransferKind, progress: &Progress, width: usize) -> String {
    let verb = kind.progressive();
    if progress.counting {
        return format!("{verb} · counting…");
    }
    if progress.bytes_total == 0 {
        return format!(
            "{verb} {}/{}",
            progress.entries_done, progress.entries_total
        );
    }

    let percent = progress.percent();
    let filled = (percent * BAR_CELLS / 100) as usize;
    let bar = format!(
        "▕{}{}▏",
        "█".repeat(filled),
        "░".repeat(BAR_CELLS as usize - filled)
    );
    let bytes = format_bytes_of(progress.bytes_done, progress.bytes_total);
    let files_left = progress.files_total.saturating_sub(progress.files_done);
    let files = match files_left {
        0 => None,
        1 => Some("1 file left".to_owned()),
        n => Some(format!("{n} files left")),
    };

    let mut forms = Vec::new();
    if let Some(files) = &files {
        forms.push(format!("{verb} {bar} {percent}% · {bytes} · {files}"));
    }
    forms.push(format!("{verb} {bar} {percent}% · {bytes}"));
    forms.push(format!("{verb} {bar} {percent}%"));
    let shortest = format!("{verb} {percent}%");
    forms
        .into_iter()
        .find(|form| form.chars().count() <= width)
        .unwrap_or(shortest)
}

/// The units, in powers of 1024.
const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

/// The unit `bytes` reads best in, as an index into [`UNITS`].
fn unit_for(bytes: u64) -> usize {
    let mut unit = 0;
    let mut size = bytes;
    while size >= 1024 && unit < UNITS.len() - 1 {
        size /= 1024;
        unit += 1;
    }
    unit
}

/// `bytes` in `unit`: whole numbers for B and KB, one decimal from MB up.
/// Rounded down, so a copy does not read as finished before it is.
fn in_unit(bytes: u64, unit: usize) -> String {
    let divisor = 1024u64.pow(unit as u32);
    if unit < 2 {
        format!("{}", bytes / divisor)
    } else {
        let tenths = u128::from(bytes) * 10 / u128::from(divisor);
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

/// A size on its own: `512 B`, `12 KB`, `1.2 GB`.
pub fn format_bytes(bytes: u64) -> String {
    let unit = unit_for(bytes);
    format!("{} {}", in_unit(bytes, unit), UNITS[unit])
}

/// Done and total in the total's unit: `1.7 / 2.9 GB`, never `1700 MB / 2.9 GB`.
pub fn format_bytes_of(done: u64, total: u64) -> String {
    let unit = unit_for(total);
    format!(
        "{} / {} {}",
        in_unit(done.min(total), unit),
        in_unit(total, unit),
        UNITS[unit]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    fn copying(done: u64, total: u64, files_left: u64) -> Progress {
        Progress {
            entries_done: 1,
            entries_total: 3,
            files_done: 10,
            files_total: 10 + files_left,
            bytes_done: done,
            bytes_total: total,
            ..Progress::default()
        }
    }

    #[test]
    fn the_units_change_at_each_power_of_1024() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(KB), "1 KB");
        assert_eq!(format_bytes(MB - 1), "1023 KB");
        assert_eq!(format_bytes(MB), "1.0 MB");
        assert_eq!(format_bytes(GB + GB / 2), "1.5 GB");
        assert_eq!(format_bytes(3 * 1024 * GB), "3.0 TB");
        assert_eq!(
            format_bytes(5000 * 1024 * GB),
            "5000.0 TB",
            "TB is the last unit"
        );
    }

    #[test]
    fn both_numbers_are_in_the_totals_unit() {
        assert_eq!(format_bytes_of(1700 * MB, 2970 * MB), "1.6 / 2.9 GB");
        assert_eq!(format_bytes_of(512, 900), "512 / 900 B");
        assert_eq!(format_bytes_of(3 * KB, 2 * MB), "0.0 / 2.0 MB");
    }

    #[test]
    fn done_is_rounded_down_so_it_is_not_finished_early() {
        assert_eq!(format_bytes_of(GB - 1, GB), "0.9 / 1.0 GB");
        assert_eq!(copying(GB - 1, GB, 0).percent(), 99);
        assert_eq!(copying(GB, GB, 0).percent(), 100);
    }

    #[test]
    fn the_full_line() {
        let line = describe(TransferKind::Copy, &copying(1740 * MB, 2970 * MB, 214), 80);
        assert_eq!(
            line,
            "copying ▕█████░░░░░▏ 58% · 1.6 / 2.9 GB · 214 files left"
        );
    }

    #[test]
    fn zero_and_a_hundred_percent() {
        let start = describe(TransferKind::Copy, &copying(0, GB, 3), 80);
        assert_eq!(
            start,
            "copying ▕░░░░░░░░░░▏ 0% · 0.0 / 1.0 GB · 3 files left"
        );
        let end = describe(TransferKind::Copy, &copying(GB, GB, 0), 80);
        assert_eq!(end, "copying ▕██████████▏ 100% · 1.0 / 1.0 GB");
    }

    #[test]
    fn narrower_drops_files_then_bytes_then_the_bar() {
        let progress = copying(1740 * MB, 2970 * MB, 214);
        let at = |width| describe(TransferKind::Copy, &progress, width);
        assert_eq!(at(40), "copying ▕█████░░░░░▏ 58% · 1.6 / 2.9 GB");
        assert_eq!(at(25), "copying ▕█████░░░░░▏ 58%");
        assert_eq!(at(15), "copying 58%");
        assert_eq!(at(3), "copying 58%", "the shortest form stays");
    }

    #[test]
    fn counting_and_a_rename_only_move() {
        let counting = Progress {
            counting: true,
            ..Progress::default()
        };
        assert_eq!(
            describe(TransferKind::Copy, &counting, 80),
            "copying · counting…"
        );
        let renames = Progress {
            entries_done: 3,
            entries_total: 10,
            ..Progress::default()
        };
        assert_eq!(describe(TransferKind::Move, &renames, 80), "moving 3/10");
    }

    #[test]
    fn one_file_left_is_singular() {
        let line = describe(TransferKind::Copy, &copying(MB, 2 * MB, 1), 80);
        assert!(line.ends_with("· 1 file left"), "{line}");
    }

    #[test]
    fn progress_is_let_through_ten_times_a_second() {
        let start = Instant::now();
        let mut throttle = Throttle::new(PROGRESS_EVERY);
        // A copy reporting every millisecond for two seconds.
        let sent = (0..2000)
            .filter(|ms| throttle.allow(start + Duration::from_millis(*ms)))
            .count();
        assert_eq!(sent, 20);
    }

    #[test]
    fn a_folder_is_measured_by_what_is_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("f");
        std::fs::create_dir_all(folder.join("sub")).unwrap();
        std::fs::write(folder.join("a"), vec![0; 100]).unwrap();
        std::fs::write(folder.join("sub/b"), vec![0; 50]).unwrap();

        assert_eq!(
            measure(&folder),
            Totals {
                files: 2,
                bytes: 150
            }
        );
        assert_eq!(
            measure(&folder.join("a")),
            Totals {
                files: 1,
                bytes: 100
            }
        );
        assert_eq!(measure(&dir.path().join("gone")), Totals::default());
    }

    #[test]
    fn a_folder_and_its_own_temp_dir_are_one_volume() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"x").unwrap();
        assert!(same_volume(&dir.path().join("a"), dir.path()));
    }

    #[cfg(windows)]
    #[test]
    fn two_drives_are_two_volumes() {
        assert!(!same_volume(Path::new(r"C:\a"), Path::new(r"D:\b")));
        assert!(same_volume(Path::new(r"C:\a"), Path::new(r"c:\b")));
    }
}
