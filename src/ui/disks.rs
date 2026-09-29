// ui/disks.rs - Disk usage panel: total/used/free with a visual bar, for
// every internal and external disk currently detected. Replaces the preview
// pane's spot when LayoutMode::Disks is active (see ui/layout.rs).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Frame;

use crate::core::diskspace::{self, DiskInfo};
use crate::theme::Theme;

const CARD_HEIGHT: u16 = 4;

pub fn render(frame: &mut Frame, area: Rect, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Disks ")
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let disks = diskspace::detect_all();
    if disks.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("no disks detected", Style::default().fg(theme.muted)))),
            inner,
        );
        return;
    }

    let visible = (inner.height / CARD_HEIGHT).max(1) as usize;
    let shown = disks.len().min(visible);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(CARD_HEIGHT); shown])
        .split(inner);

    for (disk, row) in disks.iter().zip(rows.iter()) {
        render_disk_card(frame, *row, disk, theme);
    }
}

fn render_disk_card(frame: &mut Frame, area: Rect, disk: &DiskInfo, theme: &Theme) {
    let [header, gauge_area, detail, _pad] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .areas(area);

    let header_line = Line::from(vec![
        Span::styled(format!("{} ", disk.icon), Style::default().fg(theme.accent)),
        Span::styled(disk.label.clone(), Style::default().fg(theme.fg_bright).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  ({})", disk.fstype), Style::default().fg(theme.muted)),
    ]);
    frame.render_widget(Paragraph::new(header_line), header);

    let fraction = disk.used_fraction().clamp(0.0, 1.0);
    // Same warning-level convention as `df`/most disk-usage tools: green
    // while there's plenty of room, yellow getting tight, red nearly full.
    // Every color here is a Theme field, never a literal, so it follows
    // whatever palette (Omarchy, pywal, or the built-in fallback) is active.
    let bar_color = if fraction >= 0.9 {
        theme.red
    } else if fraction >= 0.75 {
        theme.yellow
    } else {
        theme.green
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(bar_color).bg(theme.bg_lighter))
        .ratio(fraction)
        .label(format!("{:.0}%", fraction * 100.0));
    frame.render_widget(gauge, gauge_area);

    let detail_line = Line::from(Span::styled(
        format!(
            "total {}   used {}   free {}",
            diskspace::human_bytes(disk.total),
            diskspace::human_bytes(disk.used),
            diskspace::human_bytes(disk.free),
        ),
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(Paragraph::new(detail_line), detail);
}
