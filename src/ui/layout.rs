// ui/layout.rs - Panel sizing: sidebar / file list / preview+stats splits.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::config::SidebarPosition;

pub struct Panes {
    pub sidebar: Rect,
    pub filelist: Rect,
    pub preview: Rect,
    pub stats: Rect,
    pub status: Rect,
}

const MIN_SIDEBAR_COLS: u16 = 16;
/// Stats always shows a fixed, small set of fields (see ui/preview.rs's
/// render_stats): up to ~11 content rows plus 2 border rows. Giving it a
/// fixed height rather than a percentage of the pane means Preview gets
/// every remaining row instead of being capped at an arbitrary split - the
/// preview pane (especially for images) benefits far more from extra height
/// than the fixed-size stats block ever would.
const STATS_HEIGHT: u16 = 13;

/// Split the terminal area into sidebar / file list / preview+stats / status
/// bar.
///
/// `sidebar_pct` and `preview_pct` come from config; the sidebar never goes
/// below `MIN_SIDEBAR_COLS`, as long as the terminal is wide enough to
/// afford it (on a pathologically narrow terminal it degrades gracefully
/// down to whatever width is available rather than panicking).
pub fn compute(area: Rect, sidebar_pct: u8, preview_pct: u8, sidebar_position: SidebarPosition) -> Panes {
    let [body, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .areas(area);

    let total = body.width;
    let sidebar_w = ((total as u32 * sidebar_pct.min(90) as u32) / 100) as u16;
    let sidebar_w = sidebar_w.max(MIN_SIDEBAR_COLS.min(total));
    let preview_w = ((total as u32 * preview_pct.min(90) as u32) / 100) as u16;
    let filelist_w = total.saturating_sub(sidebar_w).saturating_sub(preview_w);

    let (sidebar, filelist, preview) = match sidebar_position {
        SidebarPosition::Left => {
            let [sidebar, filelist, preview] = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(sidebar_w),
                    Constraint::Length(filelist_w),
                    Constraint::Length(preview_w),
                ])
                .areas(body);
            (sidebar, filelist, preview)
        }
        SidebarPosition::Right => {
            let [filelist, preview, sidebar] = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(filelist_w),
                    Constraint::Length(preview_w),
                    Constraint::Length(sidebar_w),
                ])
                .areas(body);
            (sidebar, filelist, preview)
        }
    };

    let [preview_top, stats] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(STATS_HEIGHT)])
        .areas(preview);

    Panes {
        sidebar,
        filelist,
        preview: preview_top,
        stats,
        status,
    }
}
