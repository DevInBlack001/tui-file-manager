// ui/sidebar.rs - Sidebar widget: bookmark sections + custom bookmarks.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;
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
    let mut current_line: usize = 0;

    let row_gap = app.config.ui.sidebar_row_spacing;

    lines.push(section_header("PLACES", inner.width, theme));
    for bm in &app.bookmarks.sections {
        let is_current = if bm.is_recents {
            app.viewing_recents
        } else {
            !app.viewing_recents && app.cwd == bm.path
        };
        if is_current {
            current_line = lines.len();
        }
        lines.push(bookmark_line(&bm.name, bm.exists, is_current, idx, inner.width, theme, app.config.ui.show_icons));
        for _ in 0..row_gap {
            lines.push(Line::from(""));
        }
        idx += 1;
    }

    if !app.bookmarks.custom.is_empty() {
        lines.push(Line::from(""));
        lines.push(section_header("BOOKMARKS", inner.width, theme));
        for bm in &app.bookmarks.custom {
            let is_current = !app.viewing_recents && app.cwd == bm.path;
            if is_current {
                current_line = lines.len();
            }
            lines.push(bookmark_line(&bm.name, bm.exists, is_current, idx, inner.width, theme, app.config.ui.show_icons));
            for _ in 0..row_gap {
                lines.push(Line::from(""));
            }
            idx += 1;
        }
    }

    // Auto-scroll: with a short sidebar (e.g. LayoutMode::Top capping it to a
    // few rows), keep the highlighted entry in view rather than silently
    // clipping the bottom of the list with no way to reach it.
    let visible = inner.height as usize;
    let scroll_y = if lines.len() <= visible {
        0
    } else {
        let max_start = lines.len() - visible;
        current_line.saturating_sub(visible / 2).min(max_start)
    };

    frame.render_widget(Paragraph::new(lines).scroll((scroll_y as u16, 0)), inner);
}

fn section_header(title: &str, width: u16, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {title} "),
            Style::default().fg(theme.bg).bg(theme.muted).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "\u{2500}".repeat((width as usize).saturating_sub(title.len() + 2)),
            Style::default().fg(theme.muted),
        ),
    ])
}

fn bookmark_line<'a>(
    name: &'a str,
    exists: bool,
    is_current: bool,
    idx: u8,
    width: u16,
    theme: &Theme,
    show_icons: bool,
) -> Line<'a> {
    let number = if idx <= 9 { idx.to_string() } else { " ".to_string() };
    let marker = if is_current { "\u{25b8}" } else { " " };
    let icon = if show_icons {
        if is_current { "\u{f07c} " } else { "\u{f07b} " } // fa-folder_open / fa-folder
    } else {
        ""
    };

    let row_bg = if is_current { Some(theme.selection_bg) } else { None };
    let with_bg = |mut s: Style| {
        if let Some(bg) = row_bg {
            s = s.bg(bg);
        }
        s
    };

    let (name_style, number_style) = if !exists {
        (
            with_bg(Style::default().fg(theme.muted).add_modifier(Modifier::DIM)),
            with_bg(Style::default().fg(theme.muted).add_modifier(Modifier::DIM)),
        )
    } else if is_current {
        (
            with_bg(Style::default().fg(theme.fg_bright).add_modifier(Modifier::BOLD)),
            with_bg(Style::default().fg(theme.accent)),
        )
    } else {
        (Style::default().fg(theme.fg), Style::default().fg(theme.muted))
    };
    let marker_style = with_bg(Style::default().fg(if is_current { theme.accent } else { theme.bg }));

    let content_len = 4 + icon.chars().count() + name.chars().count(); // "M N " + icon + name
    let pad = " ".repeat((width as usize).saturating_sub(content_len));

    let mut spans = vec![
        Span::styled(format!("{marker} "), marker_style),
        Span::styled(format!("{number} "), number_style),
        Span::styled(icon, name_style),
        Span::styled(name, name_style),
    ];
    if is_current && !pad.is_empty() {
        spans.push(Span::styled(pad, with_bg(Style::default())));
    }
    Line::from(spans)
}
