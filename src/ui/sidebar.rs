// ui/sidebar.rs - Sidebar widget: bookmark sections + custom bookmarks.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    let mut idx = 1u8;

    for bm in &app.bookmarks.sections {
        let is_current = if bm.is_recents {
            app.viewing_recents
        } else {
            !app.viewing_recents && app.cwd == bm.path
        };
        lines.push(bookmark_line(&bm.name, bm.exists, is_current, idx, theme));
        idx += 1;
    }

    if !app.bookmarks.custom.is_empty() {
        lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(inner.width.max(1) as usize),
            Style::default().fg(theme.muted),
        )));
        lines.push(Line::from(Span::styled(
            "Bookmarks",
            Style::default().fg(theme.muted),
        )));
        for bm in &app.bookmarks.custom {
            let is_current = !app.viewing_recents && app.cwd == bm.path;
            lines.push(bookmark_line(&bm.name, bm.exists, is_current, idx, theme));
            idx += 1;
        }
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn bookmark_line<'a>(name: &'a str, exists: bool, is_current: bool, idx: u8, theme: &crate::theme::Theme) -> Line<'a> {
    let label = if idx <= 9 {
        format!("{idx} {name}")
    } else {
        format!("  {name}")
    };

    let mut style = if !exists {
        Style::default().fg(theme.muted)
    } else if is_current {
        Style::default()
            .fg(theme.fg_bright)
            .bg(theme.selection_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    if !exists {
        style = style.add_modifier(Modifier::DIM);
    }

    Line::from(Span::styled(label, style))
}
