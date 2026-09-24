# Terminal graphics: how fim detects what your terminal can do

This document explains a debugging trail from one session: images in the preview panel first rendered as garbled text, then as flat character art, and each step required understanding a different layer of the terminal/shell/multiplexer stack. It's written down here because the reasoning isn't obvious from the code alone, and the same class of bug is easy to reintroduce if someone "simplifies" this code later without knowing why it's shaped this way.

Nothing described here involved editing any of the user's own dotfiles or system config (`~/.tmux.conf`, `~/.config/foot/`, Omarchy's own config, etc). Those were only ever read, for investigation. Every change described below was made to this project's own source and scripts.

## Symptom 1: images rendered as garbled text

`chafa` (the tool fim shells out to for image previews) can output several formats: plain character art (`symbols`), or real pixel graphics via the Sixel or Kitty terminal protocols. Left to auto-detect, chafa picked a graphics protocol even though its stdout was piped into fim, not connected to a real terminal - fim was capturing that output as a string and displaying it as plain text, so a Sixel payload (which looks like `\eP0;1;0q"1;1;320;198#0;2;3;2;2#...`) showed up as literal characters on screen.

**Fix:** pin `chafa --format symbols` whenever fim isn't deliberately requesting real graphics (see below).

## Symptom 2: a rename prompt filled with garbage matching `rgb:RRRR/GGGG/BBBB`

This one took longer to find. `chafa` doesn't just auto-detect blindly - by default (`--probe=auto`) it actively **probes the terminal**: it writes a query escape sequence (in this case, an OSC 10/11 "what are your foreground/background colors" query) and waits up to 5 seconds for a response, to refine its format/color decisions.

The problem: chafa's subprocess inherits fim's stdin by default, which is the *real* controlling terminal. fim's own main loop is *simultaneously* reading key events from that same terminal via `crossterm`. Two independent readers on the same input stream is a race: whichever one happens to consume the terminal's query response wins. When fim's key-event reader won, it tried to interpret the response bytes (`rgb:d8d8/dede/e9e9`, etc.) as individual keystrokes - and if a text prompt (like rename) happened to be focused at that moment, every one of those bytes got typed into it.

**Fix, two parts:**
1. `--probe off` on every chafa invocation. We already pin `--format`/`--colors` explicitly, so chafa has nothing left to learn from probing.
2. `.stdin(Stdio::null())` on **every** subprocess fim spawns that doesn't need real terminal interaction: `chafa`, `bat`, `pdftotext`, `ffmpegthumbnailer`, `file`, `trash-put`, `ftctl`. This is defense in depth - even if some future flag or tool version tries to probe again, it can't reach the real terminal to do so.

**The general lesson:** any subprocess that inherits stdin can race your own input loop if it tries to read from the terminal. If a TUI spawns subprocesses at all, none of them should get real stdin unless they specifically need interactive terminal control (a foreground editor, `sudo` asking for a password) - and those cases should suspend the TUI's own input loop first, which is what `Action::RunForeground` does for `$EDITOR`.

## Symptom 3: character art looks bad and only fills part of the pane

Once the two bugs above were fixed, chafa's `symbols` mode was working correctly - it just isn't very good. Character-art image previews are inherently low-resolution (each "pixel" is a whole terminal cell), and chafa's aspect-preserving fit doesn't try to fill the requested box, so a wide photo in a tall pane leaves most of it blank.

**Fix:** `--stretch` to fill the pane, and this is also the point where real graphics protocols became worth pursuing properly instead of settling for character art everywhere.

## Finding out what terminal is actually in use

To show a *real* picture instead of character art, fim needs to know which graphics protocol (if any) the terminal understands, and it needs to know this **without querying the terminal** (that's exactly the mechanism that caused symptom 2).

The safe signal is environment variables the terminal sets at its own startup, inherited down through every child process:

| Terminal | Signal | Protocol |
|---|---|---|
| kitty | `$KITTY_WINDOW_ID` | Kitty graphics protocol |
| Ghostty | `$GHOSTTY_RESOURCES_DIR` | Kitty graphics protocol |
| WezTerm | `$WEZTERM_EXECUTABLE`, or `$TERM_PROGRAM=WezTerm` | Kitty graphics protocol |

This covers three popular terminals cleanly - but Omarchy's actual default terminal is **`foot`**, and foot sets none of these. Checking foot's own manual turned up why: foot deliberately **unsets** `$TERM_PROGRAM` (and `$TERM_PROGRAM_VERSION`) in every child process specifically to avoid other programs misdetecting it as something else. There is no environment variable that safely says "I am foot."

**Resolution:** ask the platform instead of the terminal. `omarchy default terminal` is a local CLI helper (part of Omarchy itself, not the terminal emulator) that reports which of a fixed set of terminals (`alacritty`/`foot`/`ghostty`/`kitty`) is configured as the system default. This is a hint, not a certainty - the terminal fim actually runs in could differ from the configured default - but it's a local command invocation, not a terminal query, so it carries none of the race risk from symptom 2. foot supports **Sixel** graphics (confirmed via `foot.ini`'s `sixel` option, on by default), so `foot` maps to `chafa --format sixels`.

`detect_graphics_format()` in `src/preview/image.rs` tries the environment-variable checks first, then falls back to the `omarchy` helper. Either miss falls back to character art - same as any other terminal fim doesn't recognize.

## Finding out about tmux

The user reported the fix still wasn't working, and testing turned up that this session's own shell was itself running inside tmux. That mattered for two separate reasons:

1. **`$TERM` gets masked.** Inside tmux, `$TERM` is normally `tmux-256color` (or similar), not the outer terminal's real value. Any detection strategy based on reading `$TERM` for a specific terminal name would silently break for every tmux user. (This is also why the Kitty-protocol environment variables above are checked directly rather than via `$TERM` - tmux passes through arbitrary environment variables from the process that started it, it only intercepts `$TERM` itself.)
2. **tmux intercepts terminal escape sequences by default.** Graphics protocol output written "to the terminal" from a process running inside tmux actually goes to tmux first. Unless tmux is told to let it through, it gets dropped (or, if tmux doesn't understand it, potentially mishandled). tmux calls this **passthrough**, controlled by the `allow-passthrough` option, and it requires the *sending* program to wrap its output in tmux's own passthrough envelope (`ESC P tmux; <escaped payload> ESC \`).

`chafa` already has first-class support for this: `--passthrough=tmux` makes it emit the correctly-wrapped envelope itself (fim doesn't hand-roll any of that escaping). fim detects tmux via `$TMUX`, which tmux sets unconditionally for every process it runs - not a query, just as safe as the Kitty-protocol checks.

Whether this actually *displays* anything still depends on the user's tmux configuration having `allow-passthrough on` set - fim can't control that, only use the flag correctly on its own end.

## Where the bypass happens

Real graphics payloads (`PreviewContent::RawGraphics` in `src/preview/mod.rs`) are written directly to the terminal by `sync_preview_graphics()` in `src/main.rs`, entirely outside ratatui's normal rendering. ratatui deliberately renders nothing into that screen region (see `ui/preview.rs`) so its own frame-to-frame diffing never repaints over the image; the image is written once when the focused entry (or its on-screen position) changes, and explicitly cleared when navigating away or on exit.

## Summary of the detection chain

```
detect_graphics_format():
  1. $KITTY_WINDOW_ID / $GHOSTTY_RESOURCES_DIR / $WEZTERM_EXECUTABLE / $TERM_PROGRAM
     -> Some("kitty")          [terminal-set env vars, zero risk]
  2. `omarchy default terminal` -> foot            -> Some("sixels")
                                -> kitty/ghostty    -> Some("kitty")
                                -> anything else    -> None
                                                    [local CLI call, not a terminal query]
  3. otherwise -> None -> character art fallback

detect_passthrough():
  $TMUX set -> Some("tmux")   [terminal-multiplexer-set env var, zero risk]
  otherwise -> None
```

Nothing in this chain ever writes a query to the terminal and waits for a response. That constraint is not a style preference - it's the direct lesson of symptom 2.
