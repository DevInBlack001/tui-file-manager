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
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode};
use crate::theme::Theme;

struct HelpSection {
    title: &'static str,
    keys: &'static [(&'static str, &'static str)],
}

const HELP_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Navigation",
        keys: &[
            ("j/k, arrows", "move cursor"),
            ("l, Enter", "open directory"),
            ("h, Backspace", "parent directory"),
            ("~", "go home"),
            ("g", "goto path (/ and ~ work)"),
            ("1-9", "jump to sidebar bookmark"),
            ("b", "focus sidebar (j/k move, Enter/l select, Esc/h cancel)"),
            ("E", "eject/unmount (sidebar focused on a device)"),
            ("/", "search / filter"),
        ],
    },
    HelpSection {
        title: "View",
        keys: &[
            (".", "toggle hidden files"),
            ("s / S", "cycle sort key / reverse"),
            ("Tab", "cycle layout (sidebar left/right/top, hidden)"),
            ("v", "cycle view (list/compact/detailed/grid/tree/columns)"),
            ("z", "tree view: peek/collapse focused directory"),
            ("R, F5", "refresh listing + theme"),
            ("?", "toggle this help"),
        ],
    },
    HelpSection {
        title: "Open",
        keys: &[
            ("e", "nvim, offers to install"),
            ("o", "open with xdg-open"),
            ("O", "open with (picks from installed apps, or type a command)"),
        ],
    },
    HelpSection {
        title: "Selection & Clipboard",
        keys: &[
            ("Space", "toggle selection"),
            ("a", "select all visible"),
            ("Esc", "clear selection"),
            ("c / x / p", "copy / cut / paste"),
            ("P", "show clipboard"),
        ],
    },
    HelpSection {
        title: "File Operations",
        keys: &[
            ("r", "rename"),
            ("n / t", "new directory / new file"),
            ("L", "new symlink"),
            ("d", "trash"),
            ("D", "permanent delete (confirm)"),
        ],
    },
    HelpSection {
        title: "General",
        keys: &[("q, Ctrl+C", "quit")],
    },
];

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let panes = layout::compute(
        area,
        app.config.ui.sidebar_width_pct,
        app.config.ui.preview_width_pct,
        app.config.ui.layout_mode,
    );

    app.preview_area = panes.preview;

    if panes.sidebar.width > 0 && panes.sidebar.height > 0 {
        sidebar::render(frame, panes.sidebar, app);
    }
    filelist::render(frame, panes.filelist, app);
    if panes.preview.width > 0 && panes.preview.height > 0 {
        preview::render_preview(frame, panes.preview, app);
        preview::render_stats(frame, panes.stats, app);
    }
    statusbar::render(frame, panes.status, app);

    if matches!(app.mode, Mode::Help) {
        render_help(frame, area, app);
    }
    if let Mode::OpenWithPicker { apps, selected } = &app.mode {
        render_open_with_picker(frame, area, app, apps, *selected);
    }
}

fn render_open_with_picker(frame: &mut Frame, area: Rect, app: &App, apps: &[crate::fs::desktop_apps::DesktopApp], selected: usize) {
    let theme = &app.theme;
    let popup = centered_rect(60, 60, area);

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Open With ")
        .title_alignment(ratatui::layout::Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg_darker).fg(theme.fg));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [list_area, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .areas(inner);

    let lines: Vec<Line> = apps
        .iter()
        .enumerate()
        .map(|(i, app)| {
            let style = if i == selected {
                Style::default().fg(theme.fg_bright).bg(theme.selection_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            Line::from(Span::styled(format!(" {} ", app.name), style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), list_area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "j/k move, Enter open, / custom command, Esc cancel",
            Style::default().fg(theme.muted),
        )))
        .alignment(ratatui::layout::Alignment::Center),
        footer,
    );
}

fn render_help(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let popup = centered_rect(80, 70, area);

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Help ")
        .title_alignment(ratatui::layout::Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg_darker).fg(theme.fg));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // Two columns, sections stacked top-to-bottom in each, so related keys
    // read as a group instead of one long undifferentiated list.
    let [left_area, gap, right_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1), Constraint::Length(2), Constraint::Fill(1)])
        .areas(inner);
    let _ = gap;

    let mid = HELP_SECTIONS.len().div_ceil(2);
    let (left_sections, right_sections) = HELP_SECTIONS.split_at(mid);

    frame.render_widget(help_column(left_sections, theme), left_area);
    frame.render_widget(help_column(right_sections, theme), right_area);

    let footer = centered_rect_within(popup, popup.height.saturating_sub(2));
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "press any key to close",
            Style::default().fg(theme.muted),
        )))
        .alignment(ratatui::layout::Alignment::Center),
        footer,
    );
}

fn help_column<'a>(sections: &'a [HelpSection], theme: &Theme) -> Paragraph<'a> {
    let mut lines: Vec<Line> = Vec::new();
    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            section.title,
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(section.title.len().max(8)),
            Style::default().fg(theme.muted),
        )));
        for (key, desc) in section.keys {
            lines.push(Line::from(vec![
                Span::styled(format!("{key:<16}"), Style::default().fg(theme.green)),
                Span::styled(*desc, Style::default().fg(theme.fg)),
            ]));
        }
    }
    Paragraph::new(lines)
}

/// A single-row Rect, horizontally centered and pinned to the given row
/// within `area` - used for the popup's footer hint line.
fn centered_rect_within(area: Rect, row_from_top: u16) -> Rect {
    Rect {
        x: area.x,
        y: area.y + row_from_top.min(area.height.saturating_sub(1)),
        width: area.width,
        height: 1,
    }
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
