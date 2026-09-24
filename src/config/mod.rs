pub mod loader;

use serde::{Deserialize, Deserializer};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Mtime,
    Type,
}


#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ImageRenderer {
    #[default]
    Auto,
    Chafa,
    None,
}


/// Where to load the colour theme from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub enum ThemeSource {
    /// Detect automatically: try Omarchy shell.toml, then built-in.
    #[default]
    Auto,
    /// Always use the built-in fallback palette.
    Builtin,
    /// Load from an explicit file path.
    Path(PathBuf),
}


// Custom deserializer for ThemeSource so TOML can express it as a string or
// as a table with a "path" key.
impl<'de> Deserialize<'de> for ThemeSource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Accept either a plain string or a map { path = "..." }.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Str(String),
            Map { path: String },
        }

        match Raw::deserialize(d)? {
            Raw::Str(s) => match s.to_lowercase().as_str() {
                "auto"    => Ok(ThemeSource::Auto),
                "builtin" => Ok(ThemeSource::Builtin),
                other     => Ok(ThemeSource::Path(PathBuf::from(other))),
            },
            Raw::Map { path } => Ok(ThemeSource::Path(PathBuf::from(path))),
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub show_hidden: bool,
    pub sort_key: SortKey,
    pub sort_reverse: bool,
    /// Sidebar panel width as a percentage of terminal width (1-99).
    pub sidebar_width_pct: u8,
    /// Preview panel width as a percentage of terminal width (1-99).
    pub preview_width_pct: u8,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_hidden:       false,
            sort_key:          SortKey::Name,
            sort_reverse:      false,
            sidebar_width_pct: 18,
            preview_width_pct: 36,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PreviewConfig {
    pub enabled: bool,
    /// Maximum lines to read for text preview.
    pub max_text_lines: usize,
    /// Maximum bytes to display for binary preview.
    pub max_binary_bytes: usize,
    pub image_renderer: ImageRenderer,
    pub video_thumbs: bool,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            enabled:          true,
            max_text_lines:   200,
            max_binary_bytes: 512,
            image_renderer:   ImageRenderer::Auto,
            video_thumbs:     true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub source: ThemeSource,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            source: ThemeSource::Auto,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomBookmark {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct BookmarksConfig {
    /// If None, `$HOME/Work` is checked at runtime.
    pub work_dir: Option<PathBuf>,
    pub custom: Vec<CustomBookmark>,
}


#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct TransferConfig {
    /// Path to `ftctl` binary. If None, it is resolved from PATH at runtime.
    pub ftctl_path: Option<PathBuf>,
}


#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RecentsConfig {
    pub max_entries: usize,
}

impl Default for RecentsConfig {
    fn default() -> Self {
        Self { max_entries: 50 }
    }
}

// ---------------------------------------------------------------------------
// Top-level Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ui: UiConfig,
    pub preview: PreviewConfig,
    pub theme: ThemeConfig,
    pub bookmarks: BookmarksConfig,
    pub transfer: TransferConfig,
    pub recents: RecentsConfig,
}

// ---------------------------------------------------------------------------
// Path expansion helpers
// ---------------------------------------------------------------------------

/// Expand a leading `~/` or `$HOME/` in a path string to the real home dir.
fn expand_path(p: PathBuf, home: &std::path::Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest)
    } else if let Some(rest) = s.strip_prefix("$HOME/") {
        home.join(rest)
    } else if s.as_ref() == "~" || s.as_ref() == "$HOME" {
        home.to_path_buf()
    } else {
        p
    }
}

/// Apply `expand_path` to every `PathBuf` inside Config that the user might
/// have written with a `~/` prefix.
fn expand_all_paths(cfg: &mut Config, home: &std::path::Path) {
    if let Some(ref p) = cfg.bookmarks.work_dir.clone() {
        cfg.bookmarks.work_dir = Some(expand_path(p.clone(), home));
    }
    for bm in &mut cfg.bookmarks.custom {
        bm.path = expand_path(bm.path.clone(), home);
    }
    if let ThemeSource::Path(ref p) = cfg.theme.source.clone() {
        cfg.theme.source = ThemeSource::Path(expand_path(p.clone(), home));
    }
    if let Some(ref p) = cfg.transfer.ftctl_path.clone() {
        cfg.transfer.ftctl_path = Some(expand_path(p.clone(), home));
    }
}

// ---------------------------------------------------------------------------
// Config serialization (for writing defaults)
// ---------------------------------------------------------------------------

/// Produce a minimal default TOML document so we can write a starter file.
fn default_toml_content() -> &'static str {
    r#"# fim configuration
# https://github.com/DevInBlack001/tui-file-manager

[ui]
show_hidden       = false
sort_key          = "name"
sort_reverse      = false
sidebar_width_pct = 18
preview_width_pct = 36

[preview]
enabled          = true
max_text_lines   = 200
max_binary_bytes = 512
image_renderer   = "auto"
video_thumbs     = true

[theme]
source = "auto"

[bookmarks]
# work_dir = "~/Work"
custom = []

[transfer]
# ftctl_path = "~/.local/bin/ftctl"

[recents]
max_entries = 50
"#
}

// ---------------------------------------------------------------------------
// load_or_default
// ---------------------------------------------------------------------------

/// Resolve the config file path using XDG_CONFIG_HOME / home fallback.
fn config_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("tui-fm").join("config.toml"))
}

/// Load configuration from disk, creating the file if absent, falling back to
/// defaults on any parse error.
pub fn load_or_default() -> Config {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

    let cfg_path = match config_file_path() {
        Some(p) => p,
        None => {
            eprintln!("[fim/config] could not determine config directory; using defaults");
            return Config::default();
        }
    };

    // File does not exist: write defaults and return default config.
    if !cfg_path.exists() {
        if let Some(parent) = cfg_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!(
                    "[fim/config] could not create config dir '{}': {e}",
                    parent.display()
                );
            }
        }
        if let Err(e) = std::fs::write(&cfg_path, default_toml_content()) {
            eprintln!(
                "[fim/config] could not write default config to '{}': {e}",
                cfg_path.display()
            );
        }
        return Config::default();
    }

    // Read file, capped at 256 KiB to avoid unbounded memory growth.
    let content = match read_capped(&cfg_path, 256 * 1024) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "[fim/config] could not read '{}': {e}; using defaults",
                cfg_path.display()
            );
            return Config::default();
        }
    };

    // Parse TOML.
    let mut cfg: Config = match toml::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[fim/config] parse error in '{}': {e}; using defaults",
                cfg_path.display()
            );
            return Config::default();
        }
    };

    // Expand ~/... and $HOME/... in path fields.
    expand_all_paths(&mut cfg, &home);

    cfg
}

/// Read a file, consuming at most `max_bytes` bytes.
fn read_capped(path: &std::path::Path, max_bytes: u64) -> Result<String, std::io::Error> {
    use std::io::Read;
    let f = std::fs::File::open(path)?;
    let mut buf = String::new();
    f.take(max_bytes).read_to_string(&mut buf)?;
    Ok(buf)
}
