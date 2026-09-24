// preview/glyph.rs - Hand-drawn ASCII glyphs for well-known binary container
// formats (disc images, archives, torrents, office documents), shown instead
// of a generic hex dump.
//
// Style matches this machine's Omarchy screensaver branding
// (~/.config/omarchy/branding/screensaver.txt): thin-line block letters
// built from `/ \ _ |`, not solid Unicode block characters. Each glyph mixes
// multiple colour roles across its letters rather than one flat style, so it
// reads as a small picture instead of a monochrome wall of text.
// `ui::preview` resolves `GlyphColor` to the live theme's actual colours at
// render time.

use super::{GlyphColor, GlyphLine};

fn line(segments: &[(&str, GlyphColor)]) -> GlyphLine {
    segments.iter().map(|(s, c)| (s.to_string(), *c)).collect()
}

/// Block-letter "ISO", for disc image files (`.iso`, `.img`, `.bin`, `.nrg`).
pub fn disc() -> Vec<GlyphLine> {
    use GlyphColor::*;
    vec![
        line(&[("_____ ", Cyan), ("____   ", Accent), ("___  ", Yellow)]),
        line(&[("|_ _|", Cyan), ("/ ___| ", Accent), ("/ _ \\ ", Yellow)]),
        line(&[(" | | ", Cyan), ("\\___ \\", Accent), ("| | | |", Yellow)]),
        line(&[(" | |  ", Cyan), ("___) |", Accent), ("| |_| |", Yellow)]),
        line(&[("|___|", Cyan), ("|____/ ", Accent), ("\\___/ ", Yellow)]),
        line(&[("", Fg)]),
        line(&[("        DISC IMAGE", Muted)]),
    ]
}

/// Block-letter "ZIP", for archive files.
pub fn archive() -> Vec<GlyphLine> {
    use GlyphColor::*;
    vec![
        line(&[(" _____ ", Yellow), ("___ ", Accent), ("____  ", Cyan)]),
        line(&[("|__  / ", Yellow), ("|_ _|", Accent), ("|  _ \\ ", Cyan)]),
        line(&[("  / /  ", Yellow), (" | | ", Accent), ("| |_) |", Cyan)]),
        line(&[(" / /_  ", Yellow), (" | | ", Accent), ("|  __/ ", Cyan)]),
        line(&[("/____| ", Yellow), ("|___|", Accent), ("|_|    ", Cyan)]),
        line(&[("", Fg)]),
        line(&[("         ARCHIVE", Muted)]),
    ]
}

/// Block-letter "DOC", for OpenDocument/Microsoft Office document formats
/// (`.ods`/`.odt`/`.odp` and their `.xlsx`/`.docx`/`.pptx` equivalents).
pub fn document() -> Vec<GlyphLine> {
    use GlyphColor::*;
    vec![
        line(&[(" ____   ", Accent), ("___  ", Fg), ("____ ", Cyan)]),
        line(&[("|  _ \\ ", Accent), ("/ _ \\ ", Fg), ("/ ___|", Cyan)]),
        line(&[("| | | |", Accent), ("| | | | ", Fg), ("|    ", Cyan)]),
        line(&[("| |_| | ", Accent), ("|_| | ", Fg), ("|___ ", Cyan)]),
        line(&[("|____/  ", Accent), ("\\___/  ", Fg), ("\\____|", Cyan)]),
        line(&[("", Fg)]),
        line(&[("        DOCUMENT", Muted)]),
    ]
}

/// Block-letter "P2P", for `.torrent` files.
pub fn torrent() -> Vec<GlyphLine> {
    use GlyphColor::*;
    vec![
        line(&[(" ____    ", Accent), ("____    ", FgDim), ("____  ", Accent)]),
        line(&[("|  _ \\  ", Accent), ("|___ \\  ", FgDim), ("|  _ \\ ", Accent)]),
        line(&[("| |_) |   ", Accent), ("__) | ", FgDim), ("| |_) |", Accent)]),
        line(&[("|  __/   ", Accent), ("/ __/  ", FgDim), ("|  __/ ", Accent)]),
        line(&[("|_|     ", Accent), ("|_____| ", FgDim), ("|_|    ", Accent)]),
        line(&[("", Fg)]),
        line(&[("        TORRENT", Muted)]),
    ]
}
