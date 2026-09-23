// src/preview/image.rs
// Image preview via chafa(1). Renders into ANSI/UTF-8 block art for the TUI.

const CHAFA_PATH: &str = "/usr/bin/chafa";
/// Maximum bytes read from chafa's stdout.
const MAX_CHAFA_OUTPUT_BYTES: u64 = 4 * 1024 * 1024; // 4 MiB
/// Subprocess timeout in seconds.
const CHAFA_TIMEOUT_SECS: u64 = 10;

/// Render an image using chafa.
///
/// `width` and `height` are the terminal character dimensions of the panel.
/// Returns `ChafaLines` on success, `Unavailable` if chafa is not installed
/// or the call fails.
pub fn render(
    path: &std::path::Path,
    width: u16,
    height: u16,
    truecolor: bool,
) -> crate::preview::PreviewContent {
    // TOCTOU guard.
    if path.is_symlink() {
        return crate::preview::PreviewContent::Unavailable(
            "preview not available for symbolic links".to_owned(),
        );
    }

    if !std::path::Path::new(CHAFA_PATH).exists() {
        return crate::preview::PreviewContent::Unavailable(
            "chafa not installed - run install.sh to add it".to_owned(),
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

    let size_arg = format!("{}x{}", width, height);
    let colors_arg = if truecolor { "full" } else { "256" };

    match run_chafa(path_str, &size_arg, colors_arg) {
        Ok(lines) => crate::preview::PreviewContent::ChafaLines(lines),
        Err(msg) => crate::preview::PreviewContent::Unavailable(msg),
    }
}

/// Spawn chafa with a 10-second timeout and return its output lines.
fn run_chafa(
    path_str: &str,
    size_arg: &str,
    colors_arg: &str,
) -> Result<Vec<String>, String> {
    use std::io::Read;
    use std::time::{Duration, Instant};

    let mut child = std::process::Command::new(CHAFA_PATH)
        .args([
            // Force character-art output. chafa's format auto-detection can
            // pick sixels/kitty and emit raw DCS graphics escape sequences,
            // which render as garbage text once captured as a string.
            "--format",
            "symbols",
            // chafa defaults to probing the terminal (querying it and
            // waiting up to 5s for a response, e.g. an OSC 10/11 colour
            // query) to refine its auto-detection. It inherits our stdin,
            // which is the real controlling terminal, so that query and its
            // response race directly against our own key-event reader -
            // confirmed to leak raw `rgb:RRRR/GGGG/BBBB` response bytes into
            // whatever text prompt happened to be focused. Disabled: we
            // already pin --format/--colors explicitly, so nothing chafa
            // could learn from probing changes its output here.
            "--probe",
            "off",
            "--size",
            size_arg,
            "--colors",
            colors_arg,
            "--",
            path_str,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to start chafa: {}", e))?;

    let deadline = Instant::now() + Duration::from_secs(CHAFA_TIMEOUT_SECS);

    // Read stdout (bounded).
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "chafa: no stdout handle".to_owned())?;
    let mut buf = Vec::with_capacity(65536);
    let mut limited = (&mut stdout).take(MAX_CHAFA_OUTPUT_BYTES);
    let read_result = limited.read_to_end(&mut buf);

    // Wait for the process to finish, respecting the timeout.
    let wait_timeout = deadline.saturating_duration_since(Instant::now())
        .max(Duration::from_millis(50));

    let exited = wait_with_timeout(&mut child, wait_timeout);
    if !exited {
        let _ = child.kill();
    }

    read_result.map_err(|e| format!("chafa: read error: {}", e))?;

    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<String> = text.split('\n').map(|l| l.to_owned()).collect();
    Ok(lines)
}

/// Poll a child process for up to `timeout`, returning true if it exited.
/// This avoids blocking the preview thread indefinitely.
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

/// Detect whether the current terminal supports 24-bit (truecolor) rendering.
///
/// Checks `$COLORTERM` for the values `truecolor` or `24bit` (case-insensitive).
pub fn detect_truecolor() -> bool {
    match std::env::var("COLORTERM") {
        Ok(val) => {
            let v = val.to_lowercase();
            v == "truecolor" || v == "24bit"
        }
        Err(_) => false,
    }
}
