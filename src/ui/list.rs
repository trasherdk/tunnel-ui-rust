use crate::config::Config;
use crate::detect::DetectedTunnel;
use crate::ui::theme;

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use ratatui::Frame;

#[derive(Debug, Clone)]
pub enum Row {
    Known {
        config: Config,
        listening: bool,
        failed: bool,
    },
    Detected(DetectedTunnel),
}

impl Row {
    pub fn key(&self) -> String {
        match self {
            Row::Known { config, .. } => format!("k:{}", config.name),
            Row::Detected(d) => format!("d:{}", d.pid),
        }
    }

    pub fn height(&self) -> u16 {
        match self {
            Row::Known { config, .. } if !config.comment_line().is_empty() => 3,
            _ => 2,
        }
    }

    fn known_state(listening: bool, failed: bool) -> &'static str {
        if listening {
            "on"
        } else if failed {
            "err"
        } else {
            "off"
        }
    }

    pub fn title(&self) -> String {
        match self {
            Row::Known {
                config,
                listening,
                failed,
                ..
            } => format!(
                "[{}]  {}",
                Self::known_state(*listening, *failed),
                config.name
            ),
            Row::Detected(d) => d.label.clone(),
        }
    }

    pub fn lines(&self) -> Vec<String> {
        match self {
            Row::Known {
                config,
                listening,
                failed,
                ..
            } => {
                let mut v = vec![format!(
                    "[{}]  {}",
                    Self::known_state(*listening, *failed),
                    config.name
                )];
                let c = config.comment_line();
                if !c.is_empty() {
                    v.push(c);
                }
                if *failed && !*listening {
                    v.push(format!("{}  — ssh not listening", config.forward_summary()));
                } else {
                    v.push(config.forward_summary());
                }
                v
            }
            Row::Detected(d) => vec![d.label.clone(), d.detail.clone()],
        }
    }

    pub fn haystack(&self) -> String {
        match self {
            Row::Known { config, .. } => {
                format!(
                    "{} {} {}",
                    self.title(),
                    config.forward_summary(),
                    config.comment_line()
                )
            }
            Row::Detected(d) => format!("{} {} {}", d.label, d.detail, d.command),
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let hay = self.haystack().to_lowercase();
        q.split_whitespace().all(|tok| hay.contains(tok))
    }
}

pub fn render_rows(frame: &mut Frame, area: Rect, rows: &[Row], selected: usize, scroll_rows: u16) {
    let mut y_skip = scroll_rows;
    let mut y = area.y;
    let bottom = area.y.saturating_add(area.height);
    let mut idx = 0usize;
    for row in rows {
        let h = row.height();
        if y_skip >= h {
            y_skip -= h;
            idx += 1;
            continue;
        }
        let lines = row.lines();
        let start_line = y_skip as usize;
        y_skip = 0;
        for (li, text) in lines.iter().enumerate().skip(start_line) {
            if y >= bottom {
                return;
            }
            let sel = idx == selected;
            let style = if sel {
                if li == 0 {
                    theme::selected()
                } else {
                    theme::selected_desc()
                }
            } else if li == 0 {
                theme::title()
            } else {
                theme::desc()
            };
            let line_area = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            theme::paint_fill(frame.buffer_mut(), line_area, style);
            Line::from(Span::styled(format!(" {text}"), style))
                .render(line_area, frame.buffer_mut());
            y = y.saturating_add(1);
        }
        idx += 1;
        if y >= bottom {
            return;
        }
    }
}

pub fn total_height(rows: &[Row]) -> u16 {
    rows.iter().map(|r| r.height()).sum()
}

pub fn offset_of(rows: &[Row], index: usize) -> u16 {
    rows.iter().take(index).map(|r| r.height()).sum()
}

pub fn ensure_visible(rows: &[Row], selected: usize, area_h: u16, scroll: &mut u16) {
    if rows.is_empty() || area_h == 0 {
        *scroll = 0;
        return;
    }
    let sel = selected.min(rows.len() - 1);
    let start = offset_of(rows, sel);
    let h = rows[sel].height();
    if start < *scroll {
        *scroll = start;
    } else if start + h > *scroll + area_h {
        *scroll = start.saturating_add(h).saturating_sub(area_h);
    }
    let max_scroll = total_height(rows).saturating_sub(area_h);
    if *scroll > max_scroll {
        *scroll = max_scroll;
    }
}
