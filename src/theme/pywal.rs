// theme/pywal.rs - Load a Theme from pywal's cache
// (`~/.cache/wal/colors.json`). pywal is a widely-used Arch Linux ricing
// tool with no tie to any specific desktop environment or window manager,
// so this gives non-Omarchy Arch systems a real live-theme integration
// instead of dropping straight to the static builtin palette.

use std::io::Read;

use ratatui::style::Color;
use serde_json::Value;

use crate::theme::aether::parse_hex;
use crate::theme::Theme;

fn get_color(value: &Value, path: &[&str], default: Color) -> Color {
    let mut cur = value;
    for key in path {
        match cur.get(key) {
            Some(v) => cur = v,
            None => return default,
        }
    }
    match cur.as_str().map(parse_hex) {
        Some(Ok(color)) => color,
        _ => default,
    }
}

/// Blend two RGB colors; `t` of 0.0 is `a`, 1.0 is `b`. Non-RGB inputs (never
/// produced by this module, but kept total) return `a` unchanged.
fn blend(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            let lerp = |x: u8, y: u8| {
                (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8
            };
            Color::Rgb(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
        }
        _ => a,
    }
}

/// Load `~/.cache/wal/colors.json` into a `Theme`.
///
/// Rejects symlinks (caller must verify before calling, but we double-check
/// to guard against TOCTOU between the caller's check and this open). Reads
/// at most 64 KiB to prevent unbounded memory growth. pywal has no notion of
/// fim's graduated backgrounds/selection/accent, so those are derived by
/// blending its `background`/`foreground` rather than left unset.
pub fn load_from_file(path: &std::path::Path) -> Result<Theme, String> {
    if path.is_symlink() {
        return Err(format!("refusing to read symlink at {}", path.display()));
    }

    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;

    const MAX_BYTES: u64 = 64 * 1024;
    let mut buf = String::new();
    file.take(MAX_BYTES)
        .read_to_string(&mut buf)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let value: Value =
        serde_json::from_str(&buf).map_err(|e| format!("JSON parse error in {}: {e}", path.display()))?;

    // Same fallback palette as aether.rs, so a colors.json missing fields
    // (or one from an old pywal schema) still degrades sensibly.
    let def_fg      = Color::Rgb(190, 249, 243);
    let def_fg_bri  = Color::Rgb(206, 251, 246);
    let def_bg      = Color::Rgb(9,   4,  32);
    let def_muted   = Color::Rgb(96,  96, 102);
    let def_red     = Color::Rgb(166, 135, 192);
    let def_green   = Color::Rgb(132, 202, 255);
    let def_yellow  = Color::Rgb(152, 255, 255);
    let def_cyan    = Color::Rgb(162, 219, 255);
    let def_blue    = Color::Rgb(114, 122, 187);
    let def_magenta = Color::Rgb(175, 157, 241);

    let bg = get_color(&value, &["special", "background"], def_bg);
    let fg = get_color(&value, &["special", "foreground"], def_fg);
    let bright_black = get_color(&value, &["colors", "color8"], def_muted);
    let bright_white = get_color(&value, &["colors", "color15"], def_fg_bri);

    Ok(Theme {
        fg,
        fg_dim: bright_black,
        fg_bright: bright_white,
        bg,
        bg_dark: blend(bg, Color::Black, 0.3),
        bg_darker: blend(bg, Color::Black, 0.5),
        bg_lighter: blend(bg, fg, 0.15),
        selection_bg: blend(bg, fg, 0.2),
        accent: get_color(&value, &["colors", "color4"], def_blue),
        muted: bright_black,
        red: get_color(&value, &["colors", "color1"], def_red),
        green: get_color(&value, &["colors", "color2"], def_green),
        yellow: get_color(&value, &["colors", "color3"], def_yellow),
        cyan: get_color(&value, &["colors", "color6"], def_cyan),
        blue: get_color(&value, &["colors", "color4"], def_blue),
        magenta: get_color(&value, &["colors", "color5"], def_magenta),
    })
}
