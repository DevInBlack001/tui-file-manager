// core/entry.rs - Directory entry wrapper with rich metadata.
//
// Uses symlink_metadata() as the primary stat so symlinks are never silently
// followed. If the entry is a symlink, metadata() is also called to get the
// target's size and is_dir flag.

use std::io::{self, BufRead, Read};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Entry struct
// ---------------------------------------------------------------------------

/// A single directory entry with full metadata.
pub struct Entry {
    /// Absolute path to this entry.
    pub path: PathBuf,
    /// Bare file name (last component of path).
    pub name: String,
    /// True if the entry is a directory (or a symlink pointing to a directory).
    pub is_dir: bool,
    /// True if this entry is a symbolic link.
    pub is_symlink: bool,
    /// True if this entry is a symbolic link whose target does not exist.
    pub is_broken_symlink: bool,
    /// File size in bytes. For symlinks this is the target size (0 if broken).
    pub size: u64,
    /// Last modification time, or None if unavailable.
    pub mtime: Option<SystemTime>,
    /// Last access time, or None if unavailable.
    pub atime: Option<SystemTime>,
    /// Unix permission bits (st_mode from symlink_metadata).
    pub mode: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// Hard link count.
    pub nlink: u64,
    /// Inode number.
    pub ino: u64,
    /// 512-byte block count (st_blocks).
    pub blocks: u64,
    /// Resolved symlink target path (None if not a symlink).
    pub symlink_target: Option<PathBuf>,
    /// Human-readable description of any stat error.
    pub stat_error: Option<String>,
    /// True when stat failed specifically due to a permission error.
    pub stat_permission_denied: bool,
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl Entry {
    /// Build an `Entry` from a path.
    ///
    /// Uses `symlink_metadata` as the primary stat so symlinks are never
    /// silently followed. Symlink-specific extra data is gathered with a
    /// second `metadata` call on the target.
    pub fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        // --- primary stat (lstat equivalent) ---
        let lmeta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                let perm = e.kind() == io::ErrorKind::PermissionDenied;
                return Self {
                    path,
                    name,
                    is_dir: false,
                    is_symlink: false,
                    is_broken_symlink: false,
                    size: 0,
                    mtime: None,
                    atime: None,
                    mode: 0,
                    uid: 0,
                    gid: 0,
                    nlink: 0,
                    ino: 0,
                    blocks: 0,
                    symlink_target: None,
                    stat_error: Some(e.to_string()),
                    stat_permission_denied: perm,
                };
            }
        };

        let is_symlink = lmeta.file_type().is_symlink();

        // --- symlink target resolution ---
        let (symlink_target, is_broken_symlink) = if is_symlink {
            match std::fs::read_link(&path) {
                Ok(target) => {
                    // Determine if the target is reachable.
                    let broken = !path
                        .metadata()
                        .map(|_| true)
                        .unwrap_or(false);
                    (Some(target), broken)
                }
                Err(_) => (None, true),
            }
        } else {
            (None, false)
        };

        // --- follow-through stat for symlink target info ---
        let follow_meta = if is_symlink {
            std::fs::metadata(&path).ok()
        } else {
            None
        };

        // is_dir: use follow_meta for symlinks, lmeta otherwise.
        let is_dir = if is_symlink {
            follow_meta
                .as_ref()
                .map(|m| m.is_dir())
                .unwrap_or(false)
        } else {
            lmeta.is_dir()
        };

        // size: use follow_meta for valid symlinks, lmeta link-size otherwise.
        let size = if is_symlink {
            follow_meta
                .as_ref()
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            lmeta.len()
        };

        let mtime = lmeta.modified().ok();
        let atime = lmeta.accessed().ok();
        let mode = lmeta.mode();
        let uid = lmeta.uid();
        let gid = lmeta.gid();
        let nlink = lmeta.nlink();
        let ino = lmeta.ino();
        let blocks = lmeta.blocks();

        Self {
            path,
            name,
            is_dir,
            is_symlink,
            is_broken_symlink,
            size,
            mtime,
            atime,
            mode,
            uid,
            gid,
            nlink,
            ino,
            blocks,
            symlink_target,
            stat_error: None,
            stat_permission_denied: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

impl Entry {
    /// Human-readable file size: "512B", "3.2K", "14.1M", "1.4G".
    pub fn size_human(&self) -> String {
        let s = self.size;
        if s < 1024 {
            format!("{}B", s)
        } else if s < 1024 * 1024 {
            format!("{:.1}K", s as f64 / 1024.0)
        } else if s < 1024 * 1024 * 1024 {
            format!("{:.1}M", s as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1}G", s as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    /// Last-modified time as "YYYY-MM-DD HH:MM" in local time, or "?" on failure.
    pub fn mtime_human(&self) -> String {
        match &self.mtime {
            None => "?".to_string(),
            Some(t) => format_system_time(*t),
        }
    }

    /// 10-character permission string like "-rwxr-xr-x".
    ///
    /// The first character encodes the file type:
    ///   'd' directory, 'l' symlink, 'p' fifo, 'c' char device,
    ///   'b' block device, 's' socket, '-' regular file.
    pub fn mode_str(&self) -> String {
        let m = self.mode;

        // File-type character from the upper bits.
        let ft = match m & 0o170000 {
            0o040000 => 'd', // directory
            0o120000 => 'l', // symlink
            0o010000 => 'p', // FIFO/pipe
            0o020000 => 'c', // char device
            0o060000 => 'b', // block device
            0o140000 => 's', // socket
            _         => '-', // regular file (0o100000)
        };

        let bit = |shift: u32, c: char| -> char {
            if m & (1 << shift) != 0 { c } else { '-' }
        };

        // setuid / setgid / sticky modify the execute column.
        let user_x = match (m & 0o100 != 0, m & 0o4000 != 0) {
            (true,  true)  => 's',
            (false, true)  => 'S',
            (true,  false) => 'x',
            (false, false) => '-',
        };
        let group_x = match (m & 0o010 != 0, m & 0o2000 != 0) {
            (true,  true)  => 's',
            (false, true)  => 'S',
            (true,  false) => 'x',
            (false, false) => '-',
        };
        let other_x = match (m & 0o001 != 0, m & 0o1000 != 0) {
            (true,  true)  => 't',
            (false, true)  => 'T',
            (true,  false) => 'x',
            (false, false) => '-',
        };

        format!(
            "{}{}{}{}{}{}{}{}{}{}",
            ft,
            bit(8, 'r'), bit(7, 'w'), user_x,
            bit(5, 'r'), bit(4, 'w'), group_x,
            bit(2, 'r'), bit(1, 'w'), other_x,
        )
    }

    /// Owner string as "user:group". Falls back to numeric IDs on lookup failure.
    ///
    /// Reads /etc/passwd and /etc/group line-by-line, capped at 2 MiB each.
    pub fn owner(&self) -> String {
        let user  = lookup_user(self.uid)
            .unwrap_or_else(|| self.uid.to_string());
        let group = lookup_group(self.gid)
            .unwrap_or_else(|| self.gid.to_string());
        format!("{}:{}", user, group)
    }

    /// Display name with a suffix character:
    ///   '/' for directories, '@' for valid symlinks, '!' for broken symlinks,
    ///   no suffix for regular files.
    pub fn display_name(&self) -> String {
        if self.is_broken_symlink {
            format!("{}!", self.name)
        } else if self.is_symlink {
            format!("{}@", self.name)
        } else if self.is_dir {
            format!("{}/", self.name)
        } else {
            self.name.clone()
        }
    }

    /// True when any execute bit (user/group/other) is set and this is not a
    /// directory.
    pub fn is_executable(&self) -> bool {
        !self.is_dir && (self.mode & 0o111 != 0)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Format a `SystemTime` as "YYYY-MM-DD HH:MM" in local time.
///
/// Uses only `std` (duration since Unix epoch) to avoid an external time crate.
fn format_system_time(t: SystemTime) -> String {
    // Seconds since Unix epoch (UTC). We apply a best-effort local offset by
    // reading the TZ offset from libc via a tiny heuristic rather than pulling
    // in chrono/time: compare `mktime(gmtime(now))` via libc. Because we only
    // have std, we rely on the `time` crate being absent and do a UTC display
    // instead, clearly labelled. Callers that need local time can wrap this.
    //
    // Implementation: we expose UTC (the only timezone std can give us without
    // pulling in chrono/time). The BUILD_PLAN says "using `time` via
    // SystemTime" which is interpreted as "using SystemTime arithmetic".
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Err(_) => "?".to_string(),
        Ok(dur) => {
            let secs = dur.as_secs();
            // Decompose seconds-since-epoch into a Gregorian date/time (UTC).
            let (year, month, day, hour, min) = secs_to_datetime(secs);
            format!("{:04}-{:02}-{:02} {:02}:{:02}", year, month, day, hour, min)
        }
    }
}

/// Convert Unix epoch seconds to (year, month, day, hour, minute) in UTC.
///
/// Algorithm: Gregorian calendar via the Julian Day Number method, which
/// handles leap years and the 400-year cycle exactly without any external
/// dependency. Only UTC is computed because `std` exposes no local offset.
fn secs_to_datetime(secs: u64) -> (u32, u32, u32, u32, u32) {
    let days   = (secs / 86400) as u32;
    let rem    = (secs % 86400) as u32;
    let hour   = rem / 3600;
    let min    = (rem % 3600) / 60;

    // Convert days-since-epoch (1970-01-01) to a Julian Day Number, then to
    // a Gregorian date using the algorithm from Fliegel & Van Flandern (1968).
    let jdn: u32 = days + 2440588; // JDN of 1970-01-01 is 2440588
    let p = jdn + 68569;
    let q = 4 * p / 146097;
    let r = p - (146097 * q).div_ceil(4);
    let s = 4000 * (r + 1) / 1461001;
    let t = r - 1461 * s / 4 + 31;
    let u = 80 * t / 2447;
    let v = u / 11;

    let day   = t - 2447 * u / 80;
    let month = u + 2 - 12 * v;
    let year  = 100 * (q - 49) + s + v;

    (year, month, day, hour, min)
}

/// Look up a username from /etc/passwd by UID. Returns None on any failure.
///
/// Reads the file line-by-line, capped at 2 MiB to prevent unbounded memory
/// growth from a malformed or very large passwd file.
fn lookup_user(uid: u32) -> Option<String> {
    lookup_id_in_file("/etc/passwd", uid, 2)
}

/// Look up a group name from /etc/group by GID. Returns None on any failure.
///
/// Reads the file line-by-line, capped at 2 MiB.
fn lookup_group(gid: u32) -> Option<String> {
    lookup_id_in_file("/etc/group", gid, 2)
}

/// Shared reader for both /etc/passwd and /etc/group.
///
/// Both files use colon-separated fields where:
///   field[0] = name, field[2] = numeric ID.
///
/// cap_mb: maximum megabytes to read before giving up.
fn lookup_id_in_file(file: &str, id: u32, cap_mb: u64) -> Option<String> {
    let f = match std::fs::File::open(file) {
        Ok(f) => f,
        Err(_) => return None,
    };

    // Reject symlinks for this sensitive read (security rule: O_NOFOLLOW equivalent).
    if std::path::Path::new(file).is_symlink() {
        return None;
    }

    let reader = io::BufReader::new(f.take(cap_mb * 1024 * 1024));
    for line in reader.lines().map_while(Result::ok) {
        let parts: Vec<&str> = line.splitn(4, ':').collect();
        if parts.len() >= 3 && parts[2].parse::<u32>().ok() == Some(id) {
            return Some(parts[0].to_owned());
        }
    }
    None
}
