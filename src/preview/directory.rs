// src/preview/directory.rs
// Directory summary preview. No external tools required.

use std::io::ErrorKind;
use std::time::SystemTime;

/// Render a summary of a directory's immediate contents.
///
/// Counts total items, directories, regular files, and symlinks. Sums the
/// size of direct children via `symlink_metadata` (so symlinks themselves
/// are measured, not their targets). Tracks the newest modification time.
///
/// On permission denied, returns an `Unavailable` variant with a clear
/// message. Other read_dir errors are also reported as `Unavailable`.
pub fn render(path: &std::path::Path) -> crate::preview::PreviewContent {
    let read_dir = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(e) => {
            let msg = if e.kind() == ErrorKind::PermissionDenied {
                "permission denied reading directory".to_owned()
            } else {
                format!("cannot read directory '{}': {}", path.display(), e)
            };
            return crate::preview::PreviewContent::Unavailable(msg);
        }
    };

    let mut item_count: usize = 0;
    let mut dirs: usize = 0;
    let mut files: usize = 0;
    let mut symlinks: usize = 0;
    let mut total_size: u64 = 0;
    let mut newest_mtime: Option<SystemTime> = None;

    for entry_result in read_dir {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue, // Skip unreadable entries gracefully.
        };

        item_count += 1;

        // Use symlink_metadata so we measure the link itself, not the target.
        let meta = match entry.path().symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let file_type = meta.file_type();
        if file_type.is_symlink() {
            symlinks += 1;
        } else if file_type.is_dir() {
            dirs += 1;
        } else {
            files += 1;
        }

        total_size = total_size.saturating_add(meta.len());

        if let Ok(mtime) = meta.modified() {
            newest_mtime = Some(match newest_mtime {
                None => mtime,
                Some(current) => {
                    if mtime > current {
                        mtime
                    } else {
                        current
                    }
                }
            });
        }
    }

    crate::preview::PreviewContent::DirSummary {
        item_count,
        dirs,
        files,
        symlinks,
        total_size,
        newest_mtime,
    }
}
