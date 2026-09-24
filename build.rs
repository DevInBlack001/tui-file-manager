// build.rs - meta.json is this project's single source of truth for the
// binary's name and version. Cargo itself has no mechanism to read
// `[package] name`/`version` from an external file (they must be literal
// values in Cargo.toml before Cargo can even locate a build script to run),
// so this build script instead makes the two impossible to silently drift
// apart: it fails the build if Cargo.toml's name/version ever disagrees
// with meta.json. Everything else that needs the name/version (the app's
// own `--version` output via Cargo's compile-time env vars, install.sh and
// friends, GitHub releases) reads meta.json directly, or transitively
// through Cargo's env vars, which this check keeps guaranteed in sync.

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let meta_path = std::path::Path::new(&manifest_dir).join("meta.json");
    println!("cargo:rerun-if-changed={}", meta_path.display());

    let meta_text = std::fs::read_to_string(&meta_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", meta_path.display()));
    let meta: serde_json::Value = serde_json::from_str(&meta_text)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", meta_path.display()));

    let meta_name = meta["name"]
        .as_str()
        .unwrap_or_else(|| panic!("{} is missing a string \"name\" field", meta_path.display()));
    let meta_version = meta["version"]
        .as_str()
        .unwrap_or_else(|| panic!("{} is missing a string \"version\" field", meta_path.display()));

    let cargo_name = std::env::var("CARGO_PKG_NAME").expect("CARGO_PKG_NAME not set");
    let cargo_version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set");

    assert_eq!(
        meta_name, cargo_name,
        "meta.json's \"name\" ({meta_name:?}) does not match Cargo.toml's package name ({cargo_name:?}). \
         meta.json is this project's single source of truth for the binary name and version - update whichever one is stale."
    );
    assert_eq!(
        meta_version, cargo_version,
        "meta.json's \"version\" ({meta_version:?}) does not match Cargo.toml's package version ({cargo_version:?}). \
         meta.json is this project's single source of truth for the binary name and version - update whichever one is stale."
    );
}
