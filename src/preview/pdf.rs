// src/preview/pdf.rs
// PDF text preview via pdftotext(1). Falls back to hex dump if pdftotext
// is not available.

const PDFTOTEXT_PATH: &str = "/usr/bin/pdftotext";
/// Maximum bytes read from pdftotext's stdout.
const MAX_OUTPUT_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB
/// Hex dump byte cap used when pdftotext is absent.
const HEX_FALLBACK_BYTES: usize = 512;

/// Render the first page of a PDF as plain text.
///
/// If `/usr/bin/pdftotext` is absent, falls back to a 512-byte hex dump via
/// `crate::preview::hex::render`. On success returns `Text`.
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

    if !std::path::Path::new(PDFTOTEXT_PATH).exists() {
        // Graceful fallback: show the raw bytes as a hex dump.
        return crate::preview::hex::render(path, HEX_FALLBACK_BYTES);
    }

    match run_pdftotext(path_str, max_lines) {
        Ok(lines) => crate::preview::PreviewContent::Text(lines),
        Err(msg) => crate::preview::PreviewContent::Unavailable(msg),
    }
}

/// Invoke pdftotext and collect its stdout lines (capped at 2 MiB).
fn run_pdftotext(path_str: &str, max_lines: usize) -> Result<Vec<String>, String> {
    // "-" as the output file argument tells pdftotext to write to stdout.
    let output = std::process::Command::new(PDFTOTEXT_PATH)
        .args([
            "-l", "1",       // first page only
            "-enc", "UTF-8", // force UTF-8 output
            "--",            // end of options
            path_str,
            "-",             // output to stdout
        ])
        // See preview/image.rs's chafa --probe fix: never let a subprocess
        // inherit our real controlling terminal on stdin.
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("failed to start pdftotext: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "pdftotext exited with status {} ({})",
            output.status,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("no detail")
        ));
    }

    // Cap to MAX_OUTPUT_BYTES before splitting.
    let raw_len = output.stdout.len().min(MAX_OUTPUT_BYTES as usize);
    let raw = &output.stdout[..raw_len];
    let text = String::from_utf8_lossy(raw);
    let lines: Vec<String> = text
        .split('\n')
        .take(max_lines)
        .map(|l| l.to_owned())
        .collect();
    Ok(lines)
}
