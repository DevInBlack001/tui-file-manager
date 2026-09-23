#!/usr/bin/env bash
# installed by fim install.sh
# update.sh - Re-build and re-install fim from the current repository state.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

info() { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
warn() { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }

info "Remember to fetch the newest changes first if desired: git pull --ff-only"
info "Running installer..."

exec "${SCRIPT_DIR}/install.sh" "$@"
