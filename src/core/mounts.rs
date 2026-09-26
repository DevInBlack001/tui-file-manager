// core/mounts.rs - Detect mounted removable/network devices for the
// sidebar's "DEVICES" section: USB drives, phones (MTP/gvfs), and anything
// mounted over the network (NFS/CIFS/sshfs/etc).
//
// Reads `/proc/self/mounts` directly - no udisks2/D-Bus dependency, no
// subprocess, just a bounded local file read - so it works identically
// whatever mounted it (udisks2/udiskie automount, gvfs, a manual `mount`, or
// an `/etc/fstab` entry).

use std::path::PathBuf;

use crate::core::bookmarks::Bookmark;

/// Bounded so a pathological `/proc/self/mounts` (not something this
/// process can normally produce, but defense in depth) can't blow up memory.
const MAX_MOUNTS_BYTES: u64 = 256 * 1024;

/// Pseudo/virtual filesystems that are never a "device" a user would want to
/// browse to, even if their mountpoint happened to fall under one of the
/// paths we otherwise treat as relevant.
const PSEUDO_FSTYPES: &[&str] = &[
    "proc", "sysfs", "cgroup", "cgroup2", "tmpfs", "devtmpfs", "devpts", "mqueue",
    "hugetlbfs", "debugfs", "tracefs", "securityfs", "pstore", "bpf", "autofs",
    "rpc_pipefs", "configfs", "fusectl", "binfmt_misc", "overlay", "squashfs",
    "efivarfs", "ramfs", "nsfs", "none",
];

fn is_network_fstype(fstype: &str) -> bool {
    matches!(fstype, "nfs" | "nfs4" | "cifs" | "smbfs" | "smb3")
        || fstype.starts_with("fuse.sshfs")
        || fstype.starts_with("fuse.rclone")
        || fstype.starts_with("fuse.gvfsd")
}

/// Whether a mountpoint sits under a path removable-media/automount daemons
/// (udisks2, udiskie, gvfs) or a manual mount conventionally use.
fn is_conventional_mount_root(mountpoint: &str) -> bool {
    mountpoint.starts_with("/media/")
        || mountpoint.starts_with("/run/media/")
        || mountpoint.starts_with("/mnt/")
        || mountpoint.contains("/gvfs/")
}

/// `/proc/self/mounts` escapes space, tab, backslash and newline as octal
/// (`\040` etc) inside fields; undo that so paths/labels read naturally.
fn unescape_octal(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            if let Ok(code) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(code as char);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Nerd Font glyphs, verified against JetBrainsMonoNerdFont's real cmap (see
/// the same verification approach used for file-type icons in
/// ui/filelist.rs - a memorized PUA codepoint is not trustworthy on its own).
const ICON_PHONE: &str = "\u{ed08}"; // fa-mobile
const ICON_NETWORK: &str = "\u{ef09}"; // fa-network_wired
const ICON_USB: &str = "\u{f287}"; // fa-usb

/// Turn a mountpoint's last path segment into a readable label plus an icon.
/// gvfs mount names are of the form `mtp:host=...`/
/// `smb-share:server=...,share=...` with `%XX` percent-encoding; other
/// mounts just use the directory name (typically the volume label, for
/// udisks2/udiskie automounts).
fn label_from_mountpoint(mountpoint: &str, fstype: &str) -> (String, &'static str) {
    let raw = mountpoint.rsplit('/').find(|s| !s.is_empty()).unwrap_or("device");
    let decoded = percent_decode(raw);

    if let Some(rest) = decoded.strip_prefix("mtp:host=") {
        return (format!("Phone ({rest})"), ICON_PHONE);
    }
    if let Some(rest) = decoded.strip_prefix("smb-share:server=") {
        let server = rest.split(',').next().unwrap_or(rest);
        return (format!("Network share ({server})"), ICON_NETWORK);
    }
    if let Some(rest) = decoded.strip_prefix("sftp:host=") {
        let host = rest.split(',').next().unwrap_or(rest);
        return (format!("SFTP ({host})"), ICON_NETWORK);
    }
    if is_network_fstype(fstype) {
        return (decoded.replace(['_', '-'], " "), ICON_NETWORK);
    }
    (decoded.replace(['_', '-'], " "), ICON_USB)
}

/// Minimal `%XX` percent-decoder (gvfs mount names are percent-encoded
/// UTF-8); invalid sequences are left as-is rather than dropped.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Detect currently mounted removable/network devices, as sidebar bookmarks.
///
/// A mount is included when it's either a network filesystem type (NFS,
/// CIFS, sshfs, gvfs-backed mounts including MTP phones) or sits under a
/// conventional removable-media root (`/media`, `/run/media`, `/mnt`,
/// `.../gvfs/...`) - and is never a pseudo/virtual filesystem. This
/// naturally excludes the root filesystem, `/home` on its own partition,
/// `/boot`, and container/snap overlay mounts, none of which are things a
/// user wants listed as a "device".
pub fn detect() -> Vec<Bookmark> {
    match read_capped("/proc/self/mounts", MAX_MOUNTS_BYTES) {
        Ok(content) => parse_mounts(&content),
        Err(_) => Vec::new(),
    }
}

/// Parses `/proc/self/mounts`-formatted text into device bookmarks. Split
/// out from `detect()` so the filtering/parsing logic is testable without
/// real hardware or root.
fn parse_mounts(content: &str) -> Vec<Bookmark> {
    let mut devices: Vec<Bookmark> = Vec::new();
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let Some(_device) = fields.next() else { continue };
        let Some(raw_mountpoint) = fields.next() else { continue };
        let Some(fstype) = fields.next() else { continue };

        if PSEUDO_FSTYPES.contains(&fstype) {
            continue;
        }
        let mountpoint = unescape_octal(raw_mountpoint);
        if !(is_network_fstype(fstype) || is_conventional_mount_root(&mountpoint)) {
            continue;
        }

        let (name, icon) = label_from_mountpoint(&mountpoint, fstype);
        let path = PathBuf::from(&mountpoint);
        devices.push(Bookmark {
            name,
            exists: path.is_dir(),
            path,
            is_recents: false,
            icon: Some(icon),
        });
    }

    // Dedup by path first (requires path-sorted adjacency), then sort for
    // display: a bind mount or an fstab entry duplicating an automount can
    // otherwise show the same device twice.
    devices.sort_by(|a, b| a.path.cmp(&b.path));
    devices.dedup_by(|a, b| a.path == b.path);
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices
}

fn read_capped(path: &str, max_bytes: u64) -> std::io::Result<String> {
    use std::io::Read;
    let f = std::fs::File::open(path)?;
    let mut buf = String::new();
    f.take(max_bytes).read_to_string(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_system_mounts_are_excluded() {
        let content = "\
/dev/mapper/root / btrfs rw,relatime 0 0
/dev/mapper/root /var/log btrfs rw,relatime 0 0
/dev/mapper/root /home btrfs rw,relatime 0 0
/dev/nvme0n1p1 /boot vfat rw,relatime 0 0
proc /proc proc rw,nosuid 0 0
tmpfs /run tmpfs rw,nosuid 0 0
overlay /var/lib/docker/overlay2/abc/merged overlay rw 0 0
";
        assert!(parse_mounts(content).is_empty());
    }

    #[test]
    fn usb_drive_under_run_media_is_included() {
        let content = "/dev/sdb1 /run/media/user/MY_USB vfat rw,relatime 0 0\n";
        let devices = parse_mounts(content);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "MY USB");
        assert_eq!(devices[0].icon, Some(ICON_USB));
    }

    #[test]
    fn mtp_phone_gets_a_friendly_label_and_phone_icon() {
        let content = "gvfsd-fuse /run/user/1000/gvfs/mtp:host=SAMSUNG_SM-G991B fuse.gvfsd-fuse rw 0 0\n";
        let devices = parse_mounts(content);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "Phone (SAMSUNG_SM-G991B)");
        assert_eq!(devices[0].icon, Some(ICON_PHONE));
    }

    #[test]
    fn network_filesystem_is_included_even_outside_conventional_roots() {
        let content = "192.168.1.10:/export/media /srv/media-share nfs4 rw,relatime 0 0\n";
        let devices = parse_mounts(content);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].icon, Some(ICON_NETWORK));
    }

    #[test]
    fn cifs_share_is_included() {
        let content = "//192.168.1.5/Media /mnt/media-share cifs rw,relatime 0 0\n";
        let devices = parse_mounts(content);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].icon, Some(ICON_NETWORK));
    }

    #[test]
    fn octal_escaped_spaces_in_mountpoint_are_decoded() {
        let content = "/dev/sdb1 /run/media/user/My\\040Drive vfat rw 0 0\n";
        let devices = parse_mounts(content);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].path, PathBuf::from("/run/media/user/My Drive"));
    }

    #[test]
    fn duplicate_mountpoints_are_deduplicated() {
        let content = "\
/dev/sdb1 /run/media/user/USB vfat rw 0 0
/dev/sdb1 /run/media/user/USB vfat rw 0 0
";
        assert_eq!(parse_mounts(content).len(), 1);
    }
}
