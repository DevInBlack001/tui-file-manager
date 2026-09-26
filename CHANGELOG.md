# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.3.0] - 2026-09-26

### Added

- `Tab` now cycles five layout combinations instead of just left/right: sidebar left, sidebar right, sidebar top (spanning the width, filelist/preview below it), sidebar hidden, and preview hidden. Config key renamed `ui.sidebar_position` -> `ui.layout_mode` (`left | right | top | no_sidebar | no_preview`).
- Five more file list view modes alongside the original table, cycled with `v` (`ui.view_mode`): **compact** (name only), **detailed** (adds permissions/owner columns), **grid** (icon grid, no metadata), **tree** (`z` peeks a directory's immediate children inline without navigating into it), and **columns** (parent directory alongside the current one; the preview pane lists a focused subdirectory's contents instead of just a summary, ranger/Finder-style).
- "Open with" (`O`) now offers a picker of installed applications discovered from `.desktop` `MimeType=` associations that claim to handle the focused file's type, before falling back to (`/`) a free-text command. This surfaces Wine/Proton-wrapped apps for free, since Lutris/Bottles/Heroic/Wine installers already register ordinary `.desktop` entries.

### Fixed

- The sidebar now auto-scrolls its highlighted entry into view. `LayoutMode::Top`'s shorter sidebar could otherwise leave later bookmarks permanently off-screen with no way to reach them.
- Tree view's peeked children now use `theme.cyan` instead of `theme.blue` + the `DIM` modifier: `blue` resolves to the same RGB as `accent` (already used for borders) in both shipped themes, so the previous choice read as low-contrast/blended with the border rather than as a distinct color.

## [0.2.0] - 2026-09-26

### Added

- `install.sh` writes a standard XDG desktop entry (`~/.local/share/applications/fim.desktop`) so fim shows up in app launchers and menus, including Omarchy's own app search; `uninstall.sh` removes it.
- Nerd Font type icons for directories and files in the file list and sidebar, toggled with `ui.show_icons`. Codepoints were verified against a real installed font's cmap rather than assumed from memory.
- Configurable sidebar row spacing (`ui.sidebar_row_spacing`) and sidebar position (`ui.sidebar_position`, `left` or `right`).
- PKGBUILD and `.SRCINFO` for AUR packaging as `tui-file-manager`.

## [0.1.0] - 2026-09-24

Initial release.

### Added

- Full TUI: app state machine, sidebar, file list, preview + stats panel, status bar, inline prompts, help overlay.
- Navigation: directory entry/parent, home, goto-path prompt, sidebar bookmark jump (`1`-`9`), search/filter, sort cycling, hidden-file toggle.
- Selection, clipboard (copy/cut/paste via `ftctl`), trash, permanent delete with typed confirmation, rename, mkdir, touch, symlink creation.
- Opening files: `e` tries `nvim` first, offering to install it via `sudo pacman -S neovim` if missing and falling back to `$EDITOR`; `o` opens via `xdg-open`; `O` prompts for an arbitrary "open with" command.
- Recents view backed by a persisted recent-paths list.
- Live transfer-job status in the stats panel, polled from `ftctl list` at most once every 2 seconds.
- Real terminal graphics for image/video previews: the Kitty graphics protocol on Kitty/Ghostty/WezTerm, and Sixel graphics on Omarchy's default terminal, `foot`. Both are detected purely from environment variables the terminal sets at startup, or by asking the local `omarchy default terminal` CLI helper what's configured - never by querying the terminal itself. Graphics are wrapped for tmux passthrough when `$TMUX` is set. Every other terminal falls back to character art (chafa `--format symbols --stretch`, filling the full preview pane). See [TERMINAL_GRAPHICS.md](TERMINAL_GRAPHICS.md) for the debugging trail behind this detection chain.
- An ANSI-to-ratatui-span parser (`src/ui/ansi.rs`) so `chafa`/`bat` output renders with its real colors.
- Hand-drawn, theme-colored, block-letter ASCII glyphs (matching this project's Omarchy screensaver branding style) for disc images, a wide range of archive/package formats, `.torrent` files, OpenDocument/Microsoft Office documents, and OpenVPN configs. These are centered and scaled up to fill the preview pane.
- `.ovpn` files never show their actual content in preview: OpenVPN configs commonly embed certificates, private keys, or credentials inline, so a VPN glyph replaces the text preview unconditionally.
- A Downloads sidebar section.
- `install.sh`/`uninstall.sh` detect, install, and remove the `ftctl`/`filetransferd` file-transfer plugin, since fim cannot copy or move anything without it.
- README (with a real screenshot), CHANGELOG, and MIT LICENSE.
- `meta.json` as the single source of truth for the project's name and version, with a `build.rs` check that keeps Cargo.toml's own name/version fields from drifting out of sync with it, and a `--version`/`-V` flag that prints both.

### Fixed

Bugs found through live pty-driven testing against a real Omarchy install and a real `ftctl` daemon:

- Theme loading read `$XDG_CONFIG_HOME/omarchy/shell.toml`, which only ever held font settings on a real Omarchy install. It now reads the live theme at `$XDG_STATE_HOME/omarchy/current/theme/colors.toml`, the file `omarchy theme set` actually rewrites, with `shell.toml` kept as a secondary fallback candidate. `[theme] source` in config.toml is now honored.
- Several internal diagnostic `eprintln!` calls wrote directly to stderr while the TUI held the alternate screen, corrupting the display on every use (most visibly on `R`/refresh). These are now silent.
- `ftctl list` was called with a `--json` flag the real `ftctl` binary rejects outright; `ftctl list` already emits JSON on its own, so the flag was simply dropped.
- `$EDITOR` and "open with" commands are now parsed as a command line and split on whitespace before resolving the binary. A value like Omarchy's `omarchy-launch-editor --inline` previously failed to resolve at all, because the whole string was treated as one bare binary name.
- `chafa` was invoked with no `--format`, so its own format auto-detection could pick sixel or kitty graphics protocol output; captured as a plain string, that raw escape data rendered as garbled text. `chafa` is now pinned to an explicit format (`symbols`, `kitty`, or `sixels`) on every call. The ANSI parser (`src/ui/ansi.rs`) was hardened to fully consume any OSC/DCS/APC/PM/SOS escape sequence it encounters end to end (it previously only stripped the opening bytes), so a payload like this can never leak onto the screen as literal text again.
- `chafa` also defaults to probing the terminal (`--probe=auto`): writing a query and waiting up to 5 seconds for a response, over a controlling-terminal file descriptor it inherited from fim because that subprocess's stdin was left unredirected. That query's response raced fim's own key-event reader and could leak raw `rgb:RRRR/GGGG/BBBB` bytes into whatever text prompt (e.g. rename) happened to be focused at the time. Fixed with `--probe=off`, and by setting `.stdin(Stdio::null())` on every subprocess fim spawns that doesn't need real terminal interaction (`chafa`, `bat`, `pdftotext`, `ffmpegthumbnailer`, `file`, `trash-put`, `ftctl`), as defense in depth against the same class of bug recurring.
- The preview pane was capped at a fixed 60% of the right column's height, with the remaining 40% always reserved for the stats panel even though stats only ever renders a fixed, small number of fields. The stats panel now gets a fixed compact height and the preview pane gets all remaining vertical space.
- Terminal-graphics images stacked on top of each other and stayed on screen after navigating to the next file. Sixel graphics have no separate compositing layer the way the Kitty protocol does; they paint pixels directly into the cells they cover, so drawing a new, possibly differently-sized image only overwrote the cells it actually touched, leaving the previous image's pixels sitting in the cells around it. Every transition away from a shown image (to a different image, or to non-graphics content) now clears that image's full area first.
- Images were requested from `chafa` sized for the preview pane's outer rect, border included, but blitted starting just inside that border. Every image was rendered 2 columns and 2 rows larger than the space it was drawn into and overflowed past the pane's edges; because that overflow sat outside the area the fix above clears, the overflowing strip stayed on screen even after navigating away. Preview requests now use the pane's inner content dimensions, matching exactly where the image gets drawn.

### Changed

- Applied a round of clippy-suggested cleanup: `#[derive(Default)]` with `#[default]` variants replacing several manual `impl Default` blocks, `div_ceil` replacing a hand-written ceiling-division calculation, `strip_prefix` replacing `starts_with` plus manual slicing, a `&mut [Entry]` slice parameter replacing `&mut Vec<Entry>`, `io::Error::other`, and a redundant reference removal. A security review of the diff found no findings and no behavioral changes.
