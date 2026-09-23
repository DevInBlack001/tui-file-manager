# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Full TUI event loop and app state machine (`src/app.rs`), wiring up the previously-implemented backend (config, core, fs, preview, theme) to a working ratatui interface.
- Sidebar, file list, preview + stats, status bar, and inline prompt widgets (`src/ui/`).
- Help overlay (`?`).
- Navigation: directory entry/parent, home, goto-path prompt, sidebar bookmark jump (`1`-`9`), search/filter, sort cycling, hidden-file toggle.
- Selection, clipboard (copy/cut/paste via `ftctl`), trash, permanent delete with typed confirmation, rename, mkdir, touch, symlink creation.
- Open the focused file in `nvim` by default, offering to install it via `sudo pacman -S neovim` if missing before falling back to `$EDITOR`; open via `xdg-open`; "open with" for an arbitrary command.
- Recents view backed by the persisted recent-paths list.
- Live transfer-job status in the stats panel, polled from `ftctl list` at most once every 2 seconds.
- ANSI-to-ratatui-span parser (`src/ui/ansi.rs`) so `chafa`/`bat` output renders with its real colors instead of flat monochrome text.
- Hand-drawn, theme-colored, block-letter ASCII glyphs (matching this project's Omarchy screensaver branding style) for disc images (`.iso`, `.img`, `.bin`, `.nrg`, `.mdf`, `.toast`, `.dmg`), a comprehensive set of archive/package formats (`.zip`, `.7z`, `.rar`, `.tar` and its compressed variants, `.jar`/`.war`/`.deb`/`.rpm`/`.apk` and other archive-based package formats, and more), and `.torrent` files, shown instead of a hex dump.
- Downloads sidebar section.
- `install.sh`/`uninstall.sh` now detect, install, and remove the `ftctl`/`filetransferd` file-transfer plugin (cloned to `$XDG_DATA_HOME/tui-fm/file-transfer`), since fim cannot copy or move anything without it.
- README (with a real screenshot of fim browsing its own repo), CHANGELOG, and MIT LICENSE.

### Fixed

- Theme loading pointed at `$XDG_CONFIG_HOME/omarchy/shell.toml`, which only ever held font settings on a real Omarchy install; it now reads the live theme at `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` (the file `omarchy theme set` actually rewrites), with `shell.toml` kept as a secondary fallback candidate. `[theme] source` in config.toml (`auto`/`builtin`/an explicit path) is now honored.
- Several internal diagnostic `eprintln!` calls (theme loading, recents persistence) wrote directly to stderr while the TUI held the alternate screen, corrupting the display on every use (most visibly on `R`/refresh). These are now silent, matching the rest of the codebase's graceful-fallback style.
- `ftctl list` was called with a `--json` flag the real `ftctl` binary rejects outright, so transfer-job status silently never worked; the flag is unneeded since `ftctl list` already emits JSON by default.
- `$EDITOR` (and "open with" commands) are now parsed as a command line and split on whitespace before resolving the binary, instead of treated as a single bare name; a value like Omarchy's `omarchy-launch-editor --inline` previously failed to resolve at all.
- `chafa` was invoked with no `--format`, so its format auto-detection could pick sixel/kitty graphics protocol output; the raw Sixel/DCS escape data then got rendered as garbled numeric text once captured as a string. `chafa` is now always invoked with `--format symbols`. The ANSI parser (`src/ui/ansi.rs`) was also hardened to fully consume, not just partially strip, any OSC/DCS/APC/PM/SOS escape sequence it does encounter, so a similar payload can never leak onto the screen as literal text again.
- `chafa` also defaults to *probing* the terminal (`--probe=auto`): querying it directly and waiting up to 5s for a response (e.g. an OSC 10/11 colour query), over a controlling-terminal file descriptor it inherited from us since we never redirected its stdin. That query's response raced directly against our own key-event reader and could leak raw `rgb:RRRR/GGGG/BBBB` bytes into whatever text prompt (e.g. rename) happened to be focused at the time. Fixed with `--probe=off` (we already pin `--format`/`--colors` explicitly, so probing had nothing left to contribute) and by explicitly setting `.stdin(Stdio::null())` on every subprocess this project spawns that doesn't need real terminal interaction (`chafa`, `bat`, `pdftotext`, `ffmpegthumbnailer`, `file`, `trash-put`, `ftctl`), as defense in depth against the same class of bug recurring.
- The preview pane was capped at a fixed 60% of the right column's height regardless of terminal size, with the remaining 40% always reserved for the stats panel even though stats only ever renders a fixed, small number of fields. The stats panel now gets a fixed compact height and the preview pane gets all remaining vertical space.

## [0.1.0] - Unreleased

Initial scaffolding: build plan, backend modules (`config`, `core`, `fs`, `preview`, `theme`), and install script, with a placeholder UI.
