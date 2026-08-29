use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// ANSI 16-color only. RGB / 256-index colors are ignored on the Linux
/// console (and other 16-color ttys), which leaves black-on-black bars.
pub const FG: Color = Color::White;
pub const FG_MUTED: Color = Color::White;
pub const FG_DARK: Color = Color::Black;
pub const BG_LIGHT: Color = Color::White;
/// Bright cyan (ANSI 14) — the closest 16-color “light blue”.
pub const BG_HEADER: Color = Color::LightCyan;

pub fn header() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(BG_HEADER)
        .add_modifier(Modifier::BOLD)
}

pub fn footer() -> Style {
    Style::default().fg(Color::Black).bg(BG_HEADER)
}

pub fn selected() -> Style {
    Style::default()
        .fg(FG_DARK)
        .bg(BG_LIGHT)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_desc() -> Style {
    Style::default().fg(FG_DARK).bg(BG_LIGHT)
}

/// Full-frame / body fill so leftover terminal text cannot show through.
pub fn root() -> Style {
    Style::default().fg(FG).bg(Color::Black)
}

pub fn title() -> Style {
    Style::default()
        .fg(FG)
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD)
}

pub fn desc() -> Style {
    Style::default().fg(FG_MUTED).bg(Color::Black)
}

pub fn status() -> Style {
    Style::default().fg(FG).bg(Color::Black)
}

pub fn state_on(selected: bool) -> Style {
    state_color(Color::Green, selected)
}

pub fn state_off(selected: bool) -> Style {
    state_color(Color::Red, selected)
}

pub fn state_err(selected: bool) -> Style {
    state_color(Color::Yellow, selected)
}

fn state_color(fg: Color, selected: bool) -> Style {
    let bg = if selected { BG_LIGHT } else { Color::Black };
    Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
}

pub const LIST_KEYS: &str = "enter actions   n new   r refresh   / filter   q quit";
pub const EDIT_KEYS: &str = "enter change   save writes file   esc cancel";
pub const FIELD_KEYS: &str = "enter ok   esc back";
pub const ACTION_KEYS: &str = "enter run   esc back";
pub const CONFIRM_KEYS: &str = "enter confirm   esc cancel";
pub const INSPECT_KEYS: &str = "enter back";
pub const RESULT_KEYS: &str = "any key back";

/// Write a space + style into every cell. `Block`/`Clear` alone can leave glyphs
/// behind because they often only change style, and the backend diffs skip
/// "unchanged" symbols that are still on the physical terminal.
pub fn paint_fill(buf: &mut Buffer, area: Rect, style: Style) {
    let area = area.intersection(buf.area);
    if area.width == 0 || area.height == 0 {
        return;
    }
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            let cell = &mut buf[(x, y)];
            cell.reset();
            cell.set_symbol(" ");
            cell.set_style(style);
        }
    }
}
