// src/preview/text.rs
// Text preview renderer. Uses bat(1) for syntax-highlighted ANSI output
// when available; falls back to plain BufReader line reading.

use std::io::{BufRead, ErrorKind};

const BAT_PATH: &str = "/usr/bin/bat";
/// Maximum bytes read from bat's stdout to prevent memory exhaustion.
const MAX_BAT_OUTPUT_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB

/// Render a text file preview.
///
/// * If `/usr/bin/bat` is present, call it with ANSI color and return
///   `AnsiText`. The output is capped at 2 MiB.
/// * Otherwise, read the file with `BufReader` and return `Text`.
/// * On any error, distinguish PermissionDenied from other causes.
pub fn render(path: &std::path::Path, max_lines: usize) -> crate::preview::PreviewContent {
    // TOCTOU guard.
    if path.is_symlink() {
        return crate::preview::PreviewContent::Unavailable(
            "preview not available for symbolic links".to_owned(),
        );
    }

    let path_str = match path.to_str() {
        Some(s) => s,
        None => {
            return crate::preview::PreviewContent::Unavailable(
                "path contains non-UTF-8 characters".to_owned(),
            );
        }
    };

    // Try bat first.
    if std::path::Path::new(BAT_PATH).exists() {
        match run_bat(path_str, max_lines) {
            Ok(lines) => return crate::preview::PreviewContent::AnsiText(lines),
            Err(_) => {
                // bat failed; fall through to plain reader.
            }
        }
    }

    // Plain fallback.
    plain_read(path, max_lines)
}

/// Call bat and return its stdout lines (capped at 2 MiB, then max_lines).
fn run_bat(path_str: &str, max_lines: usize) -> Result<Vec<String>, ()> {
    let output = std::process::Command::new(BAT_PATH)
        .args([
            "--color=always",
            "--paging=never",
            "--style=plain",
            "--line-range",
            &format!("1:{}", max_lines),
            "--",
            path_str,
        ])
        // Never let bat inherit our real controlling terminal: with a bare
        // .output(), stdin defaults to inherited, and any subprocess that
        // probes/queries the terminal on stdin races our own key-event
        // reader (see the chafa --probe fix in preview/image.rs).
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| ())?;

    if !output.status.success() {
        return Err(());
    }

    // Cap stdout at MAX_BAT_OUTPUT_BYTES before decoding.
    let cap = MAX_BAT_OUTPUT_BYTES as usize;
    let raw: &[u8] = if output.stdout.len() > cap {
        &output.stdout[..cap]
    } else {
        &output.stdout
    };

    // Lossy UTF-8 conversion to avoid panic on exotic byte sequences.
    let text = String::from_utf8_lossy(&raw);
    let lines: Vec<String> = text
        .split('\n')
        .take(max_lines)
        .map(|l| l.to_owned())
        .collect();
    Ok(lines)
}

/// Plain BufReader fallback: read at most max_lines lines.
fn plain_read(path: &std::path::Path, max_lines: usize) -> crate::preview::PreviewContent {
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

    let reader = std::io::BufReader::new(file);
    let mut lines = Vec::with_capacity(max_lines.min(256));
    for line_result in reader.lines().take(max_lines) {
        match line_result {
            Ok(line) => lines.push(line),
            Err(e) => {
                let msg = if e.kind() == ErrorKind::PermissionDenied {
                    format!("permission denied while reading '{}': {}", path.display(), e)
                } else {
                    format!("read error on '{}': {}", path.display(), e)
                };
                return crate::preview::PreviewContent::Unavailable(msg);
            }
        }
    }

    crate::preview::PreviewContent::Text(lines)
}
