// ui/preview.rs - Right-hand panel: content preview (top) + file stats
// (bottom), for the entry currently under the cursor.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::preview::{GlyphColor, PreviewContent};
use crate::theme::Theme;
use crate::ui::ansi;

pub fn render_preview(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Preview ")
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = match &app.preview_content {
        // chafa/bat emit real ANSI colour; parse it into styled spans so the
        // preview shows actual colour, not flat monochrome block characters.
        PreviewContent::AnsiText(v) | PreviewContent::ChafaLines(v) => {
            v.iter().map(|l| ansi::parse_line(l)).collect()
        }
        PreviewContent::Text(v) | PreviewContent::HexDump(v) => {
            v.iter().map(|l| Line::from(l.clone())).collect()
        }
        PreviewContent::Glyph(glyph_lines) => glyph_lines
            .iter()
            .map(|segments| {
                Line::from(
                    segments
                        .iter()
                        .map(|(text, role)| Span::styled(text.clone(), Style::default().fg(glyph_color(theme, *role))))
                        .collect::<Vec<_>>(),
                )
            })
            .collect(),
        PreviewContent::DirSummary {
            item_count,
            dirs,
            files,
            symlinks,
            total_size,
            newest_mtime,
        } => {
            let newest = newest_mtime
                .map(|_| "see stats panel".to_string())
                .unwrap_or_else(|| "n/a".to_string());
            vec![
                Line::from(format!("{item_count} item(s)")),
                Line::from(format!("  {dirs} directories")),
                Line::from(format!("  {files} files")),
                Line::from(format!("  {symlinks} symlinks")),
                Line::from(format!("total size: {}", human_size(*total_size))),
                Line::from(format!("newest change: {newest}")),
            ]
        }
        PreviewContent::Unavailable(reason) => {
            vec![Line::from(Span::styled(
                format!("preview unavailable: {reason}"),
                Style::default().fg(theme.muted),
            ))]
        }
        PreviewContent::Loading => {
            vec![Line::from(Span::styled("loading...", Style::default().fg(theme.muted)))]
        }
    };

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inner,
    );
}

pub fn render_stats(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Stats ")
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(entry) = app.focused_entry() else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("(no entry)", Style::default().fg(theme.muted)))),
            inner,
        );
        return;
    };

    let mut lines = vec![
        stat_line("Name", &entry.name, theme),
        stat_line("Size", &format!("{} ({} B)", entry.size_human(), entry.size), theme),
    ];
    if let Some(mime) = &app.focused_mime {
        lines.push(stat_line("MIME", mime, theme));
    }
    lines.push(stat_line("Mode", &format!("{} ({:o})", entry.mode_str(), entry.mode & 0o7777), theme));
    lines.push(stat_line("Owner", &entry.owner(), theme));
    lines.push(stat_line("Modified", &entry.mtime_human(), theme));
    lines.push(stat_line("Inode", &entry.ino.to_string(), theme));
    lines.push(stat_line("Links", &entry.nlink.to_string(), theme));
    lines.push(stat_line("Blocks", &entry.blocks.to_string(), theme));

    if let Some(target) = &entry.symlink_target {
        let status = if entry.is_broken_symlink { " (broken)" } else { "" };
        lines.push(stat_line("Link target", &format!("{}{status}", target.display()), theme));
    }

    if let Some(err) = &entry.stat_error {
        let label = if entry.stat_permission_denied {
            "permission denied"
        } else {
            err.as_str()
        };
        lines.push(Line::from(Span::styled(
            format!("stat error: {label}"),
            Style::default().fg(theme.red),
        )));
    }

    if let Some(job) = &app.focused_job {
        lines.push(Line::from(Span::styled(
            format!("transfer: {} {} ({}%)", job.mode, job.state, job.pct.unwrap_or(0)),
            Style::default().fg(theme.accent),
        )));
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn glyph_color(theme: &Theme, role: GlyphColor) -> ratatui::style::Color {
    match role {
        GlyphColor::Fg => theme.fg,
        GlyphColor::FgDim => theme.fg_dim,
        GlyphColor::Accent => theme.accent,
        GlyphColor::Muted => theme.muted,
        GlyphColor::Yellow => theme.yellow,
        GlyphColor::Cyan => theme.cyan,
    }
}

fn stat_line<'a>(label: &'a str, value: &str, theme: &Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<9} "), Style::default().fg(theme.muted)),
        Span::styled(value.to_string(), Style::default().fg(theme.fg)),
    ])
}

fn human_size(size: u64) -> String {
    if size < 1024 {
        format!("{size}B")
    } else if size < 1024 * 1024 {
        format!("{:.1}K", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1}M", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}G", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

