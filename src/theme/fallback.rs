use crate::theme::Theme;
use ratatui::style::Color;

/// Built-in dark theme used when Omarchy shell.toml is absent or unreadable.
pub fn fallback_theme() -> Theme {
    Theme {
        fg:           Color::Rgb(190, 249, 243),  // soft teal-white
        fg_dim:       Color::Rgb(143, 187, 182),  // dimmed teal
        fg_bright:    Color::Rgb(206, 251, 246),  // bright white-teal
        bg:           Color::Rgb(9,   4,  32),    // deep navy
        bg_dark:      Color::Rgb(7,   3,  24),    // darker navy
        bg_darker:    Color::Rgb(5,   2,  16),    // darkest navy
        bg_lighter:   Color::Rgb(34,  29, 54),    // lifted panel bg
        selection_bg: Color::Rgb(34,  29, 54),    // same as bg_lighter
        accent:       Color::Rgb(114, 122, 187),  // soft indigo
        muted:        Color::Rgb(96,  96, 102),   // grey
        red:          Color::Rgb(166, 135, 192),  // mauve
        green:        Color::Rgb(132, 202, 255),  // sky blue (Omarchy "green")
        yellow:       Color::Rgb(152, 255, 255),  // ice cyan (Omarchy "yellow")
        cyan:         Color::Rgb(162, 219, 255),  // pale sky
        blue:         Color::Rgb(114, 122, 187),  // same as accent
        magenta:      Color::Rgb(175, 157, 241),  // lavender
    }
}
