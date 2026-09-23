// core/recents.rs - Persist recently visited paths to XDG state.
//
// State file: $XDG_STATE_HOME/tui-fm/recents.json
// Fallback  : $HOME/.local/state/tui-fm/recents.json
//
// Security properties:
//   - File size capped at 512 KiB before reading.
//   - Each stored path capped at 4096 bytes.
//   - Corrupt or missing JSON starts an empty list (no panic).
//   - Atomic write: tmp file then rename.
//   - No symlink follow on the state file itself (checked before open).

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Recents struct
// ---------------------------------------------------------------------------

/// A bounded, deduplicated history of visited paths, persisted as JSON.
pub struct Recents {
    paths: VecDeque<PathBuf>,
    max: usize,
    state_file: PathBuf,
}

// ---------------------------------------------------------------------------
// JSON serialization shape
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct RecentsFile {
    paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// Path cap constant
// ---------------------------------------------------------------------------

const PATH_CAP_BYTES:  usize = 4096;
const FILE_CAP_BYTES:  u64   = 512 * 1024; // 512 KiB

// ---------------------------------------------------------------------------
// impl Recents
// ---------------------------------------------------------------------------

impl Recents {
    /// Load recents from the state file, or start empty on any error.
    ///
    /// `max` is the maximum number of entries to retain.
    pub fn load(max: usize) -> Self {
        let state_file = resolve_state_file();

        let paths = load_from_file(&state_file, max);

        Self {
            paths,
            max,
            state_file,
        }
    }

    /// Push a new path to the front of the list.
    ///
    /// Deduplicates: if the path is already present it is moved to the front.
    /// Trims the list to `max` entries after insertion.
    pub fn push(&mut self, path: PathBuf) {
        // Remove any existing occurrence so the push-to-front deduplicates.
        self.paths.retain(|p| p != &path);
        self.paths.push_front(path);
        while self.paths.len() > self.max {
            self.paths.pop_back();
        }
    }

    /// Borrow the current list of recent paths (most recent first).
    pub fn paths(&self) -> &VecDeque<PathBuf> {
        &self.paths
    }

    /// Atomically persist the current list to disk.
    ///
    /// Writes to `<state_file>.tmp` then renames over the real path.
    /// On any I/O error the failure is silently ignored (recents is a
    /// convenience feature, not critical data). This must never write to
    /// stderr: `save()` runs while the TUI owns the alternate screen, and an
    /// unmanaged stderr write corrupts the display.
    pub fn save(&self) {
        let _ = self.save_inner();
    }

    fn save_inner(&self) -> io::Result<()> {
        // Ensure the parent directory exists.
        if let Some(parent) = self.state_file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Build tmp path alongside the real file.
        let tmp = self.state_file
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(".recents.json.tmp");

        let data = RecentsFile {
            paths: self
                .paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        };

        let json = serde_json::to_string(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.flush()?;
        }

        std::fs::rename(&tmp, &self.state_file)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Resolve the state file path without hardcoding any absolute path.
///
/// Resolution order:
///   1. `dirs::state_dir()` / tui-fm / recents.json
///   2. `$HOME/.local/state/tui-fm/recents.json`
fn resolve_state_file() -> PathBuf {
    if let Some(state) = dirs::state_dir() {
        return state.join("tui-fm").join("recents.json");
    }
    // Fallback: construct the XDG-compliant default manually.
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    home.join(".local").join("state").join("tui-fm").join("recents.json")
}

/// Load and validate entries from the JSON state file.
///
/// Returns an empty VecDeque on any error (missing file, bad JSON, I/O error).
fn load_from_file(state_file: &std::path::Path, max: usize) -> VecDeque<PathBuf> {
    // Refuse to follow symlinks on the state file.
    if state_file.is_symlink() {
        eprintln!("[fim/recents] state file is a symlink; starting empty");
        return VecDeque::new();
    }

    let f = match std::fs::File::open(state_file) {
        Ok(f) => f,
        // File not yet created: start empty silently.
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => return VecDeque::new(),
        Err(e) => {
            eprintln!("[fim/recents] could not open state file: {e}; starting empty");
            return VecDeque::new();
        }
    };

    // Cap bytes read to prevent memory explosion on a corrupted file.
    let mut buf = String::new();
    if let Err(e) = io::BufReader::new(f.take(FILE_CAP_BYTES)).read_to_string(&mut buf) {
        eprintln!("[fim/recents] read error: {e}; starting empty");
        return VecDeque::new();
    }

    let parsed: RecentsFile = match serde_json::from_str(&buf) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("[fim/recents] corrupt JSON; starting empty");
            return VecDeque::new();
        }
    };

    parsed
        .paths
        .into_iter()
        // Cap each individual path length.
        .filter(|s| s.len() <= PATH_CAP_BYTES)
        .take(max)
        .map(PathBuf::from)
        .collect()
}
