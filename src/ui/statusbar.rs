// ui/statusbar.rs - Bottom bar: active prompt, transient status/error
// message, or the default keybind hint line.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use crate::ui::prompt;

const HINTS: &str = "[c]opy [x]cut [p]aste [d]trash [D]elete [r]ename [n]ewdir [t]ouch [/]search [g]oto [?]help [q]uit";

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    let line: Line = if let Some(l) = prompt::line(app, theme) {
        l
    } else if let Some(status) = &app.status {
        let color = if status.is_error { theme.red } else { theme.green };
        Line::from(Span::styled(status.text.clone(), Style::default().fg(color)))
    } else {
        Line::from(Span::styled(HINTS, Style::default().fg(theme.muted)))
    };

    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.bg_darker).fg(theme.fg)),
        area,
    );
}
