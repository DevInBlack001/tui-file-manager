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

pub use fallback::fallback_theme;

/// Load a theme according to `[theme] source` from config.toml.
///
/// This is called both at startup and live (on a manual refresh), so it must
/// never write to stderr: the TUI owns the alternate screen by the time a
/// live reload happens, and an unmanaged stderr write corrupts the display.
pub fn load(source: &ThemeSource) -> Theme {
    match source {
        ThemeSource::Builtin => fallback_theme(),
        ThemeSource::Path(p) => load_path(p).unwrap_or_else(fallback_theme),
        ThemeSource::Auto => {
            for candidate in auto_candidates() {
                if let Some(theme) = load_path(&candidate) {
                    return theme;
                }
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

/// Candidate live-theme colour files to try, in priority order.
///
/// Omarchy tracks the *currently active* theme's colours at
/// `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` - this is what
/// `omarchy theme set <name>` actually rewrites, so it's the only location
/// that reflects a live theme change. `$XDG_CONFIG_HOME/omarchy/shell.toml`
/// is kept as a secondary candidate for setups/versions that expose colour
/// fields there instead (on this project's own dev machine it only holds
/// font settings, so it never matches, but it costs nothing to try).
fn auto_candidates() -> Vec<PathBuf> {
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
