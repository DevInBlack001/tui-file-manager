// ui/filelist.rs - Main file list widget: parent entry, cursor, selection,
// rendered as one of several view modes (list/compact/detailed/grid/tree/
// columns).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::App;
use crate::config::ViewMode;
use crate::core::Entry;
use crate::theme::Theme;

/// Directory reads for read-only side panels (tree peek, columns) are capped
/// so a directory with an enormous number of entries can't blow up memory or
/// the render loop.
const SIDE_LISTING_CAP: usize = 300;

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
    title_spans.push(Span::styled(
        format!("[{}] ", app.config.ui.view_mode.label()),
        Style::default().fg(theme.muted),
    ));

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
            Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(theme.red)))),
            inner,
        );
        return;
    }

    let entries = app.visible_entries();
    if entries.is_empty() {
        let msg = if app.filter.is_empty() { "(empty directory)" } else { "(no matches)" };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(theme.muted)))),
            inner,
        );
        return;
    }

    match app.config.ui.view_mode {
        ViewMode::List => render_table(frame, inner, app, theme, &entries, TableStyle::List),
        ViewMode::Compact => render_table(frame, inner, app, theme, &entries, TableStyle::Compact),
        ViewMode::Detailed => render_table(frame, inner, app, theme, &entries, TableStyle::Detailed),
        ViewMode::Grid => render_grid(frame, inner, app, theme, &entries),
        ViewMode::Tree => render_tree(frame, inner, app, theme, &entries),
        ViewMode::Columns => render_columns(frame, inner, app, theme, &entries),
    }
}

// ---------------------------------------------------------------------------
// Shared: icon + color
// ---------------------------------------------------------------------------

/// Nerd Font glyph for an entry, chosen by type/extension. Codepoints are
/// verified against JetBrainsMonoNerdFont's real cmap (not guessed from
/// memory - several plausible-looking PUA codepoints turned out to map to
/// unrelated glyphs, e.g. fa-steam instead of a music icon).
fn icon_for(entry: &Entry) -> &'static str {
    if entry.is_broken_symlink || entry.is_symlink {
        return "\u{f0c1}"; // fa-link
    }
    if entry.is_dir {
        return "\u{f07b}"; // fa-folder
    }
    if entry.is_executable() {
        return "\u{f489}"; // oct-terminal
    }
    let ext = entry
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "c" | "cpp" | "h" | "hpp" | "java"
        | "sh" | "toml" | "json" | "yaml" | "yml" | "lua" | "html" | "css" => "\u{f1c9}", // fa-file_code_o
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" => "\u{f1c5}", // fa-file_picture_o
        "pdf" => "\u{f1c1}",                                                  // fa-file_pdf_o
        "xls" | "xlsx" | "csv" => "\u{f1c3}",                                 // fa-file_excel_o
        "doc" | "docx" | "odt" => "\u{f1c2}",                                 // fa-file_word_o
        "ppt" | "pptx" | "odp" => "\u{f1c4}",                                 // fa-file_powerpoint_o
        "zip" | "tar" | "gz" | "xz" | "bz2" | "7z" | "rar" | "zst" => "\u{f1c6}", // fa-file_zipper
        "mp3" | "flac" | "wav" | "ogg" | "m4a" => "\u{f1c7}",                 // fa-file_sound_o
        "mp4" | "mkv" | "webm" | "avi" | "mov" => "\u{f03d}",                 // fa-video_camera
        _ => "\u{f15b}",                                                      // fa-file
    }
}

fn entry_color(entry: &Entry, theme: &Theme) -> ratatui::style::Color {
    if entry.is_broken_symlink {
        theme.red
    } else if entry.is_symlink {
        theme.cyan
    } else if entry.is_dir {
        theme.green
    } else if entry.is_executable() {
        theme.yellow
    } else {
        theme.fg
    }
}

fn display_with_icon(entry: &Entry, show_icons: bool) -> String {
    if show_icons {
        format!("{} {}", icon_for(entry), entry.display_name())
    } else {
        entry.display_name()
    }
}

/// Compute the `start..end` window of `entries` to display, keeping the
/// cursor centered once the list is taller than the viewport.
fn scroll_window(cursor: usize, total: usize, visible: usize) -> (usize, usize) {
    let start = if total <= visible {
        0
    } else {
        let max_start = total - visible;
        cursor.saturating_sub(visible / 2).min(max_start)
    };
    let end = (start + visible).min(total);
    (start, end)
}

// ---------------------------------------------------------------------------
// List / Compact / Detailed (Table-based)
// ---------------------------------------------------------------------------

enum TableStyle {
    List,
    Compact,
    Detailed,
}

fn render_table(frame: &mut Frame, inner: Rect, app: &App, theme: &Theme, entries: &[&Entry], style: TableStyle) {
    let visible_height = inner.height as usize;
    let (start, end) = scroll_window(app.cursor, entries.len(), visible_height);

    let rows: Vec<Row> = entries[start..end]
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let real_idx = start + i;
            row_for_entry(
                entry,
                real_idx == app.cursor,
                app.selected.contains(&entry.path),
                theme,
                app.config.ui.show_icons,
                &style,
            )
        })
        .collect();

    let widths: &[Constraint] = match style {
        TableStyle::List => &[Constraint::Min(10), Constraint::Length(8), Constraint::Length(17)],
        TableStyle::Compact => &[Constraint::Min(10)],
        TableStyle::Detailed => &[
            Constraint::Min(10),
            Constraint::Length(8),
            Constraint::Length(17),
            Constraint::Length(11),
            Constraint::Length(16),
        ],
    };
    let table = Table::new(rows, widths.to_vec());
    frame.render_widget(table, inner);
}

fn row_for_entry<'a>(
    entry: &Entry,
    is_cursor: bool,
    is_selected: bool,
    theme: &Theme,
    show_icons: bool,
    style: &TableStyle,
) -> Row<'a> {
    let name_color = entry_color(entry, theme);
    let marker = if is_selected { "\u{2713} " } else { "  " };
    let name_span = format!("{marker}{}", display_with_icon(entry, show_icons));

    let mut row_style = Style::default().fg(name_color);
    if is_cursor {
        row_style = row_style.bg(theme.selection_bg).add_modifier(Modifier::BOLD);
    }
    if is_selected {
        row_style = row_style.add_modifier(Modifier::ITALIC);
    }

    let mut cells = vec![Line::from(Span::raw(name_span))];
    if !matches!(style, TableStyle::Compact) {
        let size = if entry.is_dir { "-".to_string() } else { entry.size_human() };
        cells.push(Line::from(Span::styled(size, Style::default().fg(theme.muted))));
        cells.push(Line::from(Span::styled(entry.mtime_human(), Style::default().fg(theme.muted))));
    }
    if matches!(style, TableStyle::Detailed) {
        cells.push(Line::from(Span::styled(entry.mode_str(), Style::default().fg(theme.muted))));
        cells.push(Line::from(Span::styled(entry.owner(), Style::default().fg(theme.muted))));
    }

    Row::new(cells).style(row_style)
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

const GRID_CELL_WIDTH: u16 = 20;
const GRID_CELL_HEIGHT: u16 = 2;

fn render_grid(frame: &mut Frame, inner: Rect, app: &App, theme: &Theme, entries: &[&Entry]) {
    let cols = (inner.width / GRID_CELL_WIDTH).max(1) as usize;
    let visible_rows = (inner.height / GRID_CELL_HEIGHT).max(1) as usize;
    let total_rows = entries.len().div_ceil(cols);
    let cursor_row = app.cursor / cols;
    let (start_row, end_row) = scroll_window(cursor_row, total_rows, visible_rows);
    let start_idx = start_row * cols;
    let end_idx = (end_row * cols).min(entries.len());

    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(GRID_CELL_HEIGHT); end_row - start_row])
        .split(inner);

    for (r, row_area) in row_areas.iter().enumerate() {
        let col_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Length(GRID_CELL_WIDTH); cols])
            .split(*row_area);

        for (c, cell_area) in col_areas.iter().enumerate() {
            let idx = start_idx + r * cols + c;
            if idx >= end_idx {
                continue;
            }
            let entry = entries[idx];
            render_grid_cell(
                frame,
                *cell_area,
                entry,
                idx == app.cursor,
                app.selected.contains(&entry.path),
                theme,
                app.config.ui.show_icons,
            );
        }
    }
}

fn render_grid_cell(
    frame: &mut Frame,
    area: Rect,
    entry: &Entry,
    is_cursor: bool,
    is_selected: bool,
    theme: &Theme,
    show_icons: bool,
) {
    let color = entry_color(entry, theme);
    let mut style = Style::default().fg(color);
    if is_cursor {
        style = style.bg(theme.selection_bg).add_modifier(Modifier::BOLD);
    }
    if is_selected {
        style = style.add_modifier(Modifier::ITALIC);
    }

    let icon = if show_icons { icon_for(entry) } else { "" };
    let name = truncate_chars(&entry.display_name(), (area.width as usize).saturating_sub(1));

    let lines = vec![
        Line::from(Span::styled(icon, style)),
        Line::from(Span::styled(name, style)),
    ];
    frame.render_widget(Paragraph::new(lines).style(style), area);
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 1 {
        s.chars().take(max).collect()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}'); // ellipsis
        out
    }
}

// ---------------------------------------------------------------------------
// Tree (inline peek of one expanded directory)
// ---------------------------------------------------------------------------

fn render_tree(frame: &mut Frame, inner: Rect, app: &App, theme: &Theme, entries: &[&Entry]) {
    let visible_height = inner.height as usize;
    let (start, end) = scroll_window(app.cursor, entries.len(), visible_height);

    let mut rows: Vec<Row> = Vec::new();
    for (i, entry) in entries[start..end].iter().enumerate() {
        let real_idx = start + i;
        rows.push(row_for_entry(
            entry,
            real_idx == app.cursor,
            app.selected.contains(&entry.path),
            theme,
            app.config.ui.show_icons,
            &TableStyle::List,
        ));

        if entry.is_dir && app.tree_expanded.as_deref() == Some(entry.path.as_path()) {
            // A distinct theme color for peeked children: `theme.blue` is the
            // same RGB as `theme.accent` (already used for borders) in both
            // shipped themes, so it read as low-contrast/blended with the
            // border; `theme.cyan` is a genuinely different hue. DIM was also
            // dropped - it's a terminal rendering attribute that can render
            // as nearly invisible depending on the terminal/theme.
            for child_name in read_child_names(&entry.path) {
                rows.push(Row::new(vec![Line::from(Span::styled(
                    format!("    \u{2514}\u{2500} {child_name}"),
                    Style::default().fg(theme.cyan),
                ))]));
            }
        }
    }

    let widths = [Constraint::Min(10), Constraint::Length(8), Constraint::Length(17)];
    let table = Table::new(rows, widths);
    frame.render_widget(table, inner);
}

/// Bounded, read-only child listing for the Tree view's inline peek. Errors
/// (permission denied, race with a delete) collapse to an empty peek rather
/// than a panic or a stale display.
fn read_child_names(path: &std::path::Path) -> Vec<String> {
    let Ok(read_dir) = std::fs::read_dir(path) else { return Vec::new() };
    let mut names: Vec<String> = read_dir
        .take(SIDE_LISTING_CAP)
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// ---------------------------------------------------------------------------
// Columns (parent | current)
// ---------------------------------------------------------------------------

fn render_columns(frame: &mut Frame, inner: Rect, app: &App, theme: &Theme, entries: &[&Entry]) {
    let [parent_area, current_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .areas(inner);

    render_parent_column(frame, parent_area, app, theme);

    let divider = Block::default().borders(Borders::LEFT).border_style(Style::default().fg(theme.muted));
    let current_inner = divider.inner(current_area);
    frame.render_widget(divider, current_area);

    render_table(frame, current_inner, app, theme, entries, TableStyle::List);
}

fn render_parent_column(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let Some(parent) = app.cwd.parent() else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("(no parent)", Style::default().fg(theme.muted)))),
            area,
        );
        return;
    };

    let current_name = app.cwd.file_name().map(|n| n.to_string_lossy().into_owned());
    let Ok(read_dir) = std::fs::read_dir(parent) else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "permission denied",
                Style::default().fg(theme.red),
            ))),
            area,
        );
        return;
    };

    let mut names: Vec<String> = read_dir
        .take(SIDE_LISTING_CAP)
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();

    let lines: Vec<Line> = names
        .into_iter()
        .map(|name| {
            let is_current = current_name.as_deref() == Some(name.as_str());
            let style = if is_current {
                Style::default().fg(theme.fg_bright).bg(theme.selection_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.muted)
            };
            Line::from(Span::styled(name, style))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), area);
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
