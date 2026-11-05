// trash.rs - Read and restore items in the freedesktop.org Trash
// (`$XDG_DATA_HOME/Trash`, default `~/.local/share/Trash`).
//
// Layout per the spec: `files/<name>` holds the trashed item and
// `info/<name>.trashinfo` records its original absolute `Path=` (percent
// encoded) and `DeletionDate=`. Restoring renames the item back and removes
// the info file. Nothing here shells out or touches the network.

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use super::ops::OpResult;

/// Hard cap on listed items so a huge trash cannot blow up memory or the UI.
const MAX_ITEMS: usize = 5000;
/// A .trashinfo file is a few lines; anything bigger is not one.
const MAX_INFO_BYTES: u64 = 8 * 1024;
/// Clip stored strings so one corrupted record cannot flood the UI.
const MAX_FIELD_CHARS: usize = 1024;

#[derive(Debug, Clone)]
pub struct TrashItem {
    /// Name inside `files/` (and stem of the info file).
    pub name: String,
    /// Absolute path the item had before it was trashed.
    pub original: PathBuf,
    /// `YYYY-MM-DDTHH:MM:SS` local time, as written by the trasher.
    pub deleted_at: String,
    pub is_dir: bool,
    pub size: u64,
}

/// Why the trash could not be listed.
#[derive(Debug)]
pub enum TrashError {
    /// No trash directory exists (nothing was ever trashed).
    NotPresent,
    PermissionDenied,
    Other(String),
}

pub fn trash_root() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
        let p = PathBuf::from(d);
        if p.is_absolute() {
            return Some(p.join("Trash"));
        }
    }
    dirs::home_dir().map(|h| h.join(".local").join("share").join("Trash"))
}

/// List trashed items, newest deletion first.
pub fn list() -> Result<Vec<TrashItem>, TrashError> {
    let root = trash_root().ok_or(TrashError::NotPresent)?;
    list_in(&root)
}

pub fn list_in(root: &Path) -> Result<Vec<TrashItem>, TrashError> {
    let info_dir = root.join("info");
    let files_dir = root.join("files");
    let rd = match std::fs::read_dir(&info_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(TrashError::NotPresent),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(TrashError::PermissionDenied)
        }
        Err(e) => return Err(TrashError::Other(e.to_string())),
    };

    let mut items = Vec::new();
    for entry in rd.flatten() {
        if items.len() >= MAX_ITEMS {
            break;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Some(name) = file_name.strip_suffix(".trashinfo") else { continue };
        let Some(text) = read_info(&entry.path()) else { continue };
        let Some((original, deleted_at)) = parse_info(&text) else { continue };

        // symlink_metadata: a trashed symlink is listed as a link, never followed.
        let Ok(meta) = files_dir.join(name).symlink_metadata() else { continue };
        items.push(TrashItem {
            name: clip(name),
            original,
            deleted_at,
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
        });
    }
    items.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at).then_with(|| a.name.cmp(&b.name)));
    Ok(items)
}

/// Bounded, symlink-refusing read of an info file.
fn read_info(path: &Path) -> Option<String> {
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let mut buf = String::new();
    f.take(MAX_INFO_BYTES).read_to_string(&mut buf).ok()?;
    Some(buf)
}

/// Extract `(original path, deletion date)`; the path must be absolute.
fn parse_info(text: &str) -> Option<(PathBuf, String)> {
    let mut path = None;
    let mut date = String::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Path=") {
            path = Some(percent_decode(v.trim()));
        } else if let Some(v) = line.strip_prefix("DeletionDate=") {
            date = clip(v.trim());
        }
    }
    let path = path?;
    if !path.starts_with('/') || path.contains('\0') {
        return None;
    }
    Some((PathBuf::from(path), date))
}

fn clip(s: &str) -> String {
    s.chars().take(MAX_FIELD_CHARS).collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Restore `item` to its original location. Refuses to overwrite.
pub fn restore(item: &TrashItem) -> OpResult {
    match trash_root() {
        Some(root) => restore_in(&root, item),
        None => OpResult::Err("trash directory not found".to_string()),
    }
}

pub fn restore_in(root: &Path, item: &TrashItem) -> OpResult {
    if item.name.is_empty() || item.name.contains('/') || item.name == "." || item.name == ".." {
        return OpResult::Err("invalid trash entry name".to_string());
    }
    let src = root.join("files").join(&item.name);
    let info = root.join("info").join(format!("{}.trashinfo", item.name));
    let dest = &item.original;

    if src.symlink_metadata().is_err() {
        return OpResult::NotFound;
    }
    if dest.symlink_metadata().is_ok() {
        return OpResult::Err(format!("'{}' already exists", dest.display()));
    }
    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return map_err("could not create parent folder", e);
        }
    }
    if let Err(e) = std::fs::rename(&src, dest) {
        return map_err("restore failed", e);
    }
    let _ = std::fs::remove_file(&info);
    OpResult::Ok(format!("restored '{}'", dest.display()))
}

fn map_err(ctx: &str, e: std::io::Error) -> OpResult {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => OpResult::PermissionDenied,
        _ => OpResult::Err(format!("{ctx}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("Trash/files")).unwrap();
        std::fs::create_dir_all(t.path().join("Trash/info")).unwrap();
        t
    }

    fn put(t: &tempfile::TempDir, name: &str, orig: &Path, date: &str) {
        std::fs::write(t.path().join("Trash/files").join(name), b"data").unwrap();
        let enc = orig.to_string_lossy().replace(' ', "%20");
        std::fs::write(
            t.path().join("Trash/info").join(format!("{name}.trashinfo")),
            format!("[Trash Info]\nPath={enc}\nDeletionDate={date}\n"),
        )
        .unwrap();
    }

    #[test]
    fn lists_newest_first_and_decodes_path() {
        let t = fixture();
        put(&t, "a b.txt", Path::new("/home/u/a b.txt"), "2026-01-01T00:00:00");
        put(&t, "c.txt", Path::new("/home/u/c.txt"), "2026-02-01T00:00:00");
        let items = list_in(&t.path().join("Trash")).unwrap();
        assert_eq!(items[0].name, "c.txt");
        assert_eq!(items[1].original, PathBuf::from("/home/u/a b.txt"));
    }

    #[test]
    fn restore_moves_back_and_removes_info() {
        let t = fixture();
        let dest = t.path().join("out/x.txt");
        put(&t, "x.txt", &dest, "2026-01-01T00:00:00");
        let root = t.path().join("Trash");
        let item = list_in(&root).unwrap().remove(0);
        assert!(matches!(restore_in(&root, &item), OpResult::Ok(_)));
        assert!(dest.exists());
        assert!(list_in(&root).unwrap().is_empty());
    }

    #[test]
    fn restore_refuses_to_overwrite() {
        let t = fixture();
        let dest = t.path().join("x.txt");
        std::fs::write(&dest, b"keep").unwrap();
        put(&t, "x.txt", &dest, "2026-01-01T00:00:00");
        let root = t.path().join("Trash");
        let item = list_in(&root).unwrap().remove(0);
        assert!(matches!(restore_in(&root, &item), OpResult::Err(_)));
        assert_eq!(std::fs::read(&dest).unwrap(), b"keep");
    }

    #[test]
    fn rejects_relative_paths_and_missing_trash() {
        assert!(parse_info("[Trash Info]\nPath=relative/x\n").is_none());
        assert!(matches!(list_in(Path::new("/nonexistent-trash")), Err(TrashError::NotPresent)));
    }
}
