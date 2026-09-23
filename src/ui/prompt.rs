// ui/prompt.rs - Inline input line rendering (rename, mkdir, touch, symlink,
// search, goto-path, delete confirmation). Rendered inside the status bar
// row rather than as a floating popup, matching the BUILD_PLAN layout.

use ratatui::text::{Line, Span};
use ratatui::style::Style;

use crate::app::{App, Mode};
use crate::theme::Theme;

/// The line to show in the status bar for the current mode, or `None` when
/// the status bar should show ordinary keybind hints / status text instead.
pub fn line<'a>(app: &'a App, theme: &Theme) -> Option<Line<'a>> {
    match &app.mode {
        Mode::Search => Some(edit_line("/", &app.filter, theme)),
        Mode::GotoPath(buf) => Some(edit_line("goto: ", buf, theme)),
        Mode::Prompt(kind, buf) => Some(edit_line(&format!("{}: ", kind.label()), buf, theme)),
        Mode::ConfirmInstallEditor => Some(Line::from(Span::styled(
            "nvim is not installed - install it now via sudo pacman -S neovim? [y/N]",
            Style::default().fg(theme.yellow),
        ))),
        Mode::ConfirmDelete(buf) => Some(Line::from(vec![
            Span::styled(
                "type 'yes' to permanently delete, Esc to cancel: ",
                Style::default().fg(theme.red),
            ),
            Span::styled(buf.clone(), Style::default().fg(theme.fg_bright)),
            Span::styled("\u{2588}", Style::default().fg(theme.fg_bright)),
        ])),
        _ => None,
    }
}

fn edit_line<'a>(prefix: &str, buf: &str, theme: &Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled(prefix.to_string(), Style::default().fg(theme.accent)),
        Span::styled(buf.to_string(), Style::default().fg(theme.fg_bright)),
        Span::styled("\u{2588}", Style::default().fg(theme.fg_bright)),
    ])
}
