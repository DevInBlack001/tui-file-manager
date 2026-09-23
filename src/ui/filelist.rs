// ui/filelist.rs - Main file list widget: parent entry, cursor, selection,
// column-aligned name/size/mtime.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Row, Table};
use ratatui::Frame;

use crate::app::App;
use crate::core::Entry;
use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    let title = if app.viewing_recents {
        " Recents ".to_string()
    } else {
        format!(" {} ", app.cwd.display())
    };

    let mut title_spans = vec![Span::styled(title, Style::default().fg(theme.fg_bright))];
    if !app.filter.is_empty() {
        title_spans.push(Span::styled(
            format!("[filter: {}] ", app.filter),
            Style::default().fg(theme.yellow),
        ));
    }
    if !app.selected.is_empty() {
        title_spans.push(Span::styled(
            format!("[{} selected] ", app.selected.len()),
            Style::default().fg(theme.accent),
        ));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(Line::from(title_spans))
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(err) = &app.listing.error {
        let msg = describe_list_error(err);
        frame.render_widget(
            ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                msg,
                Style::default().fg(theme.red),
            ))),
            inner,
        );
        return;
    }

    let entries = app.visible_entries();
    if entries.is_empty() {
        let msg = if app.filter.is_empty() {
            "(empty directory)"
        } else {
            "(no matches)"
        };
        frame.render_widget(
            ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                msg,
                Style::default().fg(theme.muted),
            ))),
            inner,
        );
        return;
    }

    let visible_height = inner.height as usize;
    let start = if entries.len() <= visible_height {
        0
    } else {
        let max_start = entries.len() - visible_height;
        app.cursor.saturating_sub(visible_height / 2).min(max_start)
    };
    let end = (start + visible_height).min(entries.len());

    let rows: Vec<Row> = entries[start..end]
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let real_idx = start + i;
            row_for_entry(entry, real_idx == app.cursor, app.selected.contains(&entry.path), theme)
        })
        .collect();

    let widths = [
        Constraint::Min(10),
        Constraint::Length(8),
        Constraint::Length(17),
    ];
    let table = Table::new(rows, widths);
    frame.render_widget(table, inner);
}

fn row_for_entry<'a>(entry: &Entry, is_cursor: bool, is_selected: bool, theme: &Theme) -> Row<'a> {
    let name_color = if entry.is_broken_symlink {
        theme.red
    } else if entry.is_symlink {
        theme.cyan
    } else if entry.is_dir {
        theme.green
    } else if entry.is_executable() {
        theme.yellow
    } else {
        theme.fg
    };

    let marker = if is_selected { "\u{2713} " } else { "  " };
    let name_span = format!("{marker}{}", entry.display_name());

    let mut style = Style::default().fg(name_color);
    if is_cursor {
        style = style.bg(theme.selection_bg).add_modifier(Modifier::BOLD);
    }
    if is_selected {
        style = style.add_modifier(Modifier::ITALIC);
    }

    let size = if entry.is_dir { "-".to_string() } else { entry.size_human() };

    Row::new(vec![
        Line::from(Span::raw(name_span)),
        Line::from(Span::styled(size, Style::default().fg(theme.muted))),
        Line::from(Span::styled(entry.mtime_human(), Style::default().fg(theme.muted))),
    ])
    .style(style)
}

fn describe_list_error(err: &crate::core::ListError) -> String {
    use crate::core::ListError::*;
    match err {
        PermissionDenied => "permission denied reading this directory".to_string(),
        NotFound => "directory not found".to_string(),
        NotADirectory => "not a directory".to_string(),
        Other(s) => s.clone(),
    }
}
