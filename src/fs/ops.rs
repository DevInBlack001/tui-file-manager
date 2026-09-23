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
