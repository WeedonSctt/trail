//! Copy and move progress (§2.27 of `upcoming_features.md`), driven through
//! `transfer::run` on a temporary folder.
//!
//! What these cannot show is how fast it feels on a real large copy, a USB
//! drive or a network share, and how the bar looks in a real terminal.

use std::fs;
use std::path::Path;

use trail::actions::fs_ops::CHUNKED_FROM;
use trail::actions::progress::Progress;
use trail::actions::transfer::{run, TransferItem, TransferJob, TransferKind};

fn job(kind: TransferKind, items: Vec<TransferItem>) -> TransferJob {
    TransferJob {
        id: 1,
        kind,
        label: "cp".to_owned(),
        items,
    }
}

fn item(source: &Path, dest: &Path) -> TransferItem {
    TransferItem {
        source: source.to_owned(),
        dest: dest.to_owned(),
    }
}

/// Every report `run` makes for `job`.
fn reports(job: &TransferJob) -> Vec<Progress> {
    let mut seen = Vec::new();
    run(job, |progress| seen.push(*progress));
    seen
}

/// A folder of three small files and a large one, which takes the chunked path.
fn folder_with_a_large_file(root: &Path) -> (std::path::PathBuf, u64) {
    let folder = root.join("data");
    fs::create_dir_all(folder.join("sub")).unwrap();
    fs::write(folder.join("a.txt"), vec![b'a'; 1000]).unwrap();
    fs::write(folder.join("b.txt"), vec![b'b'; 2000]).unwrap();
    fs::write(folder.join("sub/c.txt"), vec![b'c'; 3000]).unwrap();
    let large = CHUNKED_FROM as usize + 1_500_000;
    fs::write(folder.join("large.bin"), patterned(large)).unwrap();
    (folder, (6000 + large) as u64)
}

/// Bytes that differ from offset to offset, so a chunk written in the wrong
/// place would show.
fn patterned(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

#[test]
fn the_totals_are_known_before_the_first_byte_is_copied() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, bytes) = folder_with_a_large_file(dir.path());

    let seen = reports(&job(
        TransferKind::Copy,
        vec![item(&folder, &dir.path().join("copy"))],
    ));

    let first = seen[0];
    assert!(!first.counting);
    assert_eq!((first.files_total, first.bytes_total), (4, bytes));
    assert_eq!((first.files_done, first.bytes_done), (0, 0));
}

#[test]
fn the_bytes_add_up_to_the_total_at_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, bytes) = folder_with_a_large_file(dir.path());

    let seen = reports(&job(
        TransferKind::Copy,
        vec![item(&folder, &dir.path().join("copy"))],
    ));

    let last = seen.last().unwrap();
    assert_eq!((last.files_done, last.bytes_done), (4, bytes));
    assert_eq!(last.percent(), 100);
    assert!(
        seen.windows(2).all(|w| w[0].bytes_done <= w[1].bytes_done),
        "the count never goes back"
    );
    // The large file reports chunk by chunk, not only once it is done.
    let partway = seen
        .iter()
        .filter(|p| p.bytes_done > 0 && p.bytes_done < bytes)
        .count();
    assert!(partway > 3, "{partway} reports while copying");
}

#[test]
fn a_large_file_keeps_its_contents_and_its_read_only_flag() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("large.bin");
    let contents = patterned(CHUNKED_FROM as usize * 2 + 12345);
    fs::write(&source, &contents).unwrap();
    let mut permissions = fs::metadata(&source).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&source, permissions).unwrap();
    let modified = fs::metadata(&source).unwrap().modified().unwrap();
    let dest = dir.path().join("copy.bin");

    let report = run(&job(TransferKind::Copy, vec![item(&source, &dest)]), |_| {});

    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(fs::read(&dest).unwrap() == contents, "the contents differ");
    let copied = fs::metadata(&dest).unwrap();
    assert!(copied.permissions().readonly());
    assert_eq!(copied.modified().unwrap(), modified);

    // A read-only file stops the temp folder from being cleaned up on Windows.
    for path in [&source, &dest] {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        // clippy: permissions_set_readonly_false — only undoing the flag set above
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }
}

/// A move on one drive is a rename, which copies nothing, so there is no bar:
/// the status bar keeps the entry count.
#[test]
fn a_move_on_one_drive_shows_no_bar() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, _) = folder_with_a_large_file(dir.path());

    let seen = reports(&job(
        TransferKind::Move,
        vec![item(&folder, &dir.path().join("moved"))],
    ));

    assert!(seen.iter().all(|p| p.bytes_total == 0), "{seen:?}");
    assert_eq!(seen.last().unwrap().entries_done, 1);
    assert!(dir.path().join("moved/large.bin").exists());
}

/// An entry that is skipped takes its share out of the totals, so the bar can
/// still reach the end.
#[test]
fn a_skipped_entry_leaves_the_totals() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), vec![0; 100]).unwrap();
    fs::write(dir.path().join("b.txt"), vec![0; 300]).unwrap();
    fs::create_dir(dir.path().join("out")).unwrap();
    fs::write(dir.path().join("out/b.txt"), b"taken").unwrap();

    let seen = reports(&job(
        TransferKind::Copy,
        vec![
            item(&dir.path().join("a.txt"), &dir.path().join("out/a.txt")),
            item(&dir.path().join("b.txt"), &dir.path().join("out/b.txt")),
        ],
    ));

    assert_eq!(seen[0].bytes_total, 400);
    let last = seen.last().unwrap();
    assert_eq!((last.bytes_done, last.bytes_total), (100, 100));
    assert_eq!(last.percent(), 100);
}
