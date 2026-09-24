// src/preview/hex.rs
// Built-in hex dump renderer. No external tools required.

use std::io::{ErrorKind, Read};

/// Render up to `max_bytes` of `path` as a hex dump.
///
/// Format (classic xxd-style):
///   00000000  41 42 43 44 45 46 47 48  49 4a 4b 4c 4d 4e 4f 50  |ABCDEFGHIJKLMNOP|
///
/// Bytes are grouped in two blocks of 8, separated by a double space.
/// The ASCII sidebar shows printable characters or `.` for non-printable.
pub fn render(path: &std::path::Path, max_bytes: usize) -> crate::preview::PreviewContent {
    // TOCTOU guard: refuse to open symlinks.
    if path.is_symlink() {
        return crate::preview::PreviewContent::Unavailable(
            "preview not available for symbolic links".to_owned(),
        );
    }

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            let msg = if e.kind() == ErrorKind::PermissionDenied {
                format!("permission denied reading '{}'", path.display())
            } else {
                format!("cannot open '{}': {}", path.display(), e)
            };
            return crate::preview::PreviewContent::Unavailable(msg);
        }
    };

    let mut buf = Vec::with_capacity(max_bytes.min(65536));
    match file.take(max_bytes as u64).read_to_end(&mut buf) {
        Ok(_) => {}
        Err(e) => {
            let msg = if e.kind() == ErrorKind::PermissionDenied {
                format!("permission denied reading '{}': {}", path.display(), e)
            } else {
                format!("read error on '{}': {}", path.display(), e)
            };
            return crate::preview::PreviewContent::Unavailable(msg);
        }
    }

    if buf.is_empty() {
        return crate::preview::PreviewContent::Text(vec!["(empty file)".to_owned()]);
    }

    let lines = format_hex(&buf);
    crate::preview::PreviewContent::HexDump(lines)
}

/// Format a byte slice as hex dump lines (16 bytes per line).
fn format_hex(buf: &[u8]) -> Vec<String> {
    let mut lines = Vec::with_capacity(buf.len().div_ceil(16));
    let mut offset = 0usize;

    for chunk in buf.chunks(16) {
        // Hex section: two groups of 8, double-space between them.
        let mut hex_part = String::with_capacity(49);
        for (i, byte) in chunk.iter().enumerate() {
            if i == 8 {
                hex_part.push(' '); // extra space between the two groups
            }
            if i > 0 && i != 8 {
                hex_part.push(' ');
            } else if i == 8 {
                hex_part.push(' '); // space already pushed; now the byte
            }
            hex_part.push_str(&format!("{:02x}", byte));
        }

        // Pad to full width when the last chunk is short.
        let full_hex_width = 48; // "xx xx xx xx xx xx xx xx  xx xx xx xx xx xx xx xx"
        while hex_part.len() < full_hex_width {
            hex_part.push(' ');
        }

        // ASCII sidebar.
        let ascii_part: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..=0x7e).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();

        lines.push(format!(
            "{:08x}  {}  |{}|",
            offset, hex_part, ascii_part
        ));
        offset += chunk.len();
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hex_basic() {
        let data: Vec<u8> = (0x41u8..=0x50u8).collect(); // A-P
        let lines = format_hex(&data);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("41 42 43 44 45 46 47 48  49 4a 4b 4c 4d 4e 4f 50"));
        assert!(lines[0].contains("|ABCDEFGHIJKLMNOP|"));
        assert!(lines[0].starts_with("00000000"));
    }

    #[test]
    fn test_format_hex_partial_last_line() {
        let data = b"Hello";
        let lines = format_hex(data);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("|Hello|"));
    }

    #[test]
    fn test_format_hex_non_printable() {
        let data = [0x00u8, 0x01, 0x7f, 0x41];
        let lines = format_hex(&data);
        assert!(lines[0].contains("|...A|"));
    }

    #[test]
    fn test_format_hex_two_lines() {
        let data: Vec<u8> = (0u8..17).collect();
        let lines = format_hex(&data);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].starts_with("00000010"));
    }
}
