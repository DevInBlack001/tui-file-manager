// core/listing.rs - Directory reading, sorting, and filtering.

use crate::config::SortKey;
use crate::core::entry::Entry;
use std::io;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when reading a directory listing.
#[derive(Debug)]
pub enum ListError {
    /// The process lacks read permission on the directory.
    PermissionDenied,
    /// The path does not exist.
    NotFound,
    /// The path exists but is not a directory.
    NotADirectory,
    /// Any other OS-level error.
    Other(String),
}

impl From<io::Error> for ListError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::PermissionDenied => ListError::PermissionDenied,
            io::ErrorKind::NotFound         => ListError::NotFound,
            _                               => ListError::Other(e.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Listing struct
// ---------------------------------------------------------------------------

/// The result of reading a directory.
pub struct Listing {
    /// The directory that was read.
    pub path: PathBuf,
    /// All entries (filtered, sorted) ready for display.
    pub entries: Vec<Entry>,
    /// Non-fatal error encountered during the read (e.g. one unreadable entry).
    pub error: Option<ListError>,
}

// ---------------------------------------------------------------------------
// read_dir
// ---------------------------------------------------------------------------

/// Read a directory and return a sorted, optionally filtered `Listing`.
///
/// Sort order:
///   Primary key   - directories before files (always, ignoring sort_reverse).
///   Secondary key - `sort_key` (Name/Size/Mtime/Type).
///   Tiebreaker    - name.to_lowercase() ascending.
///
/// `sort_reverse` is applied _within_ each group (dirs and files separately)
/// so that directories remain above files regardless of reverse status.
///
/// Hidden entries (name starts with '.') are included only when `show_hidden`
/// is true.
pub fn read_dir(
    path: &Path,
    show_hidden: bool,
    sort_key: SortKey,
    sort_reverse: bool,
) -> Listing {
    // Validate that the path is actually a directory before trying to read it.
    match std::fs::metadata(path) {
        Err(e) => {
            return Listing {
                path: path.to_path_buf(),
                entries: Vec::new(),
                error: Some(ListError::from(e)),
            };
        }
        Ok(meta) if !meta.is_dir() => {
            return Listing {
                path: path.to_path_buf(),
                entries: Vec::new(),
                error: Some(ListError::NotADirectory),
            };
        }
        Ok(_) => {}
    }

    let read_result = std::fs::read_dir(path);
    let rd = match read_result {
        Err(e) => {
            return Listing {
                path: path.to_path_buf(),
                entries: Vec::new(),
                error: Some(ListError::from(e)),
            };
        }
        Ok(rd) => rd,
    };

    let mut entries: Vec<Entry> = Vec::new();
    let mut soft_error: Option<ListError> = None;

    for result in rd {
        match result {
            Err(e) => {
                // Record the first per-entry error but continue so one
                // unreadable entry does not hide the rest of the listing.
                if soft_error.is_none() {
                    soft_error = Some(ListError::from(e));
                }
            }
            Ok(de) => {
                let entry_path = de.path();
                let name = match entry_path.file_name() {
                    Some(n) => n.to_string_lossy().into_owned(),
                    None    => continue,
                };
                if !show_hidden && name.starts_with('.') {
                    continue;
                }
                entries.push(Entry::from_path(entry_path));
            }
        }
    }

    sort_entries(&mut entries, sort_key, sort_reverse);

    Listing {
        path: path.to_path_buf(),
        entries,
        error: soft_error,
    }
}

// ---------------------------------------------------------------------------
// sort_entries (internal)
// ---------------------------------------------------------------------------

fn sort_entries(entries: &mut Vec<Entry>, key: SortKey, reverse: bool) {
    // Partition into dirs and files in-place via stable sort with a sentinel.
    // We re-sort each group separately, then concatenate.
    let mut dirs:  Vec<Entry> = Vec::new();
    let mut files: Vec<Entry> = Vec::new();

    for e in entries.drain(..) {
        if e.is_dir {
            dirs.push(e);
        } else {
            files.push(e);
        }
    }

    sort_group(&mut dirs,  key, reverse);
    sort_group(&mut files, key, reverse);

    entries.extend(dirs);
    entries.extend(files);
}

fn sort_group(group: &mut [Entry], key: SortKey, reverse: bool) {
    group.sort_by(|a, b| {
        let primary = match key {
            SortKey::Name => std::cmp::Ordering::Equal,
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Mtime => a.mtime.cmp(&b.mtime),
            SortKey::Type => {
                // Sort by extension (case-insensitive), then fall through.
                let ext_a = a.path.extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let ext_b = b.path.extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                ext_a.cmp(&ext_b)
            }
        };
        // Tiebreaker: name lowercase ascending (always, before applying reverse).
        let order = primary.then_with(|| {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        });
        if reverse { order.reverse() } else { order }
    });
}

// ---------------------------------------------------------------------------
// filter_entries
// ---------------------------------------------------------------------------

/// Return references to entries whose name contains `pattern` (case-insensitive).
///
/// An empty pattern matches everything.
pub fn filter_entries<'a>(entries: &'a [Entry], pattern: &str) -> Vec<&'a Entry> {
    if pattern.is_empty() {
        return entries.iter().collect();
    }
    let lower = pattern.to_lowercase();
    entries
        .iter()
        .filter(|e| e.name.to_lowercase().contains(&lower))
        .collect()
}
