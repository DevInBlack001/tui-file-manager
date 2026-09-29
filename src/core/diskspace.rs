// core/diskspace.rs - Total/used/free space per disk: an always-visible
// compact summary in the sidebar's DISKS section, and a more detailed panel
// (bars, byte counts) cycled into the preview pane's spot with Tab. Covers
// every real filesystem: the internal disk(s) a system boots from (root,
// and any separate /home, /boot, or other internal partition), plus
// whatever's currently plugged in or mounted over the network - the same
// detection `core::mounts` already does for the sidebar's DEVICES section,
// reused here rather than duplicated.

use std::path::{Path, PathBuf};

use crate::core::bookmarks::Bookmark;
use crate::core::mounts::{
    self, gvfs_children, is_conventional_mount_root, is_network_fstype, label_from_mountpoint, RealMount,
    ICON_NETWORK, ICON_USB,
};

/// Nerd Font glyph for a fixed internal disk (verified against
/// JetBrainsMonoNerdFont's real cmap, same approach as every other icon in
/// this codebase - md-harddisk, distinct from the removable-USB glyph).
const ICON_DISK: &str = "\u{f02ca}";

pub struct DiskInfo {
    /// Mountpoint for internal disks; a friendly name (phone/network share)
    /// for anything `core::mounts` already labels that way.
    pub label: String,
    pub mountpoint: PathBuf,
    pub fstype: String,
    pub total: u64,
    pub used: u64,
    /// Space available to this (unprivileged) user - `statvfs`'s `f_bavail`,
    /// matching what `df` shows as "Avail", not the raw free-block count
    /// (which includes space reserved for root).
    pub free: u64,
    pub icon: &'static str,
}

impl DiskInfo {
    pub fn used_fraction(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.used as f64 / self.total as f64 }
    }

    /// Used-space percentage, rounded to the nearest whole number, for the
    /// sidebar's compact DISKS entries.
    pub fn used_percent(&self) -> u8 {
        (self.used_fraction() * 100.0).round().clamp(0.0, 100.0) as u8
    }

    fn to_bookmark(&self) -> Bookmark {
        Bookmark {
            name: self.label.clone(),
            path: self.mountpoint.clone(),
            exists: true,
            is_recents: false,
            icon: Some(self.icon),
            disk_percent: Some(self.used_percent()),
        }
    }
}

/// `detect_all`'s disks, as sidebar bookmarks for the always-visible DISKS
/// section (see `App::maybe_poll_mounts`).
pub fn detect_all_as_bookmarks() -> Vec<Bookmark> {
    detect_all().iter().map(DiskInfo::to_bookmark).collect()
}

/// Detect every real disk (internal and external) with its total/used/free
/// space. Multiple mountpoints backed by the same underlying device (e.g.
/// several btrfs subvolumes of one root partition, all reporting identical
/// space) are collapsed to one entry, keeping the shortest mountpoint as the
/// representative path.
pub fn detect_all() -> Vec<DiskInfo> {
    let mut by_device: std::collections::HashMap<String, RealMount> = std::collections::HashMap::new();
    for m in mounts::all_real_mounts() {
        by_device
            .entry(m.device.clone())
            .and_modify(|existing| {
                if m.mountpoint.as_os_str().len() < existing.mountpoint.as_os_str().len() {
                    existing.mountpoint = m.mountpoint.clone();
                    existing.fstype = m.fstype.clone();
                }
            })
            .or_insert(m);
    }

    let mut disks: Vec<DiskInfo> = by_device
        .into_values()
        .filter_map(|m| {
            let (total, used, free) = statvfs_info(&m.mountpoint)?;
            if total == 0 {
                return None;
            }
            let mp = m.mountpoint.to_string_lossy();
            let is_removable = is_network_fstype(&m.fstype) || is_conventional_mount_root(&mp);
            if is_removable {
                let default_icon = if is_network_fstype(&m.fstype) { ICON_NETWORK } else { ICON_USB };
                let (label, icon) = label_from_mountpoint(&mp, default_icon);
                Some(DiskInfo { label, mountpoint: m.mountpoint, fstype: m.fstype, total, used, free, icon })
            } else {
                Some(DiskInfo {
                    label: mp.into_owned(),
                    mountpoint: m.mountpoint,
                    fstype: m.fstype,
                    total,
                    used,
                    free,
                    icon: ICON_DISK,
                })
            }
        })
        .collect();

    // gvfs-backed devices (phones over MTP, gio-mounted network shares)
    // never get their own /proc/self/mounts entry - see mounts.rs's own
    // gvfs_children for why - so they need a separate statvfs pass.
    for bm in gvfs_children() {
        let Some((total, used, free)) = statvfs_info(&bm.path) else { continue };
        if total == 0 {
            continue;
        }
        disks.push(DiskInfo {
            label: bm.name,
            mountpoint: bm.path,
            fstype: "gvfs".to_string(),
            total,
            used,
            free,
            icon: bm.icon.unwrap_or(ICON_NETWORK),
        });
    }

    disks.sort_by(|a, b| a.mountpoint.cmp(&b.mountpoint));
    disks
}

/// `statvfs(2)` via `libc`, the standard way to get filesystem space on
/// Linux - `std` has no safe wrapper for it. Returns `(total, used, free)`
/// in bytes, `free` being what's available to this user (`f_bavail`), not
/// the raw free-block count (which includes space reserved for root).
fn statvfs_info(path: &Path) -> Option<(u64, u64, u64)> {
    use std::os::unix::ffi::OsStrExt;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if ret != 0 {
        return None;
    }

    let block_size = stat.f_frsize as u64;
    let total = stat.f_blocks as u64 * block_size;
    let free = stat.f_bavail as u64 * block_size;
    let used = total.saturating_sub(stat.f_bfree as u64 * block_size);
    Some((total, used, free))
}

/// Human-readable byte count: "512B", "3.2K", "14.1M", "1.4G", "2.1T".
/// Disk sizes routinely reach the terabyte range, unlike individual file
/// sizes elsewhere in the UI, so this carries a "T" tier the other
/// human-size helpers in this codebase don't need.
pub fn human_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let n = n as f64;
    if n < KB {
        format!("{n}B")
    } else if n < MB {
        format!("{:.1}K", n / KB)
    } else if n < GB {
        format!("{:.1}M", n / MB)
    } else if n < TB {
        format!("{:.1}G", n / GB)
    } else {
        format!("{:.1}T", n / TB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats_each_tier() {
        assert_eq!(human_bytes(512), "512B");
        assert_eq!(human_bytes(3_277), "3.2K");
        assert_eq!(human_bytes(14_890_925), "14.2M");
        assert_eq!(human_bytes(1_503_238_553), "1.4G");
        assert_eq!(human_bytes(2_199_023_255_552), "2.0T");
    }

    #[test]
    fn used_fraction_handles_zero_total() {
        let d = DiskInfo {
            label: "x".to_string(),
            mountpoint: PathBuf::from("/"),
            fstype: "ext4".to_string(),
            total: 0,
            used: 0,
            free: 0,
            icon: ICON_DISK,
        };
        assert_eq!(d.used_fraction(), 0.0);
    }

    #[test]
    fn statvfs_on_root_returns_plausible_values() {
        // Live sanity check against the real root filesystem: exercises the
        // actual libc FFI call rather than just the pure-Rust helpers above.
        let (total, used, free) = statvfs_info(Path::new("/")).expect("statvfs(/) should succeed");
        assert!(total > 0);
        assert!(used <= total);
        assert!(free <= total);
    }
}
