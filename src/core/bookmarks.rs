// core/bookmarks.rs - Sidebar bookmark sections built from XDG dirs and user config.

use crate::fs::xdg::XdgDirs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single bookmark entry rendered in the sidebar.
#[derive(Clone, Debug)]
pub struct Bookmark {
    /// Label shown in the sidebar.
    pub name: String,
    /// Target path for navigation.
    pub path: PathBuf,
    /// False when the path does not exist or is not a directory.
    pub exists: bool,
    /// True for the special "Recents" pseudo-entry (opens the recents list).
    pub is_recents: bool,
}

/// A user-defined bookmark read from config.toml.
#[derive(Clone, Debug)]
pub struct CustomBookmark {
    pub name: String,
    pub path: PathBuf,
}

/// The full set of sidebar bookmarks, split into standard sections and user
/// custom bookmarks.
#[derive(Clone, Debug)]
pub struct Bookmarks {
    /// Standard sections (Home, Documents, Pictures, Videos, Work, Recents).
    pub sections: Vec<Bookmark>,
    /// User-defined entries from config.toml [[bookmarks.custom]].
    pub custom: Vec<Bookmark>,
}

// ---------------------------------------------------------------------------
// build
// ---------------------------------------------------------------------------

/// Build sidebar bookmarks from XDG directories and user-supplied custom entries.
///
/// Standard sections (in sidebar order):
///   Home       - always present; uses `xdg.home`.
///   Documents  - present if `xdg.documents` is `Some` and is a directory.
///   Downloads  - present if `xdg.downloads` is `Some` and is a directory.
///   Pictures   - present if `xdg.pictures` is `Some` and is a directory.
///   Videos     - present if `xdg.videos` is `Some` and is a directory.
///   Work       - present if `xdg.work` is `Some` and is a directory.
///   Recents    - always present as a special pseudo-entry (is_recents=true).
pub fn build(xdg: &XdgDirs, custom: &[CustomBookmark]) -> Bookmarks {
    let mut sections: Vec<Bookmark> = Vec::new();

    // Home is always included. By definition it exists (dirs::home_dir is non-empty
    // on any POSIX system, and we resolved it to "/" as a fallback).
    sections.push(Bookmark {
        name: "Home".to_string(),
        path: xdg.home.clone(),
        exists: xdg.home.is_dir(),
        is_recents: false,
    });

    // Optional standard XDG directories.
    let optional: &[(&str, &Option<PathBuf>)] = &[
        ("Documents", &xdg.documents),
        ("Downloads", &xdg.downloads),
        ("Pictures",  &xdg.pictures),
        ("Videos",    &xdg.videos),
        ("Work",      &xdg.work),
    ];

    for (label, opt_path) in optional {
        if let Some(path) = opt_path {
            let exists = path.is_dir();
            sections.push(Bookmark {
                name: label.to_string(),
                path: path.clone(),
                exists,
                is_recents: false,
            });
        }
    }

    // Recents is always the last standard section and is a special pseudo-entry:
    // navigating to it opens the recents list rather than a real directory.
    sections.push(Bookmark {
        name: "Recents".to_string(),
        path: PathBuf::new(), // placeholder; app.rs handles is_recents specially
        exists: true,
        is_recents: true,
    });

    // User custom bookmarks.
    let custom_bookmarks: Vec<Bookmark> = custom
        .iter()
        .map(|cb| {
            let exists = cb.path.is_dir();
            Bookmark {
                name: cb.name.clone(),
                path: cb.path.clone(),
                exists,
                is_recents: false,
            }
        })
        .collect();

    Bookmarks {
        sections,
        custom: custom_bookmarks,
    }
}
