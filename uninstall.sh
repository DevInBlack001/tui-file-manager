#!/usr/bin/env bash
# uninstall.sh - Remove the binary named in meta.json, optionally config and
# runtime state.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Name/version - meta.json is this project's single source of truth (see
# build.rs, which fails the Rust build if Cargo.toml's own name/version
# fields ever disagree with it).
read_meta() {
    grep -o "\"$1\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" "${SCRIPT_DIR}/meta.json" \
        | sed -E 's/.*:[[:space:]]*"([^"]*)"/\1/'
}
NAME="$(read_meta name)"

AUTO_YES=false
for arg in "$@"; do
    case "${arg}" in
        --yes|-y) AUTO_YES=true ;;
        *) ;;
    esac
done

info() { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()   { printf '\033[1;32m[ ok ]\033[0m  %s\n' "$*"; }
warn() { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }

ask_yn() {
    local prompt="$1"
    if [[ "${AUTO_YES}" == true ]]; then
        return 0
    fi
    local reply
    read -r -p "${prompt} [y/N] " reply
    [[ "${reply}" =~ ^[Yy]$ ]]
}

INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/tui-fm"
STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/tui-fm"

# 1. Remove binary
if [[ -f "${INSTALL_DIR}/${NAME}" ]]; then
    if ask_yn "Remove binary at ${INSTALL_DIR}/${NAME}?"; then
        rm -f "${INSTALL_DIR}/${NAME}"
        ok "Removed ${INSTALL_DIR}/${NAME}"
    else
        info "Skipped removing binary"
    fi
else
    info "No binary found at ${INSTALL_DIR}/${NAME}"
fi

# 2. Remove configuration
if [[ -d "${CONFIG_DIR}" ]]; then
    info "Config directory found at ${CONFIG_DIR} with contents:"
    ls -la "${CONFIG_DIR}" || true
    if ask_yn "Remove configuration directory ${CONFIG_DIR}?"; then
        rm -rf "${CONFIG_DIR}"
        ok "Removed ${CONFIG_DIR}"
    else
        info "Kept configuration directory"
    fi
fi

# 3. Remove state
if [[ -d "${STATE_DIR}" ]]; then
    info "State directory found at ${STATE_DIR} with contents:"
    ls -la "${STATE_DIR}" || true
    if ask_yn "Remove state directory ${STATE_DIR}?"; then
        rm -rf "${STATE_DIR}"
        ok "Removed ${STATE_DIR}"
    else
        info "Kept state directory"
    fi
fi

# 4. Remove the file-transfer plugin (ftctl/filetransferd), but only the
# checkout install.sh itself made - never an ftctl the user set up some
# other way (a different clone, a system package, etc).
FTCTL_CLONE_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}/tui-fm/file-transfer"
if [[ -d "${FTCTL_CLONE_DIR}" ]]; then
    info "file-transfer plugin checkout found at ${FTCTL_CLONE_DIR}"
    if ask_yn "Run its uninstaller and remove this checkout?"; then
        if [[ -x "${FTCTL_CLONE_DIR}/uninstall.sh" ]]; then
            if [[ "${AUTO_YES}" == true ]]; then
                "${FTCTL_CLONE_DIR}/uninstall.sh" --yes || warn "file-transfer uninstaller exited non-zero; continuing"
            else
                "${FTCTL_CLONE_DIR}/uninstall.sh" || warn "file-transfer uninstaller exited non-zero; continuing"
            fi
        fi
        rm -rf "${FTCTL_CLONE_DIR}"
        ok "Removed ${FTCTL_CLONE_DIR}"
    else
        info "Kept the file-transfer plugin checkout"
    fi
fi

ok "Uninstallation steps completed."
