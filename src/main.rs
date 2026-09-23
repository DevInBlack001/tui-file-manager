pub mod app;
pub mod config;
pub mod core;
pub mod fs;
pub mod preview;
pub mod theme;
pub mod ui;

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::{Action, App};

const POLL_INTERVAL: Duration = Duration::from_millis(150);

fn main() {
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
    loop {
        app.poll_preview();
        app.maybe_poll_job_status();

        terminal.draw(|frame| ui::draw(frame, app))?;

        if !event::poll(POLL_INTERVAL)? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => match app.on_key(key) {
                Action::None => {}
                Action::Quit => break,
                Action::RunForeground(bin, args) => {
                    run_foreground(terminal, &bin, &args)?;
                }
            },
            Event::Resize(_, _) => {
                // Next loop iteration redraws against the new frame area.
            }
            _ => {}
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
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
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original(panic_info);
    }));
}
