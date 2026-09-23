# Status

Written for a follow-up testing pass. Everything below was verified in this session with `cargo build`/`clippy`/`test` and pty-driven interactive sessions (a real terminal size, real keystrokes, and a real terminal emulator to render the output) rather than assumed from reading the code.

## What's implemented

- Full UI layer (`src/app.rs`, `src/ui/`): sidebar, file list, preview + stats panel, status bar, inline prompts, help overlay. This was the only missing piece; the backend (`config`, `core`, `fs`, `preview`, `theme`) already existed and compiled cleanly.
- All keybindings from `BUILD_PLAN.md` section 4, plus two additions made mid-session at the user's request:
  - `O`: "open with", prompts for a command line and backgrounds it against the focused entry.
  - `e` now tries `nvim` first (fim's preferred default), offers to install it via `sudo pacman -S neovim` if missing, and falls back to `$EDITOR` (correctly parsed as a command line, not a bare binary name) if the user declines.
- ANSI color parsing (`src/ui/ansi.rs`) so `chafa`/`bat` output renders with real color instead of flat monochrome text. Covered by 6 unit tests (truecolor, 256-color, basic/bright SGR codes, modifiers, malformed/non-SGR escape sequences).
- Hand-drawn, theme-colored ASCII glyphs for disc images and archives (`src/preview/glyph.rs`), shown instead of a hex dump.
- `install.sh`/`uninstall.sh` now detect, install (clone + run its own installer), and remove the `ftctl`/`filetransferd` plugin.

## Bugs found and fixed this session (all via live testing, not code review)

1. **Theme never updated on `R` / theme change.** `theme::load()` read `$XDG_CONFIG_HOME/omarchy/shell.toml`, which on a real Omarchy install only holds font settings. The live theme colors actually live at `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` (what `omarchy theme set` rewrites). Fixed, with `shell.toml` kept as a secondary fallback and `[theme] source` from config.toml now honored.
2. **`R` (refresh) corrupted the display.** Several `eprintln!` diagnostics (in theme loading and recents persistence) wrote straight to stderr while the TUI held the alternate screen in raw mode, garbling the screen. Confirmed reproducible on the real Omarchy config (which is missing color fields the theme parser warned about on every load) and confirmed fixed with a 5x-repeated-`R` pty test showing zero leaked diagnostic text.
3. **`ftctl list` never worked.** The code called `ftctl list --json`; the real installed `ftctl` binary rejects that flag outright (`unrecognized arguments: --json`) and emits JSON by default anyway. Transfer-job status in the stats panel was silently broken before this fix.
4. **`$EDITOR` with flags would fail to resolve.** `$EDITOR` was treated as one bare binary name. On this machine `$EDITOR=omarchy-launch-editor --inline`, a command with a flag, which would have failed to resolve entirely. Now parsed as a whitespace-split command line (used for both `$EDITOR` and "open with").

## Known gaps / things I could not verify

- **No real terminal graphics (Kitty/Sixel/iTerm2 image protocol).** Image/video previews use `chafa`'s character-block output (now correctly colored), not actual pixel graphics. True graphics would mean bypassing ratatui's cell renderer and writing raw escape sequences directly to the terminal at the preview pane's coordinates - a legitimate technique, but `chafa` isn't installed on this dev machine, so I could not build or verify it. Flagging this rather than shipping unverified terminal-control code.
- **Mouse is not wired up.** `BUILD_PLAN.md` mentions "or click" for sidebar jump; only the keyboard path (`1`-`9`) is implemented.
- **The transfer daemon (`ftctl`/`filetransferd`) appears to run with systemd `PrivateTmp`** (or similar isolation) on this machine: `ftctl enqueue` against a source under `/tmp` fails with "source does not exist" even though the file is genuinely there, while real jobs against `$HOME`/`/run/media/...` paths succeed (confirmed via the daemon's own job history). Test paste/copy/move against a real home-directory path, not `/tmp`.
- **`config.toml`'s `[theme] source = "path"` variant** is now wired up but only exercised via code reading, not a live test with an actual custom theme file.
- I did not commit anything. `git status` shows `BUILD_PLAN.md` modified and everything else (the entire UI layer, `Cargo.toml`, install/uninstall/update scripts, `README.md`, `CHANGELOG.md`, `LICENSE`, this file) untracked.

## Suggested test checklist

- [ ] `cargo build --release` and `cargo clippy` clean (they were, at the end of this session; 17 pre-existing clippy warnings remain in backend files this session didn't touch, none in `app.rs`/`ui/`/the new modules).
- [ ] `cargo test` (10 unit tests, all passing at the end of this session).
- [ ] Launch `fim` in a real terminal (not just pty-captured): navigate, toggle hidden/sort, search, goto path.
- [ ] Rename/mkdir/touch/symlink/trash/permanent-delete in a scratch directory.
- [ ] Copy/cut/paste against a real `ftctl` install, targeting a real (non-`/tmp`) destination.
- [ ] Preview a text file (with and without `bat` installed), an image (with `chafa` installed - not available in this dev sandbox), a video (with `ffmpegthumbnailer`), a PDF, a `.iso`/`.zip`, and an arbitrary binary.
- [ ] Press `R` repeatedly and confirm the screen never corrupts, and that it actually reflects a real Omarchy theme change (`omarchy theme set <name>` then `R`).
- [ ] Press `e` with `nvim` both present and (in a disposable environment) absent, to exercise the install-prompt path; press `O` and open something with an arbitrary command.
- [ ] Run `install.sh` and `uninstall.sh` end to end, including the `ftctl` plugin install/removal path, in a disposable environment (they clone a real git repo and run its installer/uninstaller).
