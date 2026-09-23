use std::path::Path;
use std::process::Command;

/// Detect the MIME type of `path`.
///
/// Stage 1 (fast): extension lookup via `mime_guess`.
/// Stage 2 (slow, only if stage 1 yields `application/octet-stream`):
///   invoke `/usr/bin/file` or `/bin/file` directly (no shell).
pub fn detect(path: &Path) -> String {
    let fast = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    if fast != "application/octet-stream" {
        return fast;
    }

    // Stage 2: try the system `file` binary.
    let file_bin = if Path::new("/usr/bin/file").exists() {
        "/usr/bin/file"
    } else if Path::new("/bin/file").exists() {
        "/bin/file"
    } else {
        return fast; // binary not available, keep octet-stream
    };

    // Obtain a lossless string representation of the path for argv.
    // We use the raw OsStr to avoid invalid-UTF-8 truncation.
    let output = Command::new(file_bin)
        .args(["--mime-type", "--brief", "--"])
        .arg(path) // Path implements AsRef<OsStr>; no shell expansion
        // Never let a subprocess inherit our real controlling terminal on
        // stdin (see preview/image.rs's chafa --probe fix for why).
        .stdin(std::process::Stdio::null())
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let mime = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if mime.is_empty() { fast } else { mime }
        }
        _ => fast,
    }
}

/// Returns true for text/* types and well-known plain-text subtypes.
pub fn is_text(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime == "application/json"
        || mime == "application/xml"
        || mime == "application/javascript"
        || mime == "application/x-sh"
        || mime == "application/x-shellscript"
        || mime == "application/toml"
        || mime == "application/x-yaml"
        || mime == "application/yaml"
}

/// Returns true for image/* MIME types.
pub fn is_image(mime: &str) -> bool {
    mime.starts_with("image/")
}

/// Returns true for video/* MIME types.
pub fn is_video(mime: &str) -> bool {
    mime.starts_with("video/")
}

/// Returns true for application/pdf.
pub fn is_pdf(mime: &str) -> bool {
    mime == "application/pdf"
}

/// Returns true for MIME types that are not text, image, video, or PDF.
pub fn is_binary(mime: &str) -> bool {
    !is_text(mime) && !is_image(mime) && !is_video(mime) && !is_pdf(mime)
}
