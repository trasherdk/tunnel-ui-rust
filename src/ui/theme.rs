use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

pub const FG: Color = Color::White;
pub const FG_MUTED: Color = Color::Rgb(211, 211, 211);
pub const FG_DARK: Color = Color::Indexed(16);
pub const BG_LIGHT: Color = Color::White;
pub const BG_CHROME: Color = Color::Cyan;

pub fn header() -> Style {
    Style::default()
        .fg(FG_DARK)
        .bg(BG_CHROME)
        .add_modifier(Modifier::BOLD)
}

pub fn footer() -> Style {
    Style::default().fg(FG_DARK).bg(BG_CHROME)
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
