// ui/mod.rs - Top-level draw entry point: lays out the three panes + status
// bar, and overlays the help screen when active.

pub mod ansi;
pub mod filelist;
pub mod layout;
pub mod preview;
pub mod prompt;
pub mod sidebar;
pub mod statusbar;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode};

const HELP_TEXT: &[(&str, &str)] = &[
    ("j/k, arrows", "move cursor"),
    ("l, Enter", "open directory"),
    ("h, Backspace", "parent directory"),
    ("~", "go home"),
    ("g", "goto path (supports / and ~)"),
    ("1-9", "jump to sidebar bookmark"),
    (".", "toggle hidden files"),
    ("s / S", "cycle sort key / reverse"),
    ("/", "search / filter"),
    ("e", "open in nvim (offers to install; else $EDITOR)"),
    ("o", "open with xdg-open"),
    ("O", "open with (prompt for a command)"),
    ("R, F5", "refresh listing + theme"),
    ("Space", "toggle selection"),
    ("a", "select all visible"),
    ("Esc", "clear selection"),
    ("c / x / p", "copy / cut / paste (via ftctl)"),
    ("P", "show clipboard"),
    ("d", "trash"),
    ("D", "permanent delete (confirm)"),
    ("r", "rename"),
    ("n / t", "new directory / new file"),
    ("L", "new symlink"),
    ("?", "toggle this help"),
    ("q, Ctrl+C", "quit"),
];

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let panes = layout::compute(area, app.config.ui.sidebar_width_pct, app.config.ui.preview_width_pct);

    app.preview_area = panes.preview;

    sidebar::render(frame, panes.sidebar, app);
    filelist::render(frame, panes.filelist, app);
    preview::render_preview(frame, panes.preview, app);
    preview::render_stats(frame, panes.stats, app);
    statusbar::render(frame, panes.status, app);

    if matches!(app.mode, Mode::Help) {
        render_help(frame, area, app);
    }
}

fn render_help(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let popup = centered_rect(60, 80, area);

    let lines: Vec<Line> = HELP_TEXT
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(format!("{key:<14}"), Style::default().fg(theme.accent)),
                Span::styled(*desc, Style::default().fg(theme.fg)),
            ])
        })
        .collect();

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Help (press any key to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg_darker).fg(theme.fg));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn centered_rect(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let [_, mid_v, _] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .areas(area);
    let [_, mid_h, _] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .areas(mid_v);
    mid_h
}
