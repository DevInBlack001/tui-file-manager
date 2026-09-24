// src/preview/image.rs
// Image preview via chafa(1): either real terminal graphics (Kitty graphics
// protocol) when the terminal is known to support it, or character-art
// otherwise.

const CHAFA_PATH: &str = "/usr/bin/chafa";
/// Maximum bytes read from chafa's stdout.
const MAX_CHAFA_OUTPUT_BYTES: u64 = 16 * 1024 * 1024; // 16 MiB (raster payloads are larger than character art)
/// Subprocess timeout in seconds.
const CHAFA_TIMEOUT_SECS: u64 = 10;

/// Render an image using chafa.
///
/// `width` and `height` are the terminal character dimensions of the panel.
/// `graphics`, from `detect_graphics_format`, selects real terminal graphics
/// over character art when the terminal is known to support it.
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

    if let Some(format) = graphics {
        return match run_chafa_bytes(&[
            "--format", format,
            "--probe", "off",
            "--relative", "off",
            "--size", &size_arg,
            "--",
            path_str,
        ]) {
            Ok(bytes) => crate::preview::PreviewContent::KittyImage(bytes),
            Err(msg) => crate::preview::PreviewContent::Unavailable(msg),
        };
    }

    let colors_arg = if truecolor { "full" } else { "256" };
    match run_chafa_bytes(&[
        // Force character-art output. chafa's format auto-detection can
        // pick sixels/kitty and emit raw DCS graphics escape sequences,
        // which render as garbage text once captured as a string.
        "--format", "symbols",
        // chafa defaults to probing the terminal (querying it and waiting
        // up to 5s for a response, e.g. an OSC 10/11 colour query) to
        // refine its auto-detection. It inherits our stdin, which is the
        // real controlling terminal, so that query and its response race
        // directly against our own key-event reader - confirmed to leak
        // raw `rgb:RRRR/GGGG/BBBB` response bytes into whatever text
        // prompt happened to be focused. Disabled: we already pin
        // --format/--colors explicitly, so nothing chafa could learn from
        // probing changes its output here.
        "--probe", "off",
        // Fill the full requested box rather than only however much a
        // strict aspect-preserving fit would use - character art has so
        // little vertical resolution per row that aspect-correct sizing
        // often leaves most of a tall preview pane blank.
        "--stretch",
        "--size", &size_arg,
        "--colors", colors_arg,
        "--",
        path_str,
    ]) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<String> = text.split('\n').map(|l| l.to_owned()).collect();
            crate::preview::PreviewContent::ChafaLines(lines)
        }
        Err(msg) => crate::preview::PreviewContent::Unavailable(msg),
    }
}

/// Spawn chafa with a timeout and return its raw stdout bytes (bounded).
fn run_chafa_bytes(args: &[&str]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    use std::time::{Duration, Instant};

    let mut child = std::process::Command::new(CHAFA_PATH)
        .args(args)
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
    Ok(buf)
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

/// Detect a real terminal graphics protocol chafa can target, from
/// environment variables set by the terminal emulator itself - never a live
/// query, which would reopen the probe/race class of bug fixed in `render`
/// (querying the terminal and reading its response is exactly what raced our
/// own key-event reader and leaked garbage into text prompts).
///
/// Returns a value for chafa's `--format` flag, or `None` to fall back to
/// character art. Kitty, Ghostty, and WezTerm all implement the Kitty
/// graphics protocol and identify themselves via environment variables at
/// startup, so this is reliable without ever touching the terminal.
pub fn detect_graphics_format() -> Option<&'static str> {
    if std::env::var_os("KITTY_WINDOW_ID").is_some() {
        return Some("kitty");
    }
    if std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some() {
        return Some("kitty");
    }
    if std::env::var_os("WEZTERM_EXECUTABLE").is_some() {
        return Some("kitty");
    }
    match std::env::var("TERM_PROGRAM") {
        Ok(v) if v.eq_ignore_ascii_case("ghostty") || v.eq_ignore_ascii_case("wezterm") => {
            Some("kitty")
        }
        _ => None,
    }
}
