use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Outcome of a filesystem operation.
#[derive(Debug)]
pub enum OpResult {
    Ok(String),
    Err(String),
    PermissionDenied,
    NotFound,
}

// ---------------------------------------------------------------------------
// Name validation
// ---------------------------------------------------------------------------

/// Reject names that would be unsafe or meaningless as filesystem entries.
fn validate_name(name: &str) -> Result<(), OpResult> {
    if name.is_empty() {
        return Err(OpResult::Err("name must not be empty".to_string()));
    }
    if name == "." || name == ".." {
        return Err(OpResult::Err(
            "name must not be '.' or '..'".to_string(),
        ));
    }
    if name.contains('/') {
        return Err(OpResult::Err(
            "name must not contain a '/' character".to_string(),
        ));
    }
    if name.contains('\0') {
        return Err(OpResult::Err(
            "name must not contain a null byte".to_string(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// Rename a file or directory to `new_name` within its parent directory.
/// `new_name` must be a plain filename component (no path separators).
pub fn rename(src: &Path, new_name: &str) -> OpResult {
    if let Err(e) = validate_name(new_name) {
        return e;
    }

    let parent = match src.parent() {
        Some(p) => p,
        None => {
            return OpResult::Err(format!(
                "cannot determine parent of '{}'",
                src.display()
            ))
        }
    };

    let dest: PathBuf = parent.join(new_name);

    match std::fs::rename(src, &dest) {
        Ok(()) => OpResult::Ok(format!(
            "renamed '{}' to '{}'",
            src.display(),
            dest.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            OpResult::PermissionDenied
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => OpResult::NotFound,
        Err(e) => OpResult::Err(format!("rename failed: {e}")),
    }
}

/// Create a new directory named `name` inside `parent`.
pub fn mkdir(parent: &Path, name: &str) -> OpResult {
    if let Err(e) = validate_name(name) {
        return e;
    }

    let target = parent.join(name);

    match std::fs::create_dir(&target) {
        Ok(()) => OpResult::Ok(format!("created directory '{}'", target.display())),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            OpResult::PermissionDenied
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => OpResult::NotFound,
        Err(e) => OpResult::Err(format!("mkdir failed: {e}")),
    }
}

/// Create an empty file named `name` inside `parent` (like `touch`).
pub fn touch(parent: &Path, name: &str) -> OpResult {
    if let Err(e) = validate_name(name) {
        return e;
    }

    let target = parent.join(name);

    match OpenOptions::new().create(true).append(true).open(&target) {
        Ok(_) => OpResult::Ok(format!("created '{}'", target.display())),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            OpResult::PermissionDenied
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => OpResult::NotFound,
        Err(e) => OpResult::Err(format!("touch failed: {e}")),
    }
}

/// Permanently delete `path`.
///
/// Uses `symlink_metadata` so that symlinks are removed as links, not as their
/// targets.  Directories are removed recursively.
pub fn delete(path: &Path) -> OpResult {
    let meta = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return OpResult::NotFound,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return OpResult::PermissionDenied
        }
        Err(e) => return OpResult::Err(format!("stat failed: {e}")),
    };

    let result = if meta.is_symlink() || meta.is_file() {
        std::fs::remove_file(path)
    } else {
        std::fs::remove_dir_all(path)
    };

    match result {
        Ok(()) => OpResult::Ok(format!("deleted '{}'", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            OpResult::PermissionDenied
        }
        Err(e) => OpResult::Err(format!("delete failed: {e}")),
    }
}

/// Archive `paths` (all direct children of `cwd`) into `cwd/archive`.
///
/// The format follows the archive name: `.zip`, `.tar`, `.tar.gz`/`.tgz`,
/// `.tar.xz`, `.tar.zst`. Runs `tar`/`zip` with an argument list (no shell)
/// from inside `cwd` so stored paths are relative. Never overwrites, and
/// removes a partial archive if the tool fails.
pub fn compress(cwd: &Path, paths: &[PathBuf], archive: &str) -> OpResult {
    if let Err(e) = validate_name(archive) {
        return e;
    }
    let mut names = Vec::new();
    for p in paths {
        match (p.parent(), p.file_name()) {
            (Some(parent), Some(n)) if parent == cwd => names.push(n.to_os_string()),
            _ => return OpResult::Err("items must be in the current folder".to_string()),
        }
    }
    if names.is_empty() {
        return OpResult::Err("nothing to compress".to_string());
    }
    let target = cwd.join(archive);
    if target.symlink_metadata().is_ok() {
        return OpResult::Err(format!("'{archive}' already exists"));
    }

    let lower = archive.to_lowercase();
    let (tool, mut args): (&str, Vec<std::ffi::OsString>) = if lower.ends_with(".zip") {
        ("zip", vec!["-r".into(), "-q".into(), format!("./{archive}").into(), "--".into()])
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        ("tar", vec!["-czf".into(), format!("./{archive}").into(), "--".into()])
    } else if lower.ends_with(".tar.xz") {
        ("tar", vec!["-cJf".into(), format!("./{archive}").into(), "--".into()])
    } else if lower.ends_with(".tar.zst") {
        ("tar", vec!["--zstd".into(), "-cf".into(), format!("./{archive}").into(), "--".into()])
    } else if lower.ends_with(".tar") {
        ("tar", vec!["-cf".into(), format!("./{archive}").into(), "--".into()])
    } else {
        return OpResult::Err("use .zip, .tar, .tar.gz, .tgz, .tar.xz or .tar.zst".to_string());
    };
    args.extend(names);

    let Some(bin) = resolve_bin(tool) else {
        return OpResult::Err(format!("'{tool}' not found. Install it with: sudo pacman -S {tool}"));
    };
    match Command::new(&bin)
        .args(&args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(out) if out.status.success() => OpResult::Ok(format!("created '{archive}'")),
        Ok(out) => {
            let _ = std::fs::remove_file(&target);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let first = stderr.lines().next().unwrap_or("unknown error");
            OpResult::Err(format!("{tool} failed: {first}"))
        }
        Err(e) => OpResult::Err(format!("could not execute '{}': {e}", bin.display())),
    }
}

/// Move `path` to the user's trash using `trash-put`.
///
/// Binary search order:
/// 1. `$HOME/.local/bin/trash-put`
/// 2. `/usr/bin/trash-put`
/// 3. Walk `$PATH` entries (no shell; manual PATH split)
///
/// If none is found, returns a clear error with an install hint.
pub fn trash(path: &Path) -> OpResult {
    let trash_bin = find_trash_put();

    let bin = match trash_bin {
        Some(b) => b,
        None => {
            return OpResult::Err(
                "trash-put not found. Install it with: paru -S trash-cli".to_string(),
            )
        }
    };

    match Command::new(&bin)
        .arg("--")
        .arg(path)
        // Never let a subprocess inherit our real controlling terminal on
        // stdin (see preview/image.rs's chafa --probe fix for why).
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(out) if out.status.success() => {
            OpResult::Ok(format!("trashed '{}'", path.display()))
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if stderr.contains("Permission denied") || stderr.contains("permission denied") {
                OpResult::PermissionDenied
            } else {
                OpResult::Err(format!("trash-put failed: {stderr}"))
            }
        }
        Err(e) => OpResult::Err(format!("could not execute '{}': {e}", bin.display())),
    }
}

/// Resolve `name` to an absolute executable path without invoking a shell.
///
/// If `name` already contains a `/` it is used as-is (validated to be a
/// file). Otherwise `$PATH` is walked manually so the caller never passes a
/// bare, `$PATH`-searched name to `Command::new`.
pub fn resolve_bin(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return if p.is_file() { Some(p) } else { None };
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = PathBuf::from(dir).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Locate the `trash-put` binary without invoking a shell.
fn find_trash_put() -> Option<PathBuf> {
    // 1. User-local install.
    if let Some(home) = dirs::home_dir() {
        let candidate = home.join(".local").join("bin").join("trash-put");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // 2. System install.
    let system = PathBuf::from("/usr/bin/trash-put");
    if system.is_file() {
        return Some(system);
    }

    // 3. Walk $PATH manually (no shell).
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let candidate = PathBuf::from(dir).join("trash-put");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(test)]
mod compress_tests {
    use super::*;

    fn setup() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join("d")).unwrap();
        std::fs::write(t.path().join("d/a.txt"), b"a").unwrap();
        std::fs::write(t.path().join("b.txt"), b"b").unwrap();
        t
    }

    #[test]
    fn compresses_folder_and_file_to_each_format() {
        for ext in ["zip", "tar", "tar.gz", "tgz", "tar.xz", "tar.zst"] {
            let t = setup();
            let name = format!("out.{ext}");
            let paths = vec![t.path().join("d"), t.path().join("b.txt")];
            assert!(matches!(compress(t.path(), &paths, &name), OpResult::Ok(_)), "{ext}");
            assert!(t.path().join(&name).metadata().unwrap().len() > 0, "{ext}");
        }
    }

    #[test]
    fn refuses_overwrite_unknown_ext_and_foreign_paths() {
        let t = setup();
        let paths = vec![t.path().join("b.txt")];
        std::fs::write(t.path().join("x.zip"), b"keep").unwrap();
        assert!(matches!(compress(t.path(), &paths, "x.zip"), OpResult::Err(_)));
        assert_eq!(std::fs::read(t.path().join("x.zip")).unwrap(), b"keep");
        assert!(matches!(compress(t.path(), &paths, "x.rar"), OpResult::Err(_)));
        assert!(matches!(compress(t.path(), &[PathBuf::from("/etc/hostname")], "y.zip"), OpResult::Err(_)));
    }
}
