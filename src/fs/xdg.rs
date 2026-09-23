use std::io::Read;
use std::path::{Path, PathBuf};

/// XDG user directories resolved at runtime. All fields are `Option` because
/// a directory may not be configured or may not exist on this machine.
pub struct XdgDirs {
    pub home: PathBuf,
    pub documents: Option<PathBuf>,
    pub pictures: Option<PathBuf>,
    pub videos: Option<PathBuf>,
    pub music: Option<PathBuf>,
    pub downloads: Option<PathBuf>,
    /// `$HOME/Work` if it exists and is a directory.
    pub work: Option<PathBuf>,
}

/// Parse `user-dirs.dirs` shell-assignment lines.
/// Returns pairs of (KEY, expanded_value) where KEY is the bare variable name
/// (e.g. `XDG_DOCUMENTS_DIR`).
fn parse_user_dirs(content: &str, home: &Path) -> Vec<(String, PathBuf)> {
    let home_str = home.to_string_lossy();
    let mut result = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // Skip comments and empty lines.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Expect: KEY="value"
        let Some((key, rest)) = line.split_once('=') else { continue };
        let key = key.trim();
        // Strip surrounding quotes.
        let value = rest.trim().trim_matches('"');
        // Substitute $HOME at the start (the only variable used in user-dirs.dirs).
        let expanded = if value.starts_with("$HOME/") {
            format!("{}/{}", home_str, &value["$HOME/".len()..])
        } else if value.starts_with("$HOME") && value.len() == 5 {
            home_str.to_string()
        } else {
            value.to_string()
        };
        result.push((key.to_owned(), PathBuf::from(expanded)));
    }
    result
}

/// Validate that a path both exists and is a directory.
fn valid_dir(p: PathBuf) -> Option<PathBuf> {
    if p.is_dir() { Some(p) } else { None }
}

/// Resolve XDG user directories without hardcoding any paths.
///
/// Resolution order for config file:
/// 1. `$XDG_CONFIG_HOME/user-dirs.dirs`
/// 2. `$HOME/.config/user-dirs.dirs`
pub fn resolve() -> XdgDirs {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

    // Locate and read the user-dirs config; cap at 64 KiB.
    let user_dirs_path = dirs::config_dir()
        .map(|p| p.join("user-dirs.dirs"))
        .filter(|p| p.is_file());

    let assignments: Vec<(String, PathBuf)> = match user_dirs_path {
        Some(ref path) => {
            match std::fs::File::open(path) {
                Ok(f) => {
                    let mut buf = String::new();
                    if std::io::BufReader::new(f.take(64 * 1024))
                        .read_to_string(&mut buf)
                        .is_ok()
                    {
                        parse_user_dirs(&buf, &home)
                    } else {
                        Vec::new()
                    }
                }
                Err(_) => Vec::new(),
            }
        }
        None => Vec::new(),
    };

    // Build a quick lookup from key to validated path.
    let lookup = |key: &str| -> Option<PathBuf> {
        assignments
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, p)| valid_dir(p.clone()))
    };

    let work = valid_dir(home.join("Work"));

    XdgDirs {
        home,
        documents: lookup("XDG_DOCUMENTS_DIR"),
        pictures:  lookup("XDG_PICTURES_DIR"),
        videos:    lookup("XDG_VIDEOS_DIR"),
        music:     lookup("XDG_MUSIC_DIR"),
        downloads: lookup("XDG_DOWNLOAD_DIR"),
        work,
    }
}
