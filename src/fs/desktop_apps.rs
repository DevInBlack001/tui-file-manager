// fs/desktop_apps.rs - Discover installed applications via XDG `.desktop`
// files for the "Open With" picker.
//
// This surfaces ordinary Linux apps and, with no special-casing needed, any
// Wine/Proton-wrapped app too: installers for those (Lutris, Bottles,
// Heroic, or plain `wine desktop-file-install`) already register a normal
// `.desktop` entry in the same directories, so it's picked up by the same
// scan and launched the same way (its `Exec=` line is whatever wrapper
// script or `wine ... .exe` command the installer wrote).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::fs::ops::resolve_bin;

pub struct DesktopApp {
    pub name: String,
    pub exec: String,
}

/// Per-file read cap so a pathologically large `.desktop` file can't blow up
/// memory; real ones are a few hundred bytes.
const MAX_DESKTOP_FILE_BYTES: u64 = 64 * 1024;
/// Result cap so a system with an enormous number of `.desktop` files can't
/// blow up the picker's render loop.
const MAX_APPS: usize = 200;

#[derive(Default)]
struct RawDesktopEntry {
    name: Option<String>,
    exec: Option<String>,
    mime_types: Vec<String>,
    no_display: bool,
    is_application: bool,
    try_exec: Option<String>,
}

/// Directories to search for `.desktop` files, in XDG precedence order.
/// `$XDG_DATA_DIRS` is read first (never hardcoded); the two standard system
/// locations are only a fallback for when it's unset, as the XDG Base
/// Directory spec itself defines.
fn application_dirs() -> Vec<PathBuf> {
    let mut dirs_list = Vec::new();
    if let Some(data_home) = dirs::data_dir() {
        dirs_list.push(data_home.join("applications"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for d in data_dirs.split(':').filter(|s| !s.is_empty()) {
        dirs_list.push(PathBuf::from(d).join("applications"));
    }
    dirs_list
}

/// Read at most `max_bytes` of `path` as UTF-8 (lossy), never more.
fn read_capped(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    use std::io::Read;
    let f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    f.take(max_bytes).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Parse only the `[Desktop Entry]` group; any group boundary reached after
/// it (e.g. `[Desktop Action ...]`) ends parsing, since nothing after it is
/// needed here.
fn parse_desktop_entry(path: &Path) -> Option<RawDesktopEntry> {
    let content = read_capped(path, MAX_DESKTOP_FILE_BYTES).ok()?;
    let mut entry = RawDesktopEntry::default();
    let mut in_main_group = false;
    let mut seen_main_group = false;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if seen_main_group {
                break;
            }
            in_main_group = line == "[Desktop Entry]";
            seen_main_group = in_main_group;
            continue;
        }
        if !in_main_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        match key.trim() {
            "Name" => entry.name = Some(value.to_string()),
            "Exec" => entry.exec = Some(value.to_string()),
            "MimeType" => {
                entry.mime_types = value.split(';').filter(|s| !s.is_empty()).map(str::to_string).collect()
            }
            "NoDisplay" => entry.no_display = value.eq_ignore_ascii_case("true"),
            "Type" => entry.is_application = value == "Application",
            "TryExec" => entry.try_exec = Some(value.to_string()),
            _ => {}
        }
    }
    Some(entry)
}

/// `image/*` matches any `image/...`; otherwise an exact match is required.
fn mime_matches(mime: &str, patterns: &[String]) -> bool {
    let want_type = mime.split_once('/').map(|(t, _)| t).unwrap_or(mime);
    patterns.iter().any(|p| {
        p == mime || p.split_once('/').is_some_and(|(ptype, psub)| ptype == want_type && psub == "*")
    })
}

/// Find installed applications that declare (via `MimeType=`) they can open
/// `mime`. Skips entries marked `NoDisplay`, whose `Type` isn't
/// `Application`, or whose `TryExec` binary isn't actually installed (stale
/// leftover launchers). Results are deduplicated by `.desktop` file id
/// following XDG precedence (the first, highest-priority directory to
/// define a given id wins) and sorted by display name.
pub fn candidates_for_mime(mime: &str) -> Vec<DesktopApp> {
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut apps = Vec::new();

    'dirs: for dir in application_dirs() {
        let Ok(read_dir) = std::fs::read_dir(&dir) else { continue };
        for file_entry in read_dir.filter_map(|e| e.ok()) {
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
            if !seen_ids.insert(id.clone()) {
                continue;
            }

            let Some(raw) = parse_desktop_entry(&path) else { continue };
            if raw.no_display || !raw.is_application {
                continue;
            }
            let Some(exec) = raw.exec else { continue };
            if !mime_matches(mime, &raw.mime_types) {
                continue;
            }
            if let Some(try_exec) = &raw.try_exec {
                if resolve_bin(try_exec).is_none() {
                    continue;
                }
            }

            let name = raw.name.unwrap_or_else(|| id.trim_end_matches(".desktop").to_string());
            apps.push(DesktopApp { name, exec });
            if apps.len() >= MAX_APPS {
                break 'dirs;
            }
        }
    }

    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}

/// Resolve a `.desktop` `Exec=` line against a target path into a real
/// binary + argv, the same safety properties as every other subprocess fim
/// spawns: an explicit argv, never a shell, and a resolved absolute binary
/// path rather than a bare `$PATH`-searched name handed to `Command::new`.
///
/// Field codes per the Desktop Entry Specification: `%f`/`%F`/`%u`/`%U`
/// become the target path (as a single argv entry, so a path containing
/// spaces is never split); `%i`/`%c`/`%k` (icon/name/desktop-file, which fim
/// has no use for) are dropped; `%%` becomes a literal `%`. If the Exec line
/// has no path placeholder at all, the path is appended at the end, matching
/// the `xdg-open`-style convention the rest of fim's "open" actions use.
pub fn resolve_desktop_exec(exec_field: &str, path: &Path) -> Option<(PathBuf, Vec<String>)> {
    let path_str = path.to_string_lossy().into_owned();
    let mut saw_placeholder = false;
    let mut tokens: Vec<String> = Vec::new();

    for tok in exec_field.split_whitespace() {
        match tok {
            "%f" | "%F" | "%u" | "%U" => {
                tokens.push(path_str.clone());
                saw_placeholder = true;
            }
            "%i" | "%c" | "%k" => {}
            "%%" => tokens.push("%".to_string()),
            other => tokens.push(other.to_string()),
        }
    }

    if tokens.is_empty() {
        return None;
    }
    let bin = resolve_bin(&tokens.remove(0))?;
    if !saw_placeholder {
        tokens.push(path_str);
    }
    Some((bin, tokens))
}
