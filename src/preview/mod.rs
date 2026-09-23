pub mod directory;
pub mod glyph;
pub mod hex;
pub mod image;
pub mod pdf;
pub mod text;
pub mod video;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;

/// A theme role for a glyph segment. Kept independent of any UI framework
/// type (this module never depends on ratatui) - `ui::preview` maps these to
/// the live `Theme`'s actual colours at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphColor {
    Fg,
    FgDim,
    Accent,
    Muted,
    Yellow,
    Cyan,
}

/// One line of a hand-drawn ASCII glyph: a sequence of (text, colour role)
/// segments, so a single line can carry multiple colours.
pub type GlyphLine = Vec<(String, GlyphColor)>;

/// The result of a preview render operation, consumed by the UI layer.
#[derive(Debug, Clone)]
pub enum PreviewContent {
    /// Plain UTF-8 text lines (no ANSI escapes).
    Text(Vec<String>),
    /// ANSI-coloured text lines, e.g. from bat(1).
    AnsiText(Vec<String>),
    /// Pre-rendered chafa ANSI lines.
    ChafaLines(Vec<String>),
    /// Hex dump lines.
    HexDump(Vec<String>),
    /// A hand-drawn, theme-coloured ASCII glyph (e.g. disc icon for .iso,
    /// archive icon for .zip) shown instead of a hex dump for well-known
    /// binary container formats.
    Glyph(Vec<GlyphLine>),
    /// Directory summary lines.
    DirSummary {
        item_count: usize,
        dirs: usize,
        files: usize,
        symlinks: usize,
        total_size: u64,
        newest_mtime: Option<std::time::SystemTime>,
    },
    /// Preview cannot be generated; the string explains why.
    Unavailable(String),
    /// Preview is currently loading in background.
    Loading,
}

#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub path: PathBuf,
    pub is_dir: bool,
    pub width: u16,
    pub height: u16,
    pub max_text_lines: usize,
    pub max_binary_bytes: usize,
    pub video_thumbs: bool,
}

pub struct Previewer {
    sender: SyncSender<PreviewRequest>,
    pub receiver: Receiver<(PathBuf, PreviewContent)>,
}

impl Previewer {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::sync_channel::<PreviewRequest>(1);
        let (res_tx, res_rx) = mpsc::channel::<(PathBuf, PreviewContent)>();

        thread::spawn(move || {
            let truecolor = image::detect_truecolor();
            while let Ok(req) = req_rx.recv() {
                let content = render_preview(&req, truecolor);
                let _ = res_tx.send((req.path, content));
            }
        });

        Self {
            sender: req_tx,
            receiver: res_rx,
        }
    }

    pub fn request(
        &self,
        path: PathBuf,
        is_dir: bool,
        width: u16,
        height: u16,
        max_text_lines: usize,
        max_binary_bytes: usize,
        video_thumbs: bool,
    ) {
        let req = PreviewRequest {
            path,
            is_dir,
            width,
            height,
            max_text_lines,
            max_binary_bytes,
            video_thumbs,
        };
        // Bounded channel: discard request if worker is busy with previous item
        let _ = self.sender.try_send(req);
    }
}

const DISC_EXTENSIONS: &[&str] = &["iso", "img", "bin", "nrg", "mdf", "toast", "dmg"];

// Every well-known archive/compression container, so the glyph isn't limited
// to just .zip: general-purpose archives, tarballs (bare and pre-compressed
// single-file forms), package formats built on the same containers, and
// legacy/less common compressors.
const ARCHIVE_EXTENSIONS: &[&str] = &[
    // general-purpose archives
    "zip", "7z", "rar", "cab", "ar", "cpio", "arj", "lha", "lzh",
    // tar and its compressed variants
    "tar", "tgz", "tbz", "tbz2", "txz", "tzst", "taz", "tlz",
    // standalone compressors
    "gz", "bz2", "xz", "zst", "lz", "lz4", "lzma", "lzo", "z",
    // archive-based package formats
    "jar", "war", "ear", "apk", "xpi", "whl", "deb", "rpm", "crx",
];
const TORRENT_EXTENSIONS: &[&str] = &["torrent"];

fn render_preview(req: &PreviewRequest, truecolor: bool) -> PreviewContent {
    let path = &req.path;
    if req.is_dir {
        return directory::render(path);
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if DISC_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewContent::Glyph(glyph::disc());
    }
    if ARCHIVE_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewContent::Glyph(glyph::archive());
    }
    if TORRENT_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewContent::Glyph(glyph::torrent());
    }

    let mime = crate::fs::mime::detect(path);
    if crate::fs::mime::is_text(&mime) {
        text::render(path, req.max_text_lines)
    } else if crate::fs::mime::is_image(&mime) {
        image::render(path, req.width, req.height, truecolor)
    } else if crate::fs::mime::is_video(&mime) && req.video_thumbs {
        video::render(path, req.width, req.height, truecolor)
    } else if crate::fs::mime::is_pdf(&mime) {
        pdf::render(path, req.max_text_lines)
    } else {
        hex::render(path, req.max_binary_bytes)
    }
}
