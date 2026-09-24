pub mod app;
pub mod config;
pub mod core;
pub mod fs;
pub mod preview;
pub mod theme;
pub mod ui;

use std::io::{self, Stdout, Write};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::cursor::MoveTo;
use crossterm::event::{self, DisableMouseCapture, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use app::{Action, App};

const POLL_INTERVAL: Duration = Duration::from_millis(150);
/// Kitty graphics protocol: delete all image placements. Safe to send
/// unconditionally - terminals that don't implement the protocol treat an
/// unrecognized APC sequence as a no-op.
const KITTY_CLEAR_ALL: &[u8] = b"\x1b_Ga=d\x1b\\";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    install_panic_hook();

    let cfg = config::load_or_default();
    let xdg = fs::xdg::resolve();

    let mut app = App::new(cfg, xdg);

    let mut terminal = match setup_terminal() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[fim] failed to initialize terminal: {e}");
            std::process::exit(1);
        }
    };

    let result = run(&mut terminal, &mut app);

    let _ = restore_terminal(&mut terminal);

    if let Err(e) = result {
        eprintln!("[fim] error: {e}");
        std::process::exit(1);
    }
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    // The last (path, screen area) a terminal-graphics image was blitted
    // for, tracked outside App since it's terminal-IO state, not app state.
    // ratatui redraws the bordered "Preview" box every frame but never
    // touches its interior for a RawGraphics (see ui/preview.rs), so the
    // blitted image persists on screen without needing to be re-sent every
    // frame - only when the focused entry or the pane's on-screen position
    // actually changes.
    let mut graphics_shown: Option<(PathBuf, Rect)> = None;

    loop {
        app.poll_preview();
        app.maybe_poll_job_status();

        terminal.draw(|frame| ui::draw(frame, app))?;
        sync_preview_graphics(terminal, app, &mut graphics_shown)?;

        if !event::poll(POLL_INTERVAL)? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => match app.on_key(key) {
                Action::None => {}
                Action::Quit => break,
                Action::RunForeground(bin, args) => {
                    run_foreground(terminal, &bin, &args)?;
                    graphics_shown = None; // the foreground program owned the screen
                }
            },
            Event::Resize(_, _) => {
                // Next loop iteration redraws against the new frame area;
                // a changed preview_area is itself enough to trigger a
                // reblit via sync_preview_graphics's own comparison.
            }
            _ => {}
        }

        if app.should_quit {
            break;
        }
    }
    if graphics_shown.is_some() {
        let mut stdout = io::stdout();
        let _ = stdout.write_all(KITTY_CLEAR_ALL);
        let _ = stdout.flush();
    }
    Ok(())
}

/// Blit or clear the real terminal-graphics image for the focused entry,
/// bypassing ratatui's cell buffer entirely (see ui/preview.rs and
/// App::preview_graphics for why this is safe to do outside its normal
/// diffing). Only writes anything when the image or its on-screen position
/// actually changed since the last call.
///
/// Every transition away from a previously-shown image - to a different
/// image, or to non-graphics content - clears that old image first. Sixel
/// graphics aren't a separate compositing layer: they paint pixels directly
/// into the cells they cover, so a new image (which may be a different size)
/// only overwrites the cells it actually draws into, leaving any of the old
/// image's cells outside that footprint behind unless explicitly cleared.
fn sync_preview_graphics(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &App,
    graphics_shown: &mut Option<(PathBuf, Rect)>,
) -> io::Result<()> {
    match app.preview_graphics() {
        Some((path, bytes)) => {
            let area = app.preview_area;
            let already_shown = graphics_shown
                .as_ref()
                .is_some_and(|(p, a)| p == path && *a == area);
            if already_shown {
                return Ok(());
            }
            let backend = terminal.backend_mut();
            if let Some((_, old_area)) = graphics_shown.take() {
                clear_graphics_area(backend, old_area)?;
            }
            // Inside the "Preview" block's border.
            queue!(backend, MoveTo(area.x + 1, area.y + 1))?;
            backend.write_all(bytes)?;
            backend.flush()?;
            *graphics_shown = Some((path.to_path_buf(), area));
        }
        None => {
            if let Some((_, old_area)) = graphics_shown.take() {
                let backend = terminal.backend_mut();
                clear_graphics_area(backend, old_area)?;
            }
        }
    }
    Ok(())
}

/// Clear a previously-blitted graphics image occupying `area`: delete any
/// Kitty-protocol placements (a harmless no-op on a Sixel-only terminal),
/// then blank every cell inside the border so leftover Sixel pixel data
/// can't remain visible around the edges of whatever gets drawn next.
fn clear_graphics_area(backend: &mut CrosstermBackend<Stdout>, area: Rect) -> io::Result<()> {
    backend.write_all(KITTY_CLEAR_ALL)?;

    let inner_width = area.width.saturating_sub(2);
    let inner_height = area.height.saturating_sub(2);
    if inner_width > 0 && inner_height > 0 {
        let blank = " ".repeat(inner_width as usize);
        for row in 0..inner_height {
            queue!(backend, MoveTo(area.x + 1, area.y + 1 + row))?;
            backend.write_all(blank.as_bytes())?;
        }
    }
    backend.flush()
}

/// Suspend the TUI, run `bin args` in the foreground (e.g. `$EDITOR`), wait
/// for it to exit, then restore the TUI and force a full repaint.
fn run_foreground(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    bin: &std::path::Path,
    args: &[String],
) -> io::Result<()> {
    restore_terminal(terminal)?;

    let status = std::process::Command::new(bin).args(args).status();
    if let Err(e) = status {
        eprintln!("[fim] failed to run {}: {e}", bin.display());
    }

    *terminal = setup_terminal()?;
    terminal.clear()?;
    Ok(())
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()
}

/// Ensure the terminal is restored even if we panic, so a bug never leaves
/// the user's shell in raw/alternate-screen mode.
fn print_help() {
    println!(
        "{} {} - keyboard-driven TUI file manager for Arch/Omarchy",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("Usage: fim [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -V, --version   Print version and exit");
    println!("  -h, --help      Print this help and exit");
    println!();
    println!("Run with no arguments to browse the current directory. Press ? inside fim for the full keybinding reference.");
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original(panic_info);
    }));
}
