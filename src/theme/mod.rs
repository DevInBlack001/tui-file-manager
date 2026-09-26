use std::path::PathBuf;

use ratatui::style::Color;

use crate::config::ThemeSource;

/// Full colour palette used throughout the TUI.
pub struct Theme {
    pub fg: Color,
    pub fg_dim: Color,
    pub fg_bright: Color,
    pub bg: Color,
    pub bg_dark: Color,
    pub bg_darker: Color,
    pub bg_lighter: Color,
    pub selection_bg: Color,
    pub accent: Color,
    pub muted: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub cyan: Color,
    pub blue: Color,
    pub magenta: Color,
}

pub mod aether;
pub mod fallback;
pub mod pywal;

pub use fallback::fallback_theme;

/// Load a theme according to `[theme] source` from config.toml.
///
/// Auto-detection tries, in order: Omarchy's live theme, pywal's cache (a
/// ricing convention used across Arch Linux generally, not tied to Omarchy
/// or any particular desktop environment), then the static builtin palette.
/// This way a non-Omarchy Arch system still gets a real live-theme
/// integration when one is available, rather than only ever seeing the
/// builtin default.
///
/// This is called both at startup and live (on a manual refresh), so it must
/// never write to stderr: the TUI owns the alternate screen by the time a
/// live reload happens, and an unmanaged stderr write corrupts the display.
pub fn load(source: &ThemeSource) -> Theme {
    match source {
        ThemeSource::Builtin => fallback_theme(),
        ThemeSource::Path(p) => load_path(p).unwrap_or_else(fallback_theme),
        ThemeSource::Auto => {
            for candidate in omarchy_candidates() {
                if let Some(theme) = load_path(&candidate) {
                    return theme;
                }
            }
            if let Some(theme) = load_pywal_path(&pywal_candidate()) {
                return theme;
            }
            fallback_theme()
        }
    }
}

fn load_path(path: &std::path::Path) -> Option<Theme> {
    // Reject symlinks to avoid following attacker-controlled links.
    if !path.is_file() || path.is_symlink() {
        return None;
    }
    aether::load_from_file(path).ok()
}

fn load_pywal_path(path: &std::path::Path) -> Option<Theme> {
    if !path.is_file() || path.is_symlink() {
        return None;
    }
    pywal::load_from_file(path).ok()
}

/// pywal always writes its cache to the same well-known path; no env var
/// overrides this location in pywal itself, so there is nothing to read
/// before it.
fn pywal_candidate() -> PathBuf {
    let cache_dir = dirs::cache_dir().unwrap_or_else(|| {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")).join(".cache")
    });
    cache_dir.join("wal").join("colors.json")
}

/// Candidate live-theme colour files to try, in priority order.
///
/// Omarchy tracks the *currently active* theme's colours at
/// `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` - this is what
/// `omarchy theme set <name>` actually rewrites, so it's the only location
/// that reflects a live theme change. `$XDG_CONFIG_HOME/omarchy/shell.toml`
/// is kept as a secondary candidate for setups/versions that expose colour
/// fields there instead (on this project's own dev machine it only holds
/// font settings, so it never matches, but it costs nothing to try).
fn omarchy_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();

    let state_dir = dirs::state_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join(".local")
            .join("state")
    });
    v.push(
        state_dir
            .join("omarchy")
            .join("current")
            .join("theme")
            .join("colors.toml"),
    );

    if let Some(config_dir) = dirs::config_dir() {
        v.push(config_dir.join("omarchy").join("shell.toml"));
    }

    v
}
