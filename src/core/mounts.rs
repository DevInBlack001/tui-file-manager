// core/mounts.rs - Detect mounted removable/network devices for the
// sidebar's "DEVICES" section: USB drives, phones (MTP/gvfs), and anything
// mounted over the network (NFS/CIFS/sshfs/etc).
//
// Reads `/proc/self/mounts` directly - no udisks2/D-Bus dependency, no
// subprocess, just a bounded local file read - so it works identically
// whatever mounted it (udisks2/udiskie automount, gvfs, a manual `mount`, or
// an `/etc/fstab` entry).

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::core::bookmarks::Bookmark;
use crate::fs::ops::resolve_bin;

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
}

/// Whether a mountpoint sits under a path removable-media/automount daemons
/// (udisks2, udiskie) or a manual mount conventionally use. gvfs is handled
/// separately (see `gvfs_children`): `gvfsd-fuse` mounts everything under
/// one single kernel mountpoint (`$XDG_RUNTIME_DIR/gvfs`) and exposes each
/// actual device as a plain subdirectory inside it rather than as its own
/// `/proc/self/mounts` entry, so matching on this path alone would only ever
/// find that one bridge mount, never the individual phone/share inside it.
fn is_conventional_mount_root(mountpoint: &str) -> bool {
    mountpoint.starts_with("/media/") || mountpoint.starts_with("/run/media/") || mountpoint.starts_with("/mnt/")
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

/// Turn a mountpoint's last path segment (or, for a gvfs child, its raw
/// directory name) into a readable label plus an icon. gvfs mount names are
/// of the form `mtp:host=...`/`smb-share:server=...,share=...` with `%XX`
/// percent-encoding; other mounts just use the directory name (typically the
/// volume label, for udisks2/udiskie automounts).
fn label_from_mountpoint(mountpoint: &str, default_icon: &'static str) -> (String, &'static str) {
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
    (decoded.replace(['_', '-'], " "), default_icon)
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
/// Combines two sources: `/proc/self/mounts` (a mount is included when it's
/// either a network filesystem type - NFS, CIFS, sshfs - or sits under a
/// conventional removable-media root: `/media`, `/run/media`, `/mnt` - and
/// is never a pseudo/virtual filesystem, which naturally excludes the root
/// filesystem, `/home` on its own partition, `/boot`, and container/snap
/// overlay mounts), plus `gvfs_children` for anything gvfs is bridging
/// (phones over MTP, network shares connected via `gio mount`), since those
/// never get their own `/proc/self/mounts` entry.
pub fn detect() -> Vec<Bookmark> {
    let mut devices = match read_capped("/proc/self/mounts", MAX_MOUNTS_BYTES) {
        Ok(content) => parse_mounts(&content),
        Err(_) => Vec::new(),
    };
    devices.extend(gvfs_children());
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices
}

/// `gvfsd-fuse` mounts everything it manages under one single kernel
/// mountpoint (`$XDG_RUNTIME_DIR/gvfs`) and exposes each actual device -
/// phones over MTP, network shares connected via `gio mount` - as a plain
/// subdirectory inside it, not as its own `/proc/self/mounts` entry. Listing
/// that directory is the only way to see them individually.
fn gvfs_children() -> Vec<Bookmark> {
    let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else { return Vec::new() };
    gvfs_children_at(&runtime_dir.join("gvfs"))
}

/// Split out from `gvfs_children` so the scanning logic is testable against
/// a temp directory instead of the real `$XDG_RUNTIME_DIR` (mutating that
/// env var for a test would race with other tests running in parallel).
fn gvfs_children_at(gvfs_dir: &std::path::Path) -> Vec<Bookmark> {
    let Ok(read_dir) = std::fs::read_dir(gvfs_dir) else { return Vec::new() };

    read_dir
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let raw_name = entry.file_name().to_str()?.to_string();
            let path = entry.path();
            // gvfs subdirectories are never network/USB fstypes in their own
            // right (the fstype is always the bridge's own fuse.gvfsd-fuse);
            // label_from_mountpoint falls back to the network icon here
            // rather than USB, since a real removable USB disk is already
            // covered by udisks2's own /media|/run/media mount, not gvfs.
            let (name, icon) = label_from_mountpoint(&raw_name, ICON_NETWORK);
            Some(Bookmark { name, exists: path.is_dir(), path, is_recents: false, icon: Some(icon) })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// MTP auto-mount
// ---------------------------------------------------------------------------
//
// A phone connected over MTP is visible to gvfs's volume monitor the moment
// it's plugged in, but nothing actually mounts it unless something calls
// `gio mount`, which a full GNOME session normally does automatically via
// Nautilus's volume-monitor integration. A minimal window-manager session
// (no Nautilus/GNOME Files running) has nothing to do that, so the phone
// just sits there detected-but-unmounted and never shows up in
// `/proc/self/mounts` at all. fim fills that gap itself: `gio mount` also
// needs `gvfsd-fuse` running to bridge the mount to a real POSIX path (not
// auto-started either, for the same reason) - both are started as needed.

/// Whether a process named `name` is currently running, checked by reading
/// `/proc/*/comm` directly rather than shelling out to `pgrep` - this is
/// polled periodically, and CLAUDE.md's own guidance is to prefer `/proc`
/// introspection over repeatedly invoking a status subprocess.
fn is_process_running(name: &str) -> bool {
    let Ok(read_dir) = std::fs::read_dir("/proc") else { return false };
    for entry in read_dir.filter_map(|e| e.ok()) {
        let is_pid = entry.file_name().to_str().is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()));
        if !is_pid {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
            if comm.trim() == name {
                return true;
            }
        }
    }
    false
}

/// Start `gvfsd-fuse` (the daemon that bridges gvfs mounts, including MTP
/// phones, to a real path under `$XDG_RUNTIME_DIR/gvfs`) if it isn't already
/// running. Fire-and-forget: it's a persistent session daemon, the same as
/// a full desktop session would start once and leave running.
fn ensure_gvfs_fuse_running() {
    if is_process_running("gvfsd-fuse") {
        return;
    }
    let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else { return };
    let Some(bin) = resolve_bin("gvfsd-fuse") else { return };
    let _ = Command::new(bin)
        .arg(runtime_dir.join("gvfs"))
        .arg("-f")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Parse `gio mount -li`'s text dump for MTP volumes that have no `Mount(`
/// sub-entry yet (i.e. detected but not mounted), returning their
/// `activation_root=` URIs.
fn unmounted_mtp_activation_roots(gio_li_output: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut is_mtp = false;
    let mut has_mount = false;
    let mut activation_root: Option<String> = None;

    fn flush(is_mtp: bool, has_mount: bool, activation_root: Option<String>, targets: &mut Vec<String>) {
        if is_mtp && !has_mount {
            if let Some(root) = activation_root {
                targets.push(root);
            }
        }
    }

    for line in gio_li_output.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("Volume(") || trimmed.starts_with("Drive(") {
            flush(is_mtp, has_mount, activation_root.take(), &mut targets);
            is_mtp = false;
            has_mount = false;
        } else if trimmed.contains("GProxyVolumeMonitorMTP") {
            is_mtp = true;
        } else if let Some(rest) = trimmed.strip_prefix("activation_root=") {
            activation_root = Some(rest.trim().to_string());
        } else if trimmed.starts_with("Mount(") {
            has_mount = true;
        }
    }
    flush(is_mtp, has_mount, activation_root.take(), &mut targets);
    targets
}

/// Ensure any MTP phone gvfs has detected but not yet mounted gets mounted,
/// so it shows up in the sidebar without the user needing a file manager
/// that does this automatically (e.g. Nautilus). `mount_attempts` tracks
/// URIs a mount was already requested for, so a phone that takes a few
/// seconds (or needs the user to tap "Allow" on the device) isn't re-spawned
/// every poll tick; entries are dropped once the volume is no longer
/// reported as unmounted (mounted, or unplugged).
///
/// A no-op wherever `gio`/`gvfs` aren't installed (`resolve_bin` returns
/// `None`) - this is an enhancement layered on top of the plain
/// `/proc/self/mounts` detection in `detect()`, never a hard requirement.
pub fn mount_pending_mtp_devices(mount_attempts: &mut HashSet<String>) {
    let Some(gio_bin) = resolve_bin("gio") else { return };

    let output = Command::new(&gio_bin)
        .args(["mount", "-li"])
        .stdin(Stdio::null())
        .output();
    let Ok(output) = output else { return };
    let text = String::from_utf8_lossy(&output.stdout);

    let targets = unmounted_mtp_activation_roots(&text);
    let still_pending: HashSet<&String> = targets.iter().collect();
    mount_attempts.retain(|a| still_pending.contains(a));

    ensure_gvfs_fuse_running();

    for target in targets {
        if !mount_attempts.insert(target.clone()) {
            continue;
        }
        let _ = Command::new(&gio_bin)
            .args(["mount", &target])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

/// Eject/unmount a device by its mountpoint path (`E` in the sidebar).
/// Fire-and-forget, like every other subprocess fim spawns for a GUI-ish
/// external action - the actual unmount can take a moment, and the next
/// mounts poll will pick up its disappearance once it completes.
pub fn eject(path: &std::path::Path) -> Result<(), String> {
    let bin = resolve_bin("gio").ok_or_else(|| "gio not found; install gvfs to eject devices".to_string())?;
    Command::new(bin)
        .args(["mount", "-u"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to eject: {e}"))
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

        let default_icon = if is_network_fstype(fstype) { ICON_NETWORK } else { ICON_USB };
        let (name, icon) = label_from_mountpoint(&mountpoint, default_icon);
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
    fn gvfs_bridge_mount_itself_is_excluded() {
        // gvfsd-fuse's own kernel mountpoint - never an individual device,
        // and it never contains "/gvfs/" (no trailing segment) so it
        // shouldn't match the conventional-mount-root check either.
        let content = "gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw 0 0\n";
        assert!(parse_mounts(content).is_empty());
    }

    #[test]
    fn gvfs_children_are_found_by_directory_listing() {
        // gvfsd-fuse exposes each device as a subdirectory of its own single
        // mountpoint, not as its own /proc/self/mounts entry (confirmed
        // against a real phone connected over MTP) - `gvfs_children_at`
        // exists because `parse_mounts` alone can never see these.
        let tmp = tempfile::tempdir().unwrap();
        let gvfs_dir = tmp.path().join("gvfs");
        std::fs::create_dir(&gvfs_dir).unwrap();
        std::fs::create_dir(gvfs_dir.join("mtp:host=SAMSUNG_SM-G991B")).unwrap();
        std::fs::create_dir(gvfs_dir.join("smb-share:server=192.168.1.5,share=Media")).unwrap();

        let mut devices = gvfs_children_at(&gvfs_dir);
        devices.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "Network share (192.168.1.5)");
        assert_eq!(devices[0].icon, Some(ICON_NETWORK));
        assert_eq!(devices[1].name, "Phone (SAMSUNG_SM-G991B)");
        assert_eq!(devices[1].icon, Some(ICON_PHONE));
    }

    #[test]
    fn gvfs_children_missing_directory_is_empty_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(gvfs_children_at(&tmp.path().join("does-not-exist")).is_empty());
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

    // Real `gio mount -li` output (trimmed to the relevant fields) from a
    // phone connected over MTP but never mounted, alongside an already-
    // mounted disk volume - the exact scenario `mount_pending_mtp_devices`
    // exists to fix.
    const GIO_LI_SAMPLE: &str = "\
Drive(1): Samsung PSSD T7
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  Volume(0): Backup
    Type: GProxyVolume (GProxyVolumeMonitorUDisks2)
    can_mount=0
    Mount(0): Backup -> file:///run/media/user/Backup
      Type: GProxyMount (GProxyVolumeMonitorUDisks2)
Volume(0): Infinix HOT 60 Pro
  Type: GProxyVolume (GProxyVolumeMonitorMTP)
  activation_root=mtp://INFINIX_Infinix_HOT_60_Pro_147947056L001046/
  themed icons:  [phone]
";

    #[test]
    fn unmounted_mtp_volume_is_detected() {
        let targets = unmounted_mtp_activation_roots(GIO_LI_SAMPLE);
        assert_eq!(targets, vec!["mtp://INFINIX_Infinix_HOT_60_Pro_147947056L001046/".to_string()]);
    }

    #[test]
    fn already_mounted_disk_volume_is_not_returned() {
        let sample = "\
Volume(0): Backup
  Type: GProxyVolume (GProxyVolumeMonitorUDisks2)
  Mount(0): Backup -> file:///run/media/user/Backup
    Type: GProxyMount (GProxyVolumeMonitorUDisks2)
";
        assert!(unmounted_mtp_activation_roots(sample).is_empty());
    }

    #[test]
    fn already_mounted_mtp_volume_is_not_returned() {
        let sample = "\
Volume(0): Infinix HOT 60 Pro
  Type: GProxyVolume (GProxyVolumeMonitorMTP)
  activation_root=mtp://INFINIX_Infinix_HOT_60_Pro_147947056L001046/
  Mount(0): Infinix HOT 60 Pro -> mtp://INFINIX_Infinix_HOT_60_Pro_147947056L001046/
    Type: GProxyShadowMount (GProxyVolumeMonitorMTP)
";
        assert!(unmounted_mtp_activation_roots(sample).is_empty());
    }
}
