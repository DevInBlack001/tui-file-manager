// ui/prompt.rs - Inline input line rendering (rename, mkdir, touch, symlink,
// search, goto-path, delete confirmation). Rendered inside the status bar
// row rather than as a floating popup, matching the BUILD_PLAN layout.

use ratatui::text::{Line, Span};
use ratatui::style::Style;

use crate::app::{App, Input, Mode};
use crate::theme::Theme;

/// The line to show in the status bar for the current mode, or `None` when
/// the status bar should show ordinary keybind hints / status text instead.
pub fn line<'a>(app: &'a App, theme: &Theme) -> Option<Line<'a>> {
    match &app.mode {
        Mode::Search => Some(edit_line("/", &Input::from(app.filter.clone()), theme)),
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

fn edit_line<'a>(prefix: &str, input: &Input, theme: &Theme) -> Line<'a> {
    let text = Style::default().fg(theme.fg_bright);
    let before: String = input.text.chars().take(input.cursor).collect();
    let mut rest = input.text.chars().skip(input.cursor);
    // The char under the cursor is drawn inverted; at the end a block shows.
    let (under, after) = match rest.next() {
        Some(c) => (c.to_string(), rest.collect::<String>()),
        None => ("\u{2588}".to_string(), String::new()),
    };
    let cursor_style = if after.is_empty() && input.cursor >= input.text.chars().count() {
        text
    } else {
        Style::default().fg(theme.bg_darker).bg(theme.fg_bright)
    };
    Line::from(vec![
        Span::styled(prefix.to_string(), Style::default().fg(theme.accent)),
        Span::styled(before, text),
        Span::styled(under, cursor_style),
        Span::styled(after, text),
    ])
}
