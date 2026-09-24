# Status

Written for a follow-up testing pass. Everything below was verified with `cargo build`/`clippy`/`test` and pty-driven interactive sessions: a real terminal size, real keystrokes, a real terminal emulator to render the output, and in several cases the real Omarchy config and `ftctl` daemon on this machine.

Five commits are pushed to `master` at the GitHub repo, from `3ad4aca` (the original build plan) through `4a63201`.

## What's implemented

- Full UI layer (`src/app.rs`, `src/ui/`): sidebar, file list, preview + stats panel, status bar, inline prompts, help overlay. The backend (`config`, `core`, `fs`, `preview`, `theme`) existed and compiled cleanly before this work started; the UI layer was the missing piece.
- All keybindings from `BUILD_PLAN.md` section 4, plus:
  - `O`: "open with", prompts for a command line and backgrounds it against the focused entry.
  - `e` tries `nvim` first (fim's preferred default), offers to install it via `sudo pacman -S neovim` if missing, and falls back to a whitespace-split `$EDITOR` command line if declined.
- ANSI color parsing (`src/ui/ansi.rs`) so `chafa`/`bat` output renders with real color. It fully consumes OSC/DCS/APC/PM/SOS sequences end to end, so a stray escape payload (e.g. Sixel data) can never leak onto screen as literal text. 9 unit tests.
- Real terminal graphics for image/video previews: the Kitty graphics protocol on terminals that identify themselves via environment variables (Kitty, Ghostty, WezTerm), and Sixel graphics on Omarchy's actual default terminal, `foot`, identified via the local `omarchy default terminal` CLI helper (foot sets no distinguishing environment variable of its own; it even unsets `TERM_PROGRAM` to avoid being misdetected). Wrapped for tmux passthrough (`--passthrough=tmux`) when `$TMUX` is set. None of this performs a live terminal query. Character art (`chafa --format symbols --stretch`) is the fallback everywhere else. The image is written directly to the terminal, bypassing ratatui's cell buffer for that region, and cleared whenever the focused entry or the pane's position changes.
- Hand-drawn, theme-colored, block-letter ASCII glyphs (`src/preview/glyph.rs`, styled after this project's Omarchy screensaver branding) for disc images, a wide range of archive/package formats, `.torrent` files, and OpenDocument/Microsoft Office documents, centered and scaled to fill the pane.
- Downloads sidebar section; sidebar and help overlay (`?`) both restyled with grouped sections and a highlighted current location.
- `install.sh`/`uninstall.sh` detect, install (clone and run its own installer), and remove the `ftctl`/`filetransferd` plugin.
- README (with a real screenshot), CHANGELOG, MIT LICENSE.
- `meta.json` as the single source of truth for the project's name and version, with a `build.rs` check that fails the build if Cargo.toml's own fields drift from it.

## Bugs found and fixed (all via live testing against this machine's real environment)

1. **Theme never updated on `R` / theme change.** Read `$XDG_CONFIG_HOME/omarchy/shell.toml`, which on this Omarchy install only holds font settings. Fixed to read `$XDG_STATE_HOME/omarchy/current/theme/colors.toml`, the file `omarchy theme set` actually rewrites, with `shell.toml` as a secondary fallback and `[theme] source` from config.toml honored.
2. **`R` (refresh) corrupted the display.** Diagnostic `eprintln!` calls wrote to stderr while the TUI held the alternate screen. Now silent.
3. **`ftctl list` never worked.** Called with a `--json` flag the real binary rejects; it emits JSON by default. The flag was dropped.
4. **`$EDITOR`/"open with" values with flags failed to resolve.** Omarchy's `omarchy-launch-editor --inline`, for example, was treated as one bare binary name. Now parsed as a whitespace-split command line.
5. **Images rendered as garbled text (raw Sixel escape data).** `chafa`'s format auto-detection could pick sixels even though its stdout was piped to fim, not connected to the real terminal. Fixed by pinning an explicit `--format` on every call.
6. **That garbled data also corrupted a rename prompt** with what turned out to be a leaked OSC 10/11 color-query response (`rgb:RRRR/GGGG/BBBB`). `chafa` defaults to probing the terminal directly over its inherited stdin, the real controlling terminal, racing fim's own key-event reader. Fixed with `--probe=off` and by explicitly nulling stdin on every subprocess this project spawns that doesn't need real terminal interaction (`chafa`, `bat`, `pdftotext`, `ffmpegthumbnailer`, `file`, `trash-put`, `ftctl`).
7. **The preview pane was capped at a fixed 60% height** regardless of terminal size, leaving most of a tall terminal's extra space unused. Stats now gets a fixed compact height; preview gets the rest.
8. **Glyphs and character-art images only used a small corner of the (now much taller) preview pane.** Glyphs are centered and scaled; character art gets `--stretch` to fill the box.
9. **Terminal-graphics images stacked and stayed on screen after moving to the next file.** Sixel paints pixels directly into the cells it covers; it has no separate compositing layer, so a differently-sized new image left the old one's pixels behind wherever the new image didn't reach. Every image transition now clears the previous image's area first.
10. **Images overflowed past the preview pane's edges.** They were requested from `chafa` sized for the pane's outer rect (border included) but blitted starting just inside that border, so every image rendered 2 columns and 2 rows larger than the space it was drawn into. Requests now use the pane's inner content dimensions.

## Known gaps

- **Visual confirmation of real terminal graphics still depends on the user's own screen.** The graphics path is verified at the byte level: real Sixel DCS output was produced for a synthetic test image, correctly wrapped in tmux's passthrough escaping, with no panics or corruption around it. This sandbox cannot render a real terminal, so the actual on-screen appearance, sizing, and clearing behavior described above were confirmed only after the user reported and reproduced each issue on their own machine.
- **Mouse is not wired up.** `BUILD_PLAN.md` mentions "or click" for sidebar jump; only the keyboard path (`1`-`9`) is implemented.
- **The transfer daemon (`ftctl`/`filetransferd`) appears to run with systemd `PrivateTmp`** or similar isolation: `ftctl enqueue` against a source under `/tmp` fails with "source does not exist" even though the file is genuinely there, while real jobs against `$HOME`/`/run/media/...` paths succeed. Test paste/copy/move against a real home-directory path.
- **`config.toml`'s `[theme] source = "path"` variant** is wired up but has only been checked by reading the code; it still needs a live test with an actual custom theme file.

## Security review

A full-diff security review (subagent-driven vulnerability scan plus an independent false-positive filtering pass, per this repo's `security-review` skill) has run three times across this work and found no findings that met the reporting bar:

- The initial implementation: one candidate finding, an unpinned third-party install script (`install.sh` clones and runs `omarchy-transfer-manager`'s own installer with no commit/tag pinning). The filtering pass scored it 3/10: it is disclosed, consensual installation of a same-author companion project the user explicitly confirms via a prompt, the same pattern as rustup, Homebrew taps, or oh-my-zsh's installer.
- A clippy cleanup pass (Default derives, `div_ceil`, `strip_prefix`, a `&mut [Entry]` slice parameter, `io::Error::other`, a redundant reference removal, and `reader.lines().map_while(Result::ok)` replacing `.flatten()` in the `/etc/passwd`/`/etc/group` UID/GID lookup): no functional regressions. One behavioral nuance worth naming: `.flatten()` would skip an unreadable line and keep scanning the rest of the file; `map_while` stops there and gives up. Clippy's own suggestion trades a theoretical infinite-loop guard for that theoretical early stop, on a file type that's essentially always well-formed ASCII in practice. Left as clippy suggested it.
- The Sixel/tmux/`omarchy`-detection graphics code, including the new `omarchy` subprocess call and the raw-byte terminal write: no findings.

## Suggested test checklist

- [ ] `cargo build --release` and `cargo clippy` clean (0 warnings as of this update).
- [ ] `cargo test` (13 unit tests, all passing).
- [ ] Launch `fim` in a real terminal: navigate, toggle hidden/sort, search, goto path.
- [ ] Rename/mkdir/touch/symlink/trash/permanent-delete in a scratch directory.
- [ ] Copy/cut/paste against a real `ftctl` install, targeting a real (non-`/tmp`) destination.
- [ ] Preview a text file, an image (check whether it's a real picture via Kitty/Sixel or character art, depending on your terminal), a video, a PDF, a `.iso`/`.zip`/`.torrent`/`.ods`, and an arbitrary binary. Navigate between several images in a row and confirm nothing stacks, overflows, or lingers.
- [ ] Press `R` repeatedly and confirm the screen never corrupts, and that it reflects a real Omarchy theme change (`omarchy theme set <name>` then `R`).
- [ ] Press `e` with `nvim` both present and (in a disposable environment) absent; press `O` and open something with an arbitrary command.
- [ ] Open the help overlay (`?`) and confirm the two-column layout reads cleanly at your terminal's actual size.
- [ ] Run `install.sh` and `uninstall.sh` end to end, including the `ftctl` plugin install/removal path, in a disposable environment.
- [ ] `fim --version` prints the same version as `meta.json` and Cargo.toml.
