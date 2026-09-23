#!/usr/bin/env bash
# installed by fim install.sh
# install.sh - Build and install the fim binary, write default config,
# and report optional dependency status.
set -euo pipefail

# ---------------------------------------------------------------------------
# Resolve the repo root from this script's location (no hardcoded paths).
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---------------------------------------------------------------------------
# Flags
# ---------------------------------------------------------------------------
AUTO_YES=false
for arg in "$@"; do
    case "${arg}" in
        --yes|-y) AUTO_YES=true ;;
        *) ;;
    esac
done

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
info()    { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()      { printf '\033[1;32m[ ok ]\033[0m  %s\n' "$*"; }
warn()    { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
err()     { printf '\033[1;31m[err ]\033[0m  %s\n' "$*" >&2; }
die()     { err "$*"; exit 1; }

ask_yn() {
    local prompt="$1"
    if [[ "${AUTO_YES}" == true ]]; then
        return 0
    fi
    local reply
    read -r -p "${prompt} [y/N] " reply
    [[ "${reply}" =~ ^[Yy]$ ]]
}

# ---------------------------------------------------------------------------
# 1. Check cargo
# ---------------------------------------------------------------------------
if ! command -v cargo &>/dev/null; then
    die "cargo not found. Install Rust via: rustup (https://rustup.rs) or pacman -S rust"
fi

# ---------------------------------------------------------------------------
# 2. Check rustc >= 1.80
# ---------------------------------------------------------------------------
if ! command -v rustc &>/dev/null; then
    die "rustc not found. Install Rust via: rustup or pacman -S rust"
fi

RUSTC_VERSION="$(rustc --version 2>/dev/null | awk '{print $2}')"
# Parse major.minor from "1.80.0" or "1.80.0-nightly" etc.
RUSTC_MAJOR="$(printf '%s' "${RUSTC_VERSION}" | cut -d. -f1)"
RUSTC_MINOR="$(printf '%s' "${RUSTC_VERSION}" | cut -d. -f2 | grep -o '^[0-9]*')"

if [[ -z "${RUSTC_MAJOR}" || -z "${RUSTC_MINOR}" ]]; then
    warn "Could not parse rustc version '${RUSTC_VERSION}'; proceeding anyway."
elif [[ "${RUSTC_MAJOR}" -lt 1 ]] || \
     [[ "${RUSTC_MAJOR}" -eq 1 && "${RUSTC_MINOR}" -lt 80 ]]; then
    die "rustc ${RUSTC_VERSION} is too old; fim requires >= 1.80. Run: rustup update"
else
    ok "rustc ${RUSTC_VERSION} (>= 1.80)"
fi

# ---------------------------------------------------------------------------
# 3. Auto-install chafa via pacman (Arch only)
# ---------------------------------------------------------------------------
if ! command -v chafa &>/dev/null; then
    if command -v pacman &>/dev/null; then
        info "chafa not found - installing via pacman..."
        pacman -S --noconfirm chafa
        ok "chafa installed"
    else
        warn "chafa not found. Image preview will be unavailable."
        warn "Install manually: https://hpjansson.org/chafa/"
    fi
else
    ok "chafa found ($(command -v chafa))"
fi

# ---------------------------------------------------------------------------
# 4. Advisory check for optional tools
# ---------------------------------------------------------------------------
OPTIONAL_MISSING=()

check_optional() {
    local cmd="$1"
    local pkg="$2"
    if command -v "${cmd}" &>/dev/null; then
        ok "${cmd} found ($(command -v "${cmd}"))"
    else
        warn "${cmd} not found  ->  pacman -S ${pkg}"
        OPTIONAL_MISSING+=("${pkg}")
    fi
}

info "Checking optional tools..."
check_optional ffmpegthumbnailer ffmpegthumbnailer
check_optional bat               bat
check_optional trash             trash-cli

# ---------------------------------------------------------------------------
# 5. Install the file-transfer plugin (ftctl/filetransferd) - fim delegates
#    ALL copy/move operations to it, so without it paste is a no-op.
# ---------------------------------------------------------------------------
FTCTL_REPO="https://github.com/DevInBlack001/omarchy-transfer-manager.git"
FTCTL_CLONE_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}/tui-fm/file-transfer"

resolve_ftctl() {
    if [[ -n "${TFM_FTCTL_PATH:-}" && -f "${TFM_FTCTL_PATH}" ]]; then
        printf '%s' "${TFM_FTCTL_PATH}"
        return 0
    fi
    if [[ -f "${HOME}/.local/bin/ftctl" ]]; then
        printf '%s' "${HOME}/.local/bin/ftctl"
        return 0
    fi
    command -v ftctl 2>/dev/null
}

if FTCTL_BIN="$(resolve_ftctl)" && [[ -n "${FTCTL_BIN}" ]]; then
    ok "ftctl found (${FTCTL_BIN})"
elif ask_yn "ftctl not found. Install the file-transfer plugin (${FTCTL_REPO})?"; then
    if ! command -v git &>/dev/null; then
        warn "git not found; cannot install the file-transfer plugin automatically."
        warn "Install manually: ${FTCTL_REPO}"
    else
        if [[ -d "${FTCTL_CLONE_DIR}/.git" ]]; then
            info "Updating existing checkout at ${FTCTL_CLONE_DIR}..."
            git -C "${FTCTL_CLONE_DIR}" pull --ff-only
        else
            info "Cloning ${FTCTL_REPO}..."
            mkdir -p "$(dirname "${FTCTL_CLONE_DIR}")"
            git clone "${FTCTL_REPO}" "${FTCTL_CLONE_DIR}"
        fi
        info "Running the file-transfer plugin's own installer (requires python3 + rsync)..."
        if [[ "${AUTO_YES}" == true ]]; then
            "${FTCTL_CLONE_DIR}/install.sh" --yes
        else
            "${FTCTL_CLONE_DIR}/install.sh"
        fi
        ok "file-transfer plugin installed"
    fi
else
    warn "Skipped. Paste (copy/move) will show 'ftctl not found' until it's installed: ${FTCTL_REPO}"
fi

# ---------------------------------------------------------------------------
# 6. Build the release binary
# ---------------------------------------------------------------------------
info "Building fim (release)..."
cargo build --release --manifest-path "${SCRIPT_DIR}/Cargo.toml"
ok "Build complete"

BINARY="${SCRIPT_DIR}/target/release/fim"
if [[ ! -f "${BINARY}" ]]; then
    die "Expected binary not found at ${BINARY}"
fi

# ---------------------------------------------------------------------------
# 7. Install the binary
# ---------------------------------------------------------------------------
INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
mkdir -p "${INSTALL_DIR}"

DEST="${INSTALL_DIR}/fim"
if [[ -f "${DEST}" ]]; then
    info "Replacing existing fim at ${DEST}"
fi
cp "${BINARY}" "${DEST}"
ok "Installed: ${DEST}"

# ---------------------------------------------------------------------------
# 8. Write default config (only if not present)
#
# Kept in sync with Config's own first-run default in src/config/mod.rs
# (default_toml_content()) - fim writes the same content itself on first
# run, so this only matters when install.sh runs before fim ever has.
# ---------------------------------------------------------------------------
CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/tui-fm"
CONFIG_FILE="${CONFIG_DIR}/config.toml"

if [[ -f "${CONFIG_FILE}" ]]; then
    info "Config already present, skipping: ${CONFIG_FILE}"
else
    mkdir -p "${CONFIG_DIR}"
    cat > "${CONFIG_FILE}" <<'EOF'
# fim configuration
# https://github.com/DevInBlack001/tui-file-manager

[ui]
show_hidden       = false
sort_key          = "name"    # name | size | mtime | type
sort_reverse      = false
sidebar_width_pct = 18
preview_width_pct = 36

[preview]
enabled          = true
max_text_lines   = 200
max_binary_bytes = 512
image_renderer   = "auto"
video_thumbs     = true

[theme]
source = "auto"

[bookmarks]
# work_dir = "~/Work"
custom = []

[transfer]
# ftctl_path = "~/.local/bin/ftctl"

[recents]
max_entries = 50
EOF
    ok "Default config written: ${CONFIG_FILE}"
fi

# ---------------------------------------------------------------------------
# 9. Summary
# ---------------------------------------------------------------------------
printf '\n'
ok "fim installation complete."
info "Binary  : ${DEST}"
info "Config  : ${CONFIG_FILE}"
if FTCTL_BIN="$(resolve_ftctl)" && [[ -n "${FTCTL_BIN}" ]]; then
    info "ftctl   : ${FTCTL_BIN}"
else
    warn "ftctl   : not installed - copy/move will be unavailable until it is"
fi

if [[ "${#OPTIONAL_MISSING[@]}" -gt 0 ]]; then
    warn "Optional deps not installed: ${OPTIONAL_MISSING[*]}"
    warn "To install all optional deps: pacman -S ${OPTIONAL_MISSING[*]}"
fi

if [[ ":${PATH}:" != *":${INSTALL_DIR}:"* ]]; then
    warn "${INSTALL_DIR} is not in your PATH."
    warn "Add to your shell config:  export PATH=\"\${HOME}/.local/bin:\${PATH}\""
fi
