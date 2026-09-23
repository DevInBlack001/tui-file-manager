// core/clipboard.rs - Cut/copy state for fim.
//
// Paste is NOT implemented here. app.rs calls TransferClient::enqueue when
// the user presses 'p', using the paths stored here.

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// ClipMode
// ---------------------------------------------------------------------------

/// Whether the clipboard holds a copy or a move intent.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipMode {
    Copy,
    Move,
}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

/// Clipboard state: a list of paths and an optional operation mode.
///
/// The clipboard starts empty (`mode` is None, `paths` is empty).
/// After `set()` both are populated. After `clear()` both reset.
#[derive(Default, Clone, Debug)]
pub struct Clipboard {
    /// Paths staged for the next paste operation.
    pub paths: Vec<PathBuf>,
    /// The operation to perform on paste (None when clipboard is empty).
    pub mode: Option<ClipMode>,
}

impl Clipboard {
    /// Populate the clipboard with `paths` and a `mode`, replacing any prior
    /// contents.
    pub fn set(&mut self, paths: Vec<PathBuf>, mode: ClipMode) {
        self.paths = paths;
        self.mode  = Some(mode);
    }

    /// Clear all clipboard contents.
    pub fn clear(&mut self) {
        self.paths.clear();
        self.mode = None;
    }

    /// True when the clipboard holds no paths.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Short human-readable description of clipboard contents.
    ///
    /// Examples: "copy 3 items", "move 1 item", "" (empty).
    pub fn describe(&self) -> String {
        match &self.mode {
            None => String::new(),
            Some(mode) => {
                let verb = match mode {
                    ClipMode::Copy => "copy",
                    ClipMode::Move => "move",
                };
                let n = self.paths.len();
                let noun = if n == 1 { "item" } else { "items" };
                format!("{} {} {}", verb, n, noun)
            }
        }
    }
}
