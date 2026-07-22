//! Directory preview provider.
//!
//! Synchronous preview showing directory contents, file count, directory count,
//! and hidden file count.

use std::fs;

use crate::app::state::{Entry, EntryKind};
use crate::preview::provider::{PreviewContent, PreviewCtx, PreviewOutcome, PreviewProvider};

/// Maximum number of child entry names shown in the directory preview.
const MAX_PREVIEW_ENTRIES: usize = 64;

/// Synchronous preview provider for directory entries.
///
/// Reads the first `MAX_PREVIEW_ENTRIES` children from disk and returns
/// summary counts (files, dirs, hidden). This is cheap enough to run on
/// the UI thread; no worker task is spawned.
pub struct DirectoryProvider;

impl PreviewProvider for DirectoryProvider {
    fn can_handle(&self, entry: &Entry) -> bool {
        entry.kind == EntryKind::Dir
    }

    fn preview(&self, entry: &Entry, ctx: &PreviewCtx) -> PreviewOutcome {
        let content = build_directory_preview(&entry.path, ctx.show_hidden);
        PreviewOutcome::Ready(content)
    }
}

/// Builds a `PreviewContent::Directory` for `path`.
///
/// Returns `PreviewContent::Empty` if the directory cannot be read.
pub fn build_directory_preview(path: &std::path::Path, show_hidden: bool) -> PreviewContent {
    let read = match fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => return PreviewContent::Empty,
    };

    let mut file_count = 0usize;
    let mut dir_count = 0usize;
    let mut hidden_count = 0usize;
    let mut names: Vec<String> = Vec::new();

    for res in read {
        let de = match res {
            Ok(d) => d,
            Err(_) => continue,
        };

        let name = match de.file_name().to_str() {
            Some(n) => n.to_owned(),
            None => continue,
        };

        let is_hidden = name.starts_with('.');
        if is_hidden {
            hidden_count += 1;
        }

        let metadata = de.metadata().ok();
        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);

        if is_dir {
            dir_count += 1;
        } else {
            file_count += 1;
        }

        if (!is_hidden || show_hidden) && names.len() < MAX_PREVIEW_ENTRIES {
            // Suffix directories with `/` for quick visual identification.
            if is_dir {
                names.push(format!("{name}/"));
            } else {
                names.push(name);
            }
        }
    }

    // Sort names: directories first (those ending with `/`), then files.
    names.sort_by(|a, b| {
        let a_dir = a.ends_with('/');
        let b_dir = b.ends_with('/');
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.to_lowercase().cmp(&b.to_lowercase()),
        }
    });

    PreviewContent::Directory {
        file_count,
        dir_count,
        hidden_count,
        entries: names,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as sfs;
    use tempfile::tempdir;

    #[test]
    fn directory_preview_counts() {
        let dir = tempdir().unwrap();
        sfs::create_dir(dir.path().join("subdir")).unwrap();
        sfs::write(dir.path().join("file.txt"), b"").unwrap();
        sfs::write(dir.path().join(".hidden"), b"").unwrap();

        let content = build_directory_preview(dir.path(), false);

        if let PreviewContent::Directory {
            file_count,
            dir_count,
            hidden_count,
            entries,
        } = content
        {
            assert_eq!(dir_count, 1);
            assert_eq!(file_count, 2); // 'file.txt' and '.hidden'
            assert_eq!(hidden_count, 1);
            // With show_hidden=false, .hidden is excluded from entries.
            assert!(!entries.iter().any(|e| e.contains("hidden")));
        } else {
            panic!("expected Directory variant");
        }
    }

    #[test]
    fn directory_preview_shows_hidden_when_requested() {
        let dir = tempdir().unwrap();
        sfs::write(dir.path().join(".hidden"), b"").unwrap();

        let content = build_directory_preview(dir.path(), true);

        if let PreviewContent::Directory { entries, .. } = content {
            assert!(entries.iter().any(|e| e.contains("hidden")));
        } else {
            panic!("expected Directory variant");
        }
    }
}
