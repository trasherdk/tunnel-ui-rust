mod forms;
mod list;
mod theme;

use std::io::stdout;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use ratatui::Frame;

use crate::config::{apply_defaults, blank_setup, save_config, Config};
use crate::detect::{detect_others, DetectedTunnel};
use crate::paths::Paths;
use crate::prune::prune_orphan_state;
use crate::tunnel::{self, setup_error_hint, setup_has_error, setup_listening, setup_on};
use crate::ui::forms::{
    action_items, apply_field, backspace_at_cursor, delete_at_cursor, edit_menu_items,
    field_current, field_hint, field_title, insert_at_cursor, render_with_cursor, MenuItem,
};
use crate::ui::list::Row;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    List,
    EditMenu,
    Field,
    Actions,
    Confirm,
    Inspect,
    Result,
}

enum ConfirmOp {
    Delete(String),
    Start(String),
}

struct FieldEdit {
    key: String,
    input: String,
    cursor: usize,
    choice_idx: usize,
    err: String,
}

pub struct App {
    paths: Paths,
    screen: Screen,
    rows: Vec<Row>,
    selected: usize,
    scroll: u16,
    filter_editing: bool,
    filter: String,
    status: String,
    last_refresh: Instant,
    edit: Config,
    edit_is_new: bool,
    edit_name_locked: bool,
    edit_idx: usize,
    edit_err: String,
    field: Option<FieldEdit>,
    action_idx: usize,
    active_name: String,
    inspect: Option<DetectedTunnel>,
    result: String,
    confirm_prompt: String,
    confirm_op: Option<ConfirmOp>,
    quit: bool,
    term_area: Rect,
}

impl App {
    fn new(paths: Paths) -> Self {
        let mut app = Self {
            paths,
            screen: Screen::List,
            rows: Vec::new(),
            selected: 0,
            scroll: 0,
            filter_editing: false,
            filter: String::new(),
            status: "loading setups…".into(),
            last_refresh: Instant::now() - Duration::from_secs(10),
            edit: blank_setup(),
            edit_is_new: true,
            edit_name_locked: false,
            edit_idx: 0,
            edit_err: String::new(),
            field: None,
            action_idx: 0,
            active_name: String::new(),
            inspect: None,
            result: String::new(),
            confirm_prompt: String::new(),
            confirm_op: None,
            quit: false,
            term_area: Rect::default(),
        };
        app.refresh();
        app
    }

    fn visible_rows(&self) -> Vec<Row> {
        let filtering = !self.filter.trim().is_empty();
        self.rows
            .iter()
            .filter(|r| {
                // Detected Cursor/other ssh is inspect-only; hide it while filtering
                // saved setups so the last "other" row cannot stick around.
                if filtering && matches!(r, Row::Detected(_)) {
                    return false;
                }
                r.matches(&self.filter)
            })
            .cloned()
            .collect()
    }

    fn clamp_list_cursor(&mut self) {
        let n = self.visible_rows().len();
        if n == 0 {
            self.selected = 0;
            self.scroll = 0;
            return;
        }
        if self.selected >= n {
            self.selected = n - 1;
        }
    }

    fn refresh(&mut self) {
        let sel_key = self.visible_rows().get(self.selected).map(|r| r.key());
        prune_orphan_state(&self.paths);
        self.status = "scanning ssh processes…".into();
        let cfgs = crate::config::load_configs(&self.paths).unwrap_or_default();
        let mut rows = Vec::new();
        let mut failing = Vec::new();
        for c in &cfgs {
            let on = setup_on(&self.paths, c);
            let listening = setup_listening(c);
            let failed = setup_has_error(&self.paths, &c.name) || (on && !listening);
            if failed && !listening {
                let hint = setup_error_hint(&self.paths, &c.name);
                if hint.is_empty() {
                    failing.push(format!("{}: ssh not listening", c.name));
                } else {
                    failing.push(format!("{}: {hint}", c.name));
                }
            }
            rows.push(Row::Known {
                config: c.clone(),
                listening,
                failed,
            });
        }
        let known_n = rows.len();
        let dets = detect_others(&self.paths, &cfgs);
        let other_n = dets.len();
        for d in dets {
            rows.push(Row::Detected(d));
        }
        self.rows = rows;
        self.status = format!("{known_n} known   {other_n} other");
        if let Some(first) = failing.first() {
            self.status.push_str("   ");
            self.status.push_str(first);
            if failing.len() > 1 {
                self.status
                    .push_str(&format!(" (+{} more)", failing.len() - 1));
            }
        }
        if !self.filter_editing {
            if let Some(key) = sel_key {
                let vis = self.visible_rows();
                if let Some(i) = vis.iter().position(|r| r.key() == key) {
                    self.selected = i;
                } else if self.selected >= vis.len() {
                    self.selected = vis.len().saturating_sub(1);
                }
            }
        }
        self.last_refresh = Instant::now();
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    // Windows sends Press + Release for the same key; ignore Release.
    if key.kind == KeyEventKind::Release {
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        app.quit = true;
        return;
    }
    match app.screen {
        Screen::List => list_key(app, key),
        Screen::EditMenu => edit_menu_key(app, key),
        Screen::Field => field_key(app, key),
        Screen::Actions => actions_key(app, key),
        Screen::Confirm => confirm_key(app, key),
        Screen::Inspect => {
            if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
                app.screen = Screen::List;
                app.inspect = None;
            }
        }
        Screen::Result => {
            app.screen = Screen::List;
            app.result.clear();
            app.refresh();
        }
    }
}

fn list_key(app: &mut App, key: KeyEvent) {
    if app.filter_editing {
        match key.code {
            KeyCode::Esc => {
                app.filter_editing = false;
                app.filter.clear();
                app.clamp_list_cursor();
            }
            KeyCode::Enter => {
                app.filter_editing = false;
                app.clamp_list_cursor();
            }
            KeyCode::Backspace => {
                app.filter.pop();
                app.clamp_list_cursor();
            }
            KeyCode::Char('\u{7f}' | '\u{8}') => {
                app.filter.pop();
                app.clamp_list_cursor();
            }
            KeyCode::Char(c) if !c.is_control() => {
                app.filter.push(c);
                app.clamp_list_cursor();
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('n') => open_editor(app, blank_setup(), true),
        KeyCode::Char('r') => {
            app.status = "loading setups…".into();
            app.refresh();
        }
        KeyCode::Char('/') => {
            app.filter_editing = true;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let n = app.visible_rows().len();
            if n > 0 {
                app.selected = (app.selected + 1).min(n - 1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.selected = app.selected.saturating_sub(1);
        }
        KeyCode::Enter => activate_list_row(app),
        _ => {}
    }
}

fn activate_list_row(app: &mut App) {
    let vis = app.visible_rows();
    if let Some(row) = vis.get(app.selected) {
        match row {
            Row::Known { config, .. } => {
                app.active_name = config.name.clone();
                app.action_idx = 0;
                app.screen = Screen::Actions;
            }
            Row::Detected(d) => {
                app.inspect = Some(d.clone());
                app.screen = Screen::Inspect;
            }
        }
    }
}

fn enter_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
}

fn has_status_bar(app: &App) -> bool {
    !chrome_owned(app).1.is_empty()
}

fn body_rect(area: Rect, has_status: bool) -> Rect {
    let constraints: Vec<Constraint> = if has_status {
        vec![
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ]
    };
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area)[1]
}

fn list_hit_rect(body: Rect) -> Rect {
    Rect {
        x: body.x,
        y: body.y.saturating_add(1),
        width: body.width,
        height: body.height.saturating_sub(1),
    }
}

fn contains(area: Rect, col: u16, row: u16) -> bool {
    col >= area.x
        && col < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollDown => {
            if app.screen == Screen::List && !app.filter_editing {
                list_key(app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            }
        }
        MouseEventKind::ScrollUp => {
            if app.screen == Screen::List && !app.filter_editing {
                list_key(app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
            }
        }
        MouseEventKind::Down(MouseButton::Left) => mouse_left_down(app, mouse.column, mouse.row),
        _ => {}
    }
}

fn mouse_left_down(app: &mut App, col: u16, row: u16) {
    let area = app.term_area;
    if area.width == 0 {
        return;
    }
    let body = body_rect(area, has_status_bar(app));
    match app.screen {
        Screen::List => {
            if app.filter_editing {
                app.filter_editing = false;
            }
            let hit = list_hit_rect(body);
            if !contains(hit, col, row) {
                return;
            }
            let vis = app.visible_rows();
            let Some(i) = Row::index_at_y(&vis, app.scroll, row.saturating_sub(hit.y)) else {
                return;
            };
            if i == app.selected {
                activate_list_row(app);
            } else {
                app.selected = i;
            }
        }
        Screen::Actions | Screen::EditMenu => {
            let items_len = if app.screen == Screen::Actions {
                action_items().len()
            } else {
                edit_menu_items(&app.edit, app.edit_name_locked).len()
            };
            let Some(i) = menu_index_at(body, col, row, items_len) else {
                return;
            };
            let already = if app.screen == Screen::Actions {
                i == app.action_idx
            } else {
                i == app.edit_idx
            };
            if app.screen == Screen::Actions {
                app.action_idx = i;
            } else {
                app.edit_idx = i;
            }
            if already {
                handle_key(app, enter_key());
            }
        }
        Screen::Inspect | Screen::Result => {
            handle_key(app, enter_key());
        }
        Screen::Confirm | Screen::Field => {}
    }
}

fn menu_index_at(body: Rect, col: u16, row: u16, n: usize) -> Option<usize> {
    let inner = inset(body, 1, 1);
    if inner.height == 0 || n == 0 {
        return None;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(inner);
    let list_area = chunks[1];
    if !contains(list_area, col, row) {
        return None;
    }
    let i = row.saturating_sub(list_area.y) as usize;
    if i < n {
        Some(i)
    } else {
        None
    }
}

fn open_editor(app: &mut App, mut cfg: Config, is_new: bool) {
    apply_defaults(&mut cfg);
    app.edit = cfg;
    app.edit_is_new = is_new;
    app.edit_name_locked = !is_new && !app.edit.name.is_empty();
    app.edit_idx = 0;
    app.edit_err.clear();
    app.field = None;
    app.screen = Screen::EditMenu;
}

fn edit_menu_key(app: &mut App, key: KeyEvent) {
    let items = edit_menu_items(&app.edit, app.edit_name_locked);
    match key.code {
        KeyCode::Esc => {
            app.screen = Screen::List;
            app.refresh();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if !items.is_empty() {
                app.edit_idx = (app.edit_idx + 1).min(items.len() - 1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => app.edit_idx = app.edit_idx.saturating_sub(1),
        KeyCode::Enter => {
            if let Some(item) = items.get(app.edit_idx) {
                match item.key.as_str() {
                    "save" => save_edit(app),
                    "cancel" => {
                        app.screen = Screen::List;
                        app.refresh();
                    }
                    key => {
                        let cur = field_current(&app.edit, key);
                        let choice_idx = match key {
                            "auth" => {
                                if app.edit.auth_mode == "explicit" {
                                    1
                                } else {
                                    0
                                }
                            }
                            "reconnect" => {
                                if app.edit.reconnect == "0" {
                                    1
                                } else {
                                    0
                                }
                            }
                            _ => 0,
                        };
                        app.field = Some(FieldEdit {
                            key: key.to_string(),
                            cursor: cur.chars().count(),
                            input: cur,
                            choice_idx,
                            err: String::new(),
                        });
                        app.screen = Screen::Field;
                    }
                }
            }
        }
        _ => {}
    }
}

fn save_edit(app: &mut App) {
    apply_defaults(&mut app.edit);
    match save_config(&app.paths, app.edit.clone()) {
        Ok(_) => {
            let is_new = app.edit_is_new;
            let name = app.edit.name.clone();
            app.edit_err.clear();
            if is_new && !name.is_empty() {
                app.confirm_prompt = format!("Start tunnel '{name}'?");
                app.confirm_op = Some(ConfirmOp::Start(name));
                app.screen = Screen::Confirm;
            } else {
                app.screen = Screen::List;
                app.refresh();
            }
        }
        Err(e) => {
            app.edit_err = e.to_string();
            app.screen = Screen::EditMenu;
        }
    }
}

fn field_key(app: &mut App, key: KeyEvent) {
    if app.field.is_none() {
        app.screen = Screen::EditMenu;
        return;
    }
    let is_choice = app
        .field
        .as_ref()
        .is_some_and(|f| f.key == "auth" || f.key == "reconnect");
    match key.code {
        KeyCode::Esc => {
            app.field = None;
            app.screen = Screen::EditMenu;
        }
        KeyCode::Enter => {
            let field = app.field.as_ref().unwrap();
            let fkey = field.key.clone();
            let value = if is_choice {
                if field.key == "auth" {
                    if field.choice_idx == 1 {
                        "explicit"
                    } else {
                        "alias"
                    }
                } else if field.choice_idx == 1 {
                    "0"
                } else {
                    "1"
                }
                .to_string()
            } else {
                field.input.clone()
            };
            match apply_field(&mut app.edit, &fkey, &value) {
                Ok(()) => {
                    app.field = None;
                    app.screen = Screen::EditMenu;
                }
                Err(e) => {
                    if let Some(f) = app.field.as_mut() {
                        f.err = e;
                    }
                }
            }
        }
        KeyCode::Down | KeyCode::Up | KeyCode::Char('j') | KeyCode::Char('k') if is_choice => {
            if let Some(field) = app.field.as_mut() {
                field.choice_idx = if field.choice_idx == 0 { 1 } else { 0 };
            }
        }
        KeyCode::Left if !is_choice => {
            if let Some(field) = app.field.as_mut() {
                field.cursor = field.cursor.saturating_sub(1);
            }
        }
        KeyCode::Right if !is_choice => {
            if let Some(field) = app.field.as_mut() {
                let max = field.input.chars().count();
                if field.cursor < max {
                    field.cursor += 1;
                }
            }
        }
        KeyCode::Home if !is_choice => {
            if let Some(field) = app.field.as_mut() {
                field.cursor = 0;
            }
        }
        KeyCode::End if !is_choice => {
            if let Some(field) = app.field.as_mut() {
                field.cursor = field.input.chars().count();
            }
        }
        KeyCode::Delete if !is_choice => {
            if let Some(field) = app.field.as_mut() {
                delete_at_cursor(&mut field.input, &mut field.cursor);
            }
        }
        KeyCode::Backspace if !is_choice => {
            if let Some(field) = app.field.as_mut() {
                backspace_at_cursor(&mut field.input, &mut field.cursor);
            }
        }
        KeyCode::Char(c) if !is_choice => {
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
            {
                if key.modifiers.contains(KeyModifiers::CONTROL) && (c == 'h' || c == 'H') {
                    if let Some(field) = app.field.as_mut() {
                        backspace_at_cursor(&mut field.input, &mut field.cursor);
                    }
                }
                return;
            }
            if c == '\u{7f}' || c == '\u{8}' {
                if let Some(field) = app.field.as_mut() {
                    backspace_at_cursor(&mut field.input, &mut field.cursor);
                }
                return;
            }
            if !c.is_control() {
                if let Some(field) = app.field.as_mut() {
                    insert_at_cursor(&mut field.input, &mut field.cursor, c);
                }
            }
        }
        _ => {}
    }
}

fn run_named(app: &mut App, cmd: &str, name: &str) {
    let silent = matches!(cmd, "start" | "stop" | "delete");
    let res = tunnel::run_cli(&app.paths, &[cmd.to_string(), name.to_string()]);
    match res {
        Ok(_) if silent => {
            app.screen = Screen::List;
            app.refresh();
        }
        Ok(body) => {
            app.result = body;
            app.screen = Screen::Result;
        }
        Err(e) => {
            app.result = e.to_string();
            app.screen = Screen::Result;
        }
    }
}

fn actions_key(app: &mut App, key: KeyEvent) {
    let items = action_items();
    match key.code {
        KeyCode::Esc => {
            app.screen = Screen::List;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.action_idx = (app.action_idx + 1).min(items.len() - 1);
        }
        KeyCode::Up | KeyCode::Char('k') => app.action_idx = app.action_idx.saturating_sub(1),
        KeyCode::Enter => {
            let name = app.active_name.clone();
            match items.get(app.action_idx).map(|i| i.key.as_str()) {
                Some("start") => run_named(app, "start", &name),
                Some("stop") => run_named(app, "stop", &name),
                Some("status") => run_named(app, "status", &name),
                Some("edit") => match crate::config::load_named_config(&app.paths, &name) {
                    Ok(c) => open_editor(app, c, false),
                    Err(e) => {
                        app.result = e.to_string();
                        app.screen = Screen::Result;
                    }
                },
                Some("delete") => {
                    app.confirm_prompt =
                        format!("Delete saved tunnel '{name}'? (Stops it if running.)");
                    app.confirm_op = Some(ConfirmOp::Delete(name));
                    app.screen = Screen::Confirm;
                }
                _ => app.screen = Screen::List,
            }
        }
        _ => {}
    }
}

fn confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.confirm_op = None;
            app.screen = Screen::List;
            app.refresh();
        }
        KeyCode::Enter => match app.confirm_op.take() {
            Some(ConfirmOp::Delete(name)) => run_named(app, "delete", &name),
            Some(ConfirmOp::Start(name)) => run_named(app, "start", &name),
            None => app.screen = Screen::List,
        },
        _ => {}
    }
}

fn render(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    app.term_area = area;
    let (title, status, keys) = chrome_owned(app);
    let has_status = !status.is_empty();
    let constraints = if has_status {
        vec![
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    fill_area(frame, area, theme::root());
    fill_bar(frame, chunks[0], &format!(" {title}"), theme::header());
    let body = chunks[1];
    fill_area(frame, body, theme::root());
    if has_status {
        fill_bar(frame, chunks[2], &format!(" {status}"), theme::status());
        fill_bar(frame, chunks[3], &format!(" {keys}"), theme::footer());
    } else {
        fill_bar(frame, chunks[2], &format!(" {keys}"), theme::footer());
    }
    match app.screen {
        Screen::List => render_list(app, frame, body),
        Screen::EditMenu => render_menu(
            frame,
            body,
            &edit_menu_items(&app.edit, app.edit_name_locked),
            app.edit_idx,
            if app.edit_err.is_empty() {
                "Select a setting to change, then Save to write the file and return."
            } else {
                &app.edit_err
            },
        ),
        Screen::Field => render_field(app, frame, body),
        Screen::Actions => {
            let items = action_items();
            render_menu(
                frame,
                body,
                &items,
                app.action_idx,
                &format!("Tunnel: {}", app.active_name),
            );
        }
        Screen::Confirm => {
            let p = Paragraph::new(app.confirm_prompt.clone())
                .style(theme::title())
                .wrap(Wrap { trim: false });
            frame.render_widget(p, inset(body, 1, 2));
        }
        Screen::Inspect => {
            if let Some(d) = &app.inspect {
                let text = format!(
                    "{}\nPID {}\n{}\n\n{}\n\nThis process is not managed here (Cursor / other tools).",
                    d.label, d.pid, d.detail, d.command
                );
                let p = Paragraph::new(text)
                    .style(theme::title())
                    .wrap(Wrap { trim: false });
                frame.render_widget(p, inset(body, 1, 2));
            }
        }
        Screen::Result => {
            let p = Paragraph::new(format!("{}\n\nPress any key to return.", app.result))
                .style(theme::title())
                .wrap(Wrap { trim: false });
            frame.render_widget(p, inset(body, 1, 2));
        }
    }
}

fn chrome_owned(app: &App) -> (String, String, &'static str) {
    match app.screen {
        Screen::List => {
            let status = if app.filter_editing {
                format!("filter: {}_", app.filter)
            } else if !app.filter.is_empty() {
                format!("{}   /{}", app.status, app.filter)
            } else {
                app.status.clone()
            };
            (format!("SSH tunnels  {VERSION}"), status, theme::LIST_KEYS)
        }
        Screen::EditMenu => {
            let title = if app.edit_is_new {
                "New setup".into()
            } else if app.edit.name.is_empty() {
                "Edit".into()
            } else {
                format!("Edit: {}", app.edit.name)
            };
            (title, app.edit_err.clone(), theme::EDIT_KEYS)
        }
        Screen::Field => ("Edit setting".into(), String::new(), theme::FIELD_KEYS),
        Screen::Actions => (
            format!("Tunnel: {}", app.active_name),
            String::new(),
            theme::ACTION_KEYS,
        ),
        Screen::Confirm => ("Confirm".into(), String::new(), theme::CONFIRM_KEYS),
        Screen::Inspect => ("Detected tunnel".into(), String::new(), theme::INSPECT_KEYS),
        Screen::Result => ("Result".into(), String::new(), theme::RESULT_KEYS),
    }
}

fn fill_area(frame: &mut Frame, area: Rect, style: ratatui::style::Style) {
    theme::paint_fill(frame.buffer_mut(), area, style);
}

fn fill_bar(frame: &mut Frame, area: Rect, text: &str, style: ratatui::style::Style) {
    fill_area(frame, area, style);
    let width = area.width as usize;
    let mut line = text.to_string();
    if line.chars().count() < width {
        line.extend(std::iter::repeat_n(
            ' ',
            width.saturating_sub(line.chars().count()),
        ));
    }
    Line::from(Span::styled(line, style)).render(area, frame.buffer_mut());
}

fn inset(area: Rect, dy: u16, dx: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(dx),
        y: area.y.saturating_add(dy),
        width: area.width.saturating_sub(dx.saturating_mul(2)),
        height: area.height.saturating_sub(dy.saturating_mul(2)),
    }
}

fn render_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let list_area = Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    let vis = app.visible_rows();
    app.clamp_list_cursor();
    list::ensure_visible(&vis, app.selected, list_area.height, &mut app.scroll);
    list::render_rows(frame, list_area, &vis, app.selected, app.scroll);
}

fn render_menu(frame: &mut Frame, area: Rect, items: &[MenuItem], selected: usize, hint: &str) {
    fill_area(frame, area, theme::root());
    let inner = inset(area, 1, 1);
    if inner.height == 0 {
        return;
    }
    let hint_p = Paragraph::new(hint)
        .style(theme::desc())
        .wrap(Wrap { trim: true });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(inner);
    fill_area(frame, chunks[0], theme::desc());
    frame.render_widget(hint_p, chunks[0]);
    let list_area = chunks[1];
    let mut y = list_area.y;
    for (i, item) in items.iter().enumerate() {
        if y >= list_area.y.saturating_add(list_area.height) {
            break;
        }
        let style = if i == selected {
            theme::selected()
        } else {
            theme::title()
        };
        let r = Rect {
            x: list_area.x,
            y,
            width: list_area.width,
            height: 1,
        };
        fill_area(frame, r, style);
        Line::from(Span::styled(format!(" {}", item.label), style)).render(r, frame.buffer_mut());
        y = y.saturating_add(1);
    }
}

fn render_field(app: &App, frame: &mut Frame, area: Rect) {
    let Some(field) = &app.field else { return };
    fill_area(frame, area, theme::root());
    let inner = inset(area, 1, 2);
    let title = field_title(&field.key);
    let hint = field_hint(&field.key);
    let mut lines = vec![title.to_string()];
    if !hint.is_empty() {
        lines.push(hint.to_string());
    }
    if field.key == "auth" {
        lines.push(String::new());
        lines.push(choice_line(field.choice_idx == 0, "SSH config alias"));
        lines.push(choice_line(field.choice_idx == 1, "User, host, and key"));
    } else if field.key == "reconnect" {
        lines.push(String::new());
        lines.push(choice_line(field.choice_idx == 0, "Yes"));
        lines.push(choice_line(field.choice_idx == 1, "No"));
    } else {
        lines.push(String::new());
        lines.push(render_with_cursor(&field.input, field.cursor));
    }
    if !field.err.is_empty() {
        lines.push(String::new());
        lines.push(field.err.clone());
    }
    let p = Paragraph::new(lines.join("\n"))
        .style(theme::title())
        .wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn choice_line(sel: bool, label: &str) -> String {
    if sel {
        format!("> {label}")
    } else {
        format!("  {label}")
    }
}

pub fn run(paths: Paths) -> Result<()> {
    let mut terminal = ratatui::init();
    crate::console::apply_tui_console_mode();
    execute!(stdout(), EnableMouseCapture)?;
    terminal.clear()?;
    let result = (|| {
        let mut app = App::new(paths);
        let mut drawn_screen = app.screen;
        loop {
            if app.screen != drawn_screen {
                terminal.clear()?;
                drawn_screen = app.screen;
            }
            terminal.draw(|frame| render(&mut app, frame))?;
            if event::poll(Duration::from_millis(200))? {
                match event::read()? {
                    Event::Key(key) => handle_key(&mut app, key),
                    Event::Mouse(mouse) => handle_mouse(&mut app, mouse),
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
            if app.quit {
                break;
            }
            if app.screen == Screen::List
                && !app.filter_editing
                && app.last_refresh.elapsed() >= Duration::from_secs(5)
            {
                app.refresh();
            }
        }
        Ok(())
    })();
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    println!();
    result
}
