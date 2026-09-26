// ui/layout.rs - Panel sizing: sidebar / file list / preview+stats splits.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::config::LayoutMode;

pub struct Panes {
    pub sidebar: Rect,
    pub filelist: Rect,
    pub preview: Rect,
    pub stats: Rect,
    pub status: Rect,
}

const MIN_SIDEBAR_COLS: u16 = 16;
const MIN_SIDEBAR_ROWS: u16 = 6;
/// Stats always shows a fixed, small set of fields (see ui/preview.rs's
/// render_stats): up to ~11 content rows plus 2 border rows. Giving it a
/// fixed height rather than a percentage of the pane means Preview gets
/// every remaining row instead of being capped at an arbitrary split - the
/// preview pane (especially for images) benefits far more from extra height
/// than the fixed-size stats block ever would.
const STATS_HEIGHT: u16 = 13;

fn pct(total: u16, p: u8) -> u16 {
    ((total as u32 * p.min(90) as u32) / 100) as u16
}

fn split_preview(preview: Rect) -> (Rect, Rect) {
    let [preview_top, stats] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(STATS_HEIGHT)])
        .areas(preview);
    (preview_top, stats)
}

/// Split the terminal area into sidebar / file list / preview+stats / status
/// bar, honoring the selected `LayoutMode`.
///
/// `sidebar_pct` and `preview_pct` come from config; panels never go below a
/// minimum size, as long as the terminal affords it (on a pathologically
/// small terminal this degrades gracefully down to whatever space is
/// available rather than panicking).
pub fn compute(area: Rect, sidebar_pct: u8, preview_pct: u8, layout_mode: LayoutMode) -> Panes {
    let [body, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .areas(area);

    if layout_mode == LayoutMode::Top {
        let sidebar_h = pct(body.height, sidebar_pct)
            .max(MIN_SIDEBAR_ROWS.min(body.height))
            .min(body.height);
        let [sidebar, rest] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(sidebar_h), Constraint::Min(3)])
            .areas(body);

        let preview_w = pct(rest.width, preview_pct);
        let filelist_w = rest.width.saturating_sub(preview_w);
        let [filelist, preview] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(filelist_w), Constraint::Length(preview_w)])
            .areas(rest);

        let (preview_top, stats) = split_preview(preview);
        return Panes { sidebar, filelist, preview: preview_top, stats, status };
    }

    let total = body.width;
    let sidebar_w = if layout_mode == LayoutMode::NoSidebar {
        0
    } else {
        pct(total, sidebar_pct).max(MIN_SIDEBAR_COLS.min(total))
    };
    let preview_w = if layout_mode == LayoutMode::NoPreview { 0 } else { pct(total, preview_pct) };
    let filelist_w = total.saturating_sub(sidebar_w).saturating_sub(preview_w);

    let (sidebar, filelist, preview) = if layout_mode == LayoutMode::Right {
        let [filelist, preview, sidebar] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(filelist_w),
                Constraint::Length(preview_w),
                Constraint::Length(sidebar_w),
            ])
            .areas(body);
        (sidebar, filelist, preview)
    } else {
        let [sidebar, filelist, preview] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(sidebar_w),
                Constraint::Length(filelist_w),
                Constraint::Length(preview_w),
            ])
            .areas(body);
        (sidebar, filelist, preview)
    };

    let (preview_top, stats) = split_preview(preview);
    Panes { sidebar, filelist, preview: preview_top, stats, status }
}
