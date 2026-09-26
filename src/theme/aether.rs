use crate::theme::Theme;
use ratatui::style::Color;
use std::io::Read;

/// Parse a `#RRGGBB` hex string into a ratatui `Color::Rgb`.
pub(crate) fn parse_hex(s: &str) -> Result<Color, String> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return Err(format!("expected 6 hex digits, got '{s}'"));
    }
    let r = u8::from_str_radix(&s[0..2], 16)
        .map_err(|e| format!("bad red component in '{s}': {e}"))?;
    let g = u8::from_str_radix(&s[2..4], 16)
        .map_err(|e| format!("bad green component in '{s}': {e}"))?;
    let b = u8::from_str_radix(&s[4..6], 16)
        .map_err(|e| format!("bad blue component in '{s}': {e}"))?;
    Ok(Color::Rgb(r, g, b))
}

/// Pull a colour field from a parsed TOML table, falling back to `default`
/// silently if the key is absent or unparseable.
///
/// Never logs to stderr: this runs both at startup and live (on a manual
/// theme refresh), and the TUI owns the alternate screen by the time a live
/// reload happens - an unmanaged stderr write corrupts the display.
fn get_color(table: &toml::Value, key: &str, default: Color) -> Color {
    match table.get(key).and_then(|v| v.as_str()) {
        Some(hex) => parse_hex(hex).unwrap_or(default),
        None => default,
    }
}

/// Load an Omarchy shell.toml into a `Theme`.
///
/// Rejects symlinks (caller must verify before calling, but we double-check).
/// Reads at most 64 KiB to prevent unbounded memory growth.
pub fn load_from_file(path: &std::path::Path) -> Result<Theme, String> {
    // Double-check: reject symlinks even if the caller already checked,
    // to guard against TOCTOU between the caller's check and this open.
    if path.is_symlink() {
        return Err(format!("refusing to read symlink at {}", path.display()));
    }

    let file = std::fs::File::open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;

    const MAX_BYTES: u64 = 64 * 1024;
    let mut buf = String::new();
    file.take(MAX_BYTES)
        .read_to_string(&mut buf)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let value: toml::Value = toml::from_str(&buf)
        .map_err(|e| format!("TOML parse error in {}: {e}", path.display()))?;

    // `get_color` falls back to a hardcoded default for any single missing
    // key, which is right for a theme file that's *mostly* complete - but it
    // means a file with none of these keys at all (e.g. Omarchy's
    // shell.toml, which on some installs holds only font settings) would
    // otherwise still return `Ok` full of nothing but defaults, and the
    // auto-detection candidate chain in theme/mod.rs would wrongly treat
    // that as "a real theme was found" and never try the next candidate
    // (pywal, then the builtin palette). Require at least one recognized key
    // before accepting this file as a theme source.
    const KEYS: &[&str] = &[
        "foreground", "dark_foreground", "bright_foreground", "background",
        "dark_background", "darker_background", "lighter_background", "selection",
        "accent", "muted", "red", "green", "yellow", "cyan", "blue", "magenta",
    ];
    if !KEYS.iter().any(|k| value.get(k).is_some()) {
        return Err(format!("no recognized color keys in {}", path.display()));
    }

    // Sensible fallback colours (match fallback_theme palette).
    let def_fg       = Color::Rgb(190, 249, 243);
    let def_fg_dim   = Color::Rgb(143, 187, 182);
    let def_fg_bri   = Color::Rgb(206, 251, 246);
    let def_bg       = Color::Rgb(9,   4,  32);
    let def_bg_dark  = Color::Rgb(7,   3,  24);
    let def_bg_dkr   = Color::Rgb(5,   2,  16);
    let def_bg_ltr   = Color::Rgb(34,  29, 54);
    let def_sel      = Color::Rgb(34,  29, 54);
    let def_accent   = Color::Rgb(114, 122, 187);
    let def_muted    = Color::Rgb(96,  96, 102);
    let def_red      = Color::Rgb(166, 135, 192);
    let def_green    = Color::Rgb(132, 202, 255);
    let def_yellow   = Color::Rgb(152, 255, 255);
    let def_cyan     = Color::Rgb(162, 219, 255);
    let def_blue     = Color::Rgb(114, 122, 187);
    let def_magenta  = Color::Rgb(175, 157, 241);

    Ok(Theme {
        fg:           get_color(&value, "foreground",        def_fg),
        fg_dim:       get_color(&value, "dark_foreground",   def_fg_dim),
        fg_bright:    get_color(&value, "bright_foreground", def_fg_bri),
        bg:           get_color(&value, "background",        def_bg),
        bg_dark:      get_color(&value, "dark_background",   def_bg_dark),
        bg_darker:    get_color(&value, "darker_background", def_bg_dkr),
        bg_lighter:   get_color(&value, "lighter_background",def_bg_ltr),
        selection_bg: get_color(&value, "selection",         def_sel),
        accent:       get_color(&value, "accent",            def_accent),
        muted:        get_color(&value, "muted",             def_muted),
        red:          get_color(&value, "red",               def_red),
        green:        get_color(&value, "green",             def_green),
        yellow:       get_color(&value, "yellow",            def_yellow),
        cyan:         get_color(&value, "cyan",              def_cyan),
        blue:         get_color(&value, "blue",              def_blue),
        magenta:      get_color(&value, "magenta",           def_magenta),
    })
}
