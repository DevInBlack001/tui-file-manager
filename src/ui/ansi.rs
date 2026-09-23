// ui/ansi.rs - Minimal ANSI SGR (Select Graphic Rendition) parser.
//
// chafa and bat emit real colour via ANSI escape codes (truecolor, 256-color,
// and the basic 8/16-color set). Rendering that as plain text would either
// leak raw escape bytes onto the screen or - if stripped - collapse a
// colourful chafa render into flat monochrome block characters. This parses
// just enough of SGR to carry real per-segment colour into ratatui spans.
//
// Any other escape sequence - CSI (cursor movement etc.), OSC, DCS/APC/PM/SOS
// (e.g. raw Sixel graphics data, which chafa can emit if its output format
// isn't pinned) - is consumed and dropped whole, payload included. Letting a
// sequence's *payload* fall through as literal text (as a naive "strip ESC
// [...]m only" parser would) turns e.g. a Sixel image into a flood of
// numbers/punctuation on screen, so every recognized sequence kind is fully
// consumed to its own terminator, never just its opening bytes.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Parse one line of ANSI-coloured text into a styled ratatui `Line`.
pub fn parse_line(input: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut current = String::new();

    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next(); // consume '['
                    let mut params = String::new();
                    let mut final_byte = None;
                    for c2 in chars.by_ref() {
                        if c2.is_ascii_alphabetic() || c2 == '@' || c2 == '~' {
                            final_byte = Some(c2);
                            break;
                        }
                        params.push(c2);
                    }
                    if final_byte == Some('m') {
                        if !current.is_empty() {
                            spans.push(Span::styled(std::mem::take(&mut current), style));
                        }
                        style = apply_sgr(style, &params);
                    }
                    // Any other final byte (cursor movement, etc.) is
                    // dropped silently - we only care about colour/style.
                }
                Some(']') => {
                    // OSC: consume until BEL or ST (ESC \).
                    chars.next();
                    skip_string_terminated(&mut chars, true);
                }
                Some('P') | Some('X') | Some('^') | Some('_') => {
                    // DCS / SOS / PM / APC (e.g. Sixel graphics data):
                    // consume the whole payload until ST (ESC \).
                    chars.next();
                    skip_string_terminated(&mut chars, false);
                }
                Some(_) => {
                    // A bare two-character escape (ESC 7, ESC c, ...).
                    chars.next();
                }
                None => {}
            }
            continue;
        }
        // Drop other C0 control characters (e.g. \r) that would otherwise
        // confuse ratatui's rendering; keep \t and printable text.
        if c.is_control() && c != '\t' {
            continue;
        }
        current.push(c);
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, style));
    }
    if spans.is_empty() {
        spans.push(Span::raw(""));
    }
    Line::from(spans)
}

/// Consume characters up to and including a string terminator: `ESC \`, or
/// (when `allow_bel`) a bare BEL. Used for OSC/DCS/APC/PM/SOS payloads.
/// If the terminator is never found (truncated input), this consumes the
/// rest of the iterator and returns - it never panics or infinite-loops.
fn skip_string_terminated(chars: &mut std::iter::Peekable<std::str::Chars>, allow_bel: bool) {
    while let Some(c) = chars.next() {
        if allow_bel && c == '\u{7}' {
            return;
        }
        if c == '\u{1b}' && chars.peek() == Some(&'\\') {
            chars.next();
            return;
        }
    }
}

fn apply_sgr(mut style: Style, params: &str) -> Style {
    let codes: Vec<i32> = params
        .split(';')
        .map(|p| p.parse::<i32>().unwrap_or(0))
        .collect();
    let codes = if codes.is_empty() { vec![0] } else { codes };

    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            22 => style = style.remove_modifier(Modifier::BOLD).remove_modifier(Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            30..=37 => style = style.fg(basic_color((codes[i] - 30) as u8)),
            38 => {
                if let Some((color, consumed)) = extended_color(&codes[i + 1..]) {
                    style = style.fg(color);
                    i += consumed;
                }
            }
            39 => style = style.fg(Color::Reset),
            40..=47 => style = style.bg(basic_color((codes[i] - 40) as u8)),
            48 => {
                if let Some((color, consumed)) = extended_color(&codes[i + 1..]) {
                    style = style.bg(color);
                    i += consumed;
                }
            }
            49 => style = style.bg(Color::Reset),
            90..=97 => style = style.fg(bright_color((codes[i] - 90) as u8)),
            100..=107 => style = style.bg(bright_color((codes[i] - 100) as u8)),
            _ => {}
        }
        i += 1;
    }
    style
}

/// Parse the parameters following a `38` or `48` code: either
/// `5;<index>` (256-color) or `2;<r>;<g>;<b>` (truecolor).
/// Returns the color and how many extra codes were consumed.
fn extended_color(rest: &[i32]) -> Option<(Color, usize)> {
    match rest.first() {
        Some(5) => rest.get(1).map(|idx| (Color::Indexed(*idx as u8), 2)),
        Some(2) => {
            if rest.len() >= 4 {
                Some((Color::Rgb(rest[1] as u8, rest[2] as u8, rest[3] as u8), 4))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn basic_color(n: u8) -> Color {
    match n {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        _ => Color::Gray,
    }
}

fn bright_color(n: u8) -> Color {
    match n {
        0 => Color::DarkGray,
        1 => Color::LightRed,
        2 => Color::LightGreen,
        3 => Color::LightYellow,
        4 => Color::LightBlue,
        5 => Color::LightMagenta,
        6 => Color::LightCyan,
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truecolor_fg_bg() {
        let line = parse_line("\x1b[38;2;255;0;0;48;2;0;255;0mhi\x1b[0m");
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "hi");
        assert_eq!(line.spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(line.spans[0].style.bg, Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn indexed_256_color() {
        let line = parse_line("\x1b[38;5;196mred\x1b[0m plain");
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].style.fg, Some(Color::Indexed(196)));
        assert_eq!(line.spans[1].style.fg, None);
        assert_eq!(line.spans[1].content, " plain");
    }

    #[test]
    fn basic_and_bright_colors() {
        let line = parse_line("\x1b[31mred\x1b[0m\x1b[92mgreen");
        assert_eq!(line.spans[0].style.fg, Some(Color::Red));
        assert_eq!(line.spans[1].style.fg, Some(Color::LightGreen));
    }

    #[test]
    fn bold_modifier_and_reset() {
        let line = parse_line("\x1b[1mbold\x1b[22m normal");
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert!(!line.spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn non_sgr_csi_is_dropped_without_corrupting_text() {
        // Cursor-movement CSI (not SGR) must be consumed, not leaked as text.
        let line = parse_line("a\x1b[2Kb");
        let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "ab");
    }

    #[test]
    fn malformed_escape_does_not_panic() {
        // Unterminated CSI at end of string must not panic or infinite-loop.
        let _ = parse_line("abc\x1b[38;2;1");
    }

    #[test]
    fn sixel_dcs_payload_is_fully_dropped_not_leaked_as_text() {
        // A Sixel image (DCS ... ST) must never leak its numeric payload as
        // literal text - that's exactly what garbled the preview pane when
        // chafa's format auto-detection picked sixels over a piped stdout.
        let line = parse_line("before\x1bP0;1;0q\"1;1;320;198#0;2;3;2;2#1;2;1\x1b\\after");
        let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "beforeafter");
    }

    #[test]
    fn osc_payload_is_dropped_bel_terminated() {
        let line = parse_line("a\x1b]0;window title\x07b");
        let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "ab");
    }

    #[test]
    fn unterminated_dcs_does_not_panic_or_hang() {
        let _ = parse_line("before\x1bP0;1;0qunterminated-sixel-data");
    }
}
