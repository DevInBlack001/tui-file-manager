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
    /// Raw terminal graphics protocol payload (chafa's `--format kitty` or
    /// `--format sixels` output, optionally wrapped for a multiplexer via
    /// `--passthrough`) for a real raster image, to be written directly to
    /// the terminal, bypassing ratatui's cell buffer entirely. Only
    /// produced when the terminal was detected (see
    /// `image::detect_graphics_format` - env vars or the local `omarchy`
    /// CLI helper, never a live terminal query) to support it.
    RawGraphics(Vec<u8>),
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

impl Default for Previewer {
    fn default() -> Self {
        Self::new()
    }
}

impl Previewer {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::sync_channel::<PreviewRequest>(1);
        let (res_tx, res_rx) = mpsc::channel::<(PathBuf, PreviewContent)>();

        thread::spawn(move || {
            let truecolor = image::detect_truecolor();
            let graphics = image::detect_graphics_format();
            let passthrough = image::detect_passthrough();
            while let Ok(req) = req_rx.recv() {
                let content = render_preview(&req, truecolor, graphics, passthrough);
                let _ = res_tx.send((req.path, content));
            }
        });

        Self {
            sender: req_tx,
            receiver: res_rx,
        }
    }

    #[allow(clippy::too_many_arguments)]
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

// OpenVPN config files are plain text, so without this check they'd go
// through the normal text preview - but they commonly embed certificates,
// private keys, or plaintext credentials inline. Their content is never
// shown; a glyph replaces it unconditionally.
const VPN_EXTENSIONS: &[&str] = &["ovpn"];

// OpenDocument and Microsoft Office document formats. These are zip
// containers under the hood, but showing them as a generic "ARCHIVE" glyph
// (or a hex dump, since mime_guess doesn't always resolve them to something
// preview-friendly) is misleading - they're documents, not archives a user
// would want to unpack.
const DOCUMENT_EXTENSIONS: &[&str] = &[
    // OpenDocument family (.ods and its siblings/alternatives)
    "ods", "ots", "odt", "ott", "odp", "otp", "odg", "otg", "odf", "odc",
    // Microsoft Office family
    "xlsx", "xlsm", "xls", "docx", "docm", "doc", "pptx", "pptm", "ppt",
];

fn render_preview(
    req: &PreviewRequest,
    truecolor: bool,
    graphics: Option<&str>,
    passthrough: Option<&str>,
) -> PreviewContent {
    let path = &req.path;
    if req.is_dir {
        return directory::render(path);
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if DOCUMENT_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewContent::Glyph(glyph::document());
    }
    if DISC_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewContent::Glyph(glyph::disc());
    }
    if ARCHIVE_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewContent::Glyph(glyph::archive());
    }
    if TORRENT_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewContent::Glyph(glyph::torrent());
    }
    if VPN_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewContent::Glyph(glyph::vpn());
    }

    let mime = crate::fs::mime::detect(path);
    if crate::fs::mime::is_text(&mime) {
        text::render(path, req.max_text_lines)
    } else if crate::fs::mime::is_image(&mime) {
        image::render(path, req.width, req.height, truecolor, graphics, passthrough)
    } else if crate::fs::mime::is_video(&mime) && req.video_thumbs {
        video::render(path, req.width, req.height, truecolor, graphics, passthrough)
    } else if crate::fs::mime::is_pdf(&mime) {
        pdf::render(path, req.max_text_lines)
    } else {
        hex::render(path, req.max_binary_bytes)
    }
}
