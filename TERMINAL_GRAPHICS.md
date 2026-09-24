# Terminal graphics: how fim detects what your terminal can do

This document explains a debugging trail from real testing on a real machine: images in the preview panel went through several visibly broken states before rendering correctly, and each one required understanding a different layer of the terminal/shell/multiplexer stack. It's written down here because the reasoning isn't obvious from the code alone, and the same class of bug is easy to reintroduce if this code gets "simplified" later without knowing why it's shaped this way.

Nothing described here involved editing any of the user's own dotfiles or system config (`~/.tmux.conf`, `~/.config/foot/`, Omarchy's own config, etc). Those were only ever read, for investigation. Every change described below was made to this project's own source and scripts.

## Symptom 1: images rendered as garbled text

`chafa` (the tool fim shells out to for image previews) can output several formats: plain character art (`symbols`), or real pixel graphics via the Sixel or Kitty terminal protocols. Left to auto-detect, chafa picked a graphics protocol even though its stdout was piped into fim and never reached a real terminal. fim captured that output as a string and displayed it as plain text, so a Sixel payload (which looks like `\eP0;1;0q"1;1;320;198#0;2;3;2;2#...`) showed up as literal characters on screen.

**Fix:** pin `chafa --format symbols` whenever fim isn't deliberately requesting real graphics (see below).

## Symptom 2: a rename prompt filled with garbage matching `rgb:RRRR/GGGG/BBBB`

This one took longer to find. `chafa` doesn't just auto-detect blindly: by default (`--probe=auto`) it actively **probes the terminal**, writing a query escape sequence (in this case, an OSC 10/11 "what are your foreground/background colors" query) and waiting up to 5 seconds for a response to refine its format/color decisions.

The problem: chafa's subprocess inherits fim's stdin by default, which is the *real* controlling terminal. fim's own main loop is *simultaneously* reading key events from that same terminal via `crossterm`. Two independent readers on the same input stream is a race: whichever one happens to consume the terminal's query response wins. When fim's key-event reader won, it tried to interpret the response bytes (`rgb:d8d8/dede/e9e9`, etc.) as individual keystrokes, and a text prompt (like rename) happened to be focused at the time, so every one of those bytes got typed into it.

**Fix, two parts:**
1. `--probe off` on every chafa invocation. `--format`/`--colors` are already pinned explicitly, so chafa has nothing left to learn from probing.
2. `.stdin(Stdio::null())` on **every** subprocess fim spawns that doesn't need real terminal interaction: `chafa`, `bat`, `pdftotext`, `ffmpegthumbnailer`, `file`, `trash-put`, `ftctl`. This is defense in depth: even if some future flag or tool version tries to probe again, it can't reach the real terminal to do so.

**The general lesson:** any subprocess that inherits stdin can race a TUI's own input loop if it tries to read from the terminal. A subprocess should only get real stdin when it specifically needs interactive terminal control - a foreground editor, `sudo` asking for a password - and those cases should suspend the TUI's own input loop first, which is what `Action::RunForeground` does for `$EDITOR`.

## Symptom 3: character art looks bad and only fills part of the pane

Once the two bugs above were fixed, chafa's `symbols` mode was working correctly, but it isn't very good. Character-art image previews are inherently low-resolution (each "pixel" is a whole terminal cell), and chafa's aspect-preserving fit doesn't try to fill the requested box, so a wide photo in a tall pane leaves most of it blank.

**Fix:** `--stretch` to fill the pane. This is also the point where pursuing real graphics protocols properly became worth the effort.

## Finding out what terminal is actually in use

Showing a *real* picture requires knowing which graphics protocol (if any) the terminal understands, and it has to be known **without querying the terminal** - that's exactly the mechanism that caused symptom 2.

The safe signal is environment variables the terminal sets at its own startup, inherited down through every child process:

| Terminal | Signal | Protocol |
|---|---|---|
| kitty | `$KITTY_WINDOW_ID` | Kitty graphics protocol |
| Ghostty | `$GHOSTTY_RESOURCES_DIR` | Kitty graphics protocol |
| WezTerm | `$WEZTERM_EXECUTABLE`, or `$TERM_PROGRAM=WezTerm` | Kitty graphics protocol |

This covers three popular terminals cleanly, but Omarchy's actual default terminal is **`foot`**, and foot sets none of these. Checking foot's own manual turned up why: foot deliberately **unsets** `$TERM_PROGRAM` (and `$TERM_PROGRAM_VERSION`) in every child process, specifically to avoid other programs misdetecting it as something else. There is no environment variable that safely says "I am foot."

**Resolution:** ask the platform. `omarchy default terminal` is a local CLI helper, part of Omarchy itself, that reports which of a fixed set of terminals (`alacritty`/`foot`/`ghostty`/`kitty`) is configured as the system default. It's a hint: the terminal fim actually runs in could differ from the configured default. But it's a local command invocation, so it carries none of the race risk from symptom 2. foot supports **Sixel** graphics (confirmed via `foot.ini`'s `sixel` option, on by default), so `foot` maps to `chafa --format sixels`.

`detect_graphics_format()` in `src/preview/image.rs` tries the environment-variable checks first, then falls back to the `omarchy` helper. A miss on both falls back to character art, the same as any other terminal fim doesn't recognize.

## Finding out about tmux

The user reported the fix still wasn't working, and testing turned up that this session's own shell was itself running inside tmux. That mattered for two separate reasons:

1. **`$TERM` gets masked.** Inside tmux, `$TERM` is normally `tmux-256color` or similar, never the outer terminal's real value. Any detection strategy based on reading `$TERM` for a specific terminal name would silently break for every tmux user. (This is also why the Kitty-protocol environment variables above are checked directly and not via `$TERM`: tmux passes through arbitrary environment variables from the process that started it, and only intercepts `$TERM` itself.)
2. **tmux intercepts terminal escape sequences by default.** Graphics protocol output written "to the terminal" from a process running inside tmux actually goes to tmux first. Unless tmux is told to let it through, it gets dropped, or mishandled if tmux doesn't understand it. tmux calls this **passthrough**, controlled by the `allow-passthrough` option, and it requires the *sending* program to wrap its output in tmux's own passthrough envelope (`ESC P tmux; <escaped payload> ESC \`).

`chafa` already has first-class support for this: `--passthrough=tmux` makes it emit the correctly-wrapped envelope itself, so fim never hand-rolls any of that escaping. fim detects tmux via `$TMUX`, which tmux sets unconditionally for every process it runs - as safe a signal as the Kitty-protocol checks, and just as far from a terminal query.

Whether this actually *displays* anything still depends on the user's tmux configuration having `allow-passthrough on` set. fim can only use the flag correctly on its own end; it has no way to change the user's tmux.conf.

## Symptom 4: images stacked on top of each other and never cleared

Once real graphics were rendering, moving from one image to the next left the old one on screen underneath the new one. The clearing logic only ran when leaving graphics mode entirely, for example moving to a text file, so it never fired for an image-to-image transition.

This mattered more for Sixel than it would have for the Kitty protocol. The Kitty protocol treats an image as a distinct object the terminal tracks and can delete as a whole. Sixel has no such object: it paints pixels directly into whatever cells the cursor covers at the time. A new, possibly differently-sized image only overwrites the cells it actually draws into, so any cells the previous image occupied outside that footprint keep showing its pixels indefinitely.

**Fix:** every transition away from a shown image, whether to a different image or to non-graphics content, now clears that image's recorded screen area first: a Kitty-protocol delete-all command (inert on a Sixel-only terminal), followed by blanking every cell inside the pane's border.

## Symptom 5: images overflowed past the pane's borders

Images were requested from `chafa` using the preview pane's full outer size, border included, but blitted starting one cell inside that border. Every image was therefore rendered 2 columns and 2 rows larger than the space it was drawn into, and the extra strip spilled past the pane's right and bottom edges. Because the clearing fix above only clears the pane's recorded inner area, that overflow sat outside its reach and stayed visible even after moving to a different file.

**Fix:** size preview requests using the pane's inner content dimensions (width and height each minus 2, matching how a bordered `ratatui` block computes its own inner area), so the image is generated at exactly the size it gets drawn into.

## Where the bypass happens

Real graphics payloads (`PreviewContent::RawGraphics` in `src/preview/mod.rs`) are written directly to the terminal by `sync_preview_graphics()` in `src/main.rs`, entirely outside ratatui's normal rendering. ratatui deliberately renders nothing into that screen region (see `ui/preview.rs`), so its own frame-to-frame diffing never repaints over the image. The image is written once when the focused entry or its on-screen position changes, and cleared when navigating away or on exit.

## Summary of the detection chain

```
detect_graphics_format():
  1. $KITTY_WINDOW_ID / $GHOSTTY_RESOURCES_DIR / $WEZTERM_EXECUTABLE / $TERM_PROGRAM
     -> Some("kitty")          [terminal-set env vars, zero risk]
  2. `omarchy default terminal` -> foot            -> Some("sixels")
                                -> kitty/ghostty    -> Some("kitty")
                                -> anything else    -> None
                                                    [local CLI call, no terminal query]
  3. otherwise -> None -> character art fallback

detect_passthrough():
  $TMUX set -> Some("tmux")   [terminal-multiplexer-set env var, zero risk]
  otherwise -> None
```

Every step here reads a value that was already set before fim started; none of it writes anything to the terminal and waits for a reply. That constraint comes directly from symptom 2.
