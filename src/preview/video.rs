// src/preview/video.rs
// Video thumbnail preview via ffmpegthumbnailer(1) + chafa(1).
//
// Workflow:
//   1. Write a thumbnail PNG to a temp path under $XDG_RUNTIME_DIR.
//   2. Use a Drop guard to clean it up on any exit path.
//   3. Call image::render() on the thumbnail.

const FFTHUMB_PATH: &str = "/usr/bin/ffmpegthumbnailer";
/// Subprocess timeout in seconds.
const FFTHUMB_TIMEOUT_SECS: u64 = 15;

/// Render a video file by extracting a thumbnail frame and passing it through
/// the image renderer. Returns `Unavailable` if ffmpegthumbnailer is absent,
/// XDG_RUNTIME_DIR is unset, or the subprocess fails.
pub fn render(
    path: &std::path::Path,
    width: u16,
    height: u16,
    truecolor: bool,
    graphics: Option<&str>,
) -> crate::preview::PreviewContent {
    // TOCTOU guard.
    if path.is_symlink() {
        return crate::preview::PreviewContent::Unavailable(
            "preview not available for symbolic links".to_owned(),
        );
    }

    if !std::path::Path::new(FFTHUMB_PATH).exists() {
        return crate::preview::PreviewContent::Unavailable(
            "ffmpegthumbnailer not installed (pacman -S ffmpegthumbnailer)".to_owned(),
        );
    }

    let path_str = match path.to_str() {
        Some(s) => s,
        None => {
            return crate::preview::PreviewContent::Unavailable(
                "video path contains non-UTF-8 characters".to_owned(),
            );
        }
    };

    // Require XDG_RUNTIME_DIR for the temp file; do not fall back to /tmp
    // because that directory may be world-readable on some systems.
    let runtime_dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => d,
        _ => {
            return crate::preview::PreviewContent::Unavailable(
                "XDG_RUNTIME_DIR is not set; cannot create video thumbnail".to_owned(),
            );
        }
    };

    let random_hex = random_hex8();
    let thumb_name = format!("fim-thumb-{}.png", random_hex);
    let thumb_path = std::path::PathBuf::from(&runtime_dir).join(&thumb_name);

    // The guard removes the temp file when it is dropped, regardless of how
    // we exit this function.
    let _cleanup = TempFileGuard::new(thumb_path.clone());

    let thumb_str = match thumb_path.to_str() {
        Some(s) => s,
        None => {
            return crate::preview::PreviewContent::Unavailable(
                "thumb path contains non-UTF-8 characters".to_owned(),
            );
        }
    };

    // Scale: ffmpegthumbnailer wants pixel size; width * 8 is a reasonable
    // heuristic (each terminal cell ~8px wide).
    let size_arg = format!("{}", (width as u32) * 8);

    match run_ffthumb(path_str, thumb_str, &size_arg) {
        Ok(()) => {}
        Err(msg) => return crate::preview::PreviewContent::Unavailable(msg),
    }

    // Verify the output file was created (ffmpegthumbnailer returns 0 on some
    // errors without writing anything).
    if !thumb_path.exists() {
        return crate::preview::PreviewContent::Unavailable(
            "ffmpegthumbnailer did not produce a thumbnail (unsupported format?)".to_owned(),
        );
    }

    // The thumb file is a real file, not a symlink we created; safe to pass
    // to image::render which does its own symlink check.
    crate::preview::image::render(&thumb_path, width, height, truecolor, graphics)
}

/// Spawn ffmpegthumbnailer and wait up to FFTHUMB_TIMEOUT_SECS.
fn run_ffthumb(path_str: &str, thumb_str: &str, size_arg: &str) -> Result<(), String> {
    let mut child = std::process::Command::new(FFTHUMB_PATH)
        .args([
            "-i", path_str,
            "-o", thumb_str,
            "-s", size_arg,
            "-t", "10%",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to start ffmpegthumbnailer: {}", e))?;

    let timeout = std::time::Duration::from_secs(FFTHUMB_TIMEOUT_SECS);
    let exited = wait_with_timeout(&mut child, timeout);
    if !exited {
        let _ = child.kill();
        return Err("ffmpegthumbnailer timed out (>15 s)".to_owned());
    }

    match child.wait() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "ffmpegthumbnailer exited with status {}",
            status
        )),
        Err(e) => Err(format!("ffmpegthumbnailer wait error: {}", e)),
    }
}

/// Poll a child process for up to `timeout`, returning true if it exited.
fn wait_with_timeout(child: &mut std::process::Child, timeout: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(_) => return false,
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Generate 8 pseudorandom hex characters derived from system time nanoseconds.
/// This is not cryptographically random, but is sufficient for a temp-file name
/// that lives only for the duration of this function call.
fn random_hex8() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // Mix in the thread ID's low bits for extra uniqueness when multiple
    // preview threads might run concurrently in future.
    let mixed = nanos ^ (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(1));
    format!("{:08x}", mixed)
}

/// RAII guard that removes a temp file when dropped.
struct TempFileGuard {
    path: std::path::PathBuf,
}

impl TempFileGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        // Best-effort removal; ignore errors (the file might never have been
        // created if ffmpegthumbnailer failed before writing).
        let _ = std::fs::remove_file(&self.path);
    }
}
