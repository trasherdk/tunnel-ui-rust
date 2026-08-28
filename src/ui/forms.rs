use crate::config::{auth_label, dash, edit_menu_keys, reconnect_label, Config};

pub struct MenuItem {
    pub key: String,
    pub label: String,
}

pub fn edit_menu_items(cfg: &Config, name_locked: bool) -> Vec<MenuItem> {
    let mut items = Vec::new();
    let push = |items: &mut Vec<MenuItem>, key: &str, label: &str, value: &str| {
        items.push(MenuItem {
            key: key.into(),
            label: format!("{:<18} {}", label, dash(value)),
        });
    };
    if !name_locked {
        push(&mut items, "name", "Name", &cfg.name);
    }
    push(&mut items, "comment", "Comment", &cfg.comment_line());
    push(&mut items, "auth", "Setup type", auth_label(&cfg.auth_mode));
    if cfg.auth_mode == "explicit" {
        push(&mut items, "user", "User", &cfg.remote_user);
        push(&mut items, "host", "Host", &cfg.ssh_host);
        push(&mut items, "key", "SSH key", &cfg.ssh_key);
    } else {
        push(&mut items, "target", "SSH alias", &cfg.ssh_target);
    }
    push(&mut items, "local", "Local port", &cfg.local_port);
    push(&mut items, "rhost", "Remote host", &cfg.remote_host);
    push(&mut items, "rport", "Remote port", &cfg.remote_port);
    push(
        &mut items,
        "reconnect",
        "Auto-reconnect",
        reconnect_label(&cfg.reconnect),
    );
    push(&mut items, "delay", "Reconnect delay", &cfg.reconnect_delay);
    items.push(MenuItem {
        key: "save".into(),
        label: "Save               write file and return".into(),
    });
    items.push(MenuItem {
        key: "cancel".into(),
        label: "Cancel             discard changes".into(),
    });
    let _ = edit_menu_keys(cfg, name_locked);
    items
}

pub fn action_items() -> Vec<MenuItem> {
    ["start", "stop", "status", "edit", "delete", "back"]
        .into_iter()
        .map(|k| MenuItem {
            key: k.into(),
            label: match k {
                "start" => "Start".into(),
                "stop" => "Stop".into(),
                "status" => "Status".into(),
                "edit" => "Edit".into(),
                "delete" => "Delete".into(),
                _ => "Back".into(),
            },
        })
        .collect()
}

pub fn field_title(field: &str) -> &'static str {
    match field {
        "name" => "Name",
        "comment" => "Comment",
        "auth" => "Setup type",
        "target" => "SSH config alias",
        "user" => "User",
        "host" => "Host",
        "key" => "SSH private key path",
        "local" => "Local port",
        "rhost" => "Remote host",
        "rport" => "Remote port",
        "reconnect" => "Auto-reconnect",
        "delay" => "Reconnect delay (seconds)",
        _ => "Edit setting",
    }
}

pub fn field_hint(field: &str) -> &'static str {
    match field {
        "name" => "Saved as configs/<name>.conf",
        "comment" => "What this tunnel is for, e.g. videoscan",
        "target" => "Host entry from ~/.ssh/config",
        "key" => "Default ${HOME}/.ssh/identity-rsa. Arrows move the cursor.",
        "rhost" => "As seen from the SSH host",
        _ => "",
    }
}

pub fn apply_field(cfg: &mut Config, field: &str, value: &str) -> Result<(), String> {
    let v = value.trim().to_string();
    match field {
        "name" => {
            if v.is_empty() {
                return Err("name is required".into());
            }
            crate::config::sanitize_name(&v).map_err(|e| e.to_string())?;
            cfg.name = v;
        }
        "comment" => cfg.comment = v,
        "auth" => {
            cfg.auth_mode = if v == "explicit" { "explicit" } else { "alias" }.into();
            crate::config::apply_defaults(cfg);
        }
        "target" => {
            if v.is_empty() {
                return Err("alias is required".into());
            }
            cfg.ssh_target = v;
        }
        "user" => {
            if v.is_empty() {
                return Err("user is required".into());
            }
            cfg.remote_user = v;
        }
        "host" => {
            if v.is_empty() {
                return Err("host is required".into());
            }
            cfg.ssh_host = v;
        }
        "key" => {
            let v = if v.is_empty() {
                crate::config::default_ssh_key()
            } else {
                crate::config::expand_ssh_key(&v)
            };
            if v.is_empty() {
                return Err("ssh key is required".into());
            }
            cfg.ssh_key = v;
        }
        "local" => {
            if v.is_empty() {
                return Err("local port is required".into());
            }
            cfg.local_port = v;
        }
        "rhost" => cfg.remote_host = v,
        "rport" => cfg.remote_port = v,
        "reconnect" => cfg.reconnect = if v == "0" { "0" } else { "1" }.into(),
        "delay" => cfg.reconnect_delay = v,
        _ => {}
    }
    Ok(())
}

pub fn field_current(cfg: &Config, field: &str) -> String {
    match field {
        "name" => cfg.name.clone(),
        "comment" => cfg.comment.clone(),
        "auth" => cfg.auth_mode.clone(),
        "target" => cfg.ssh_target.clone(),
        "user" => cfg.remote_user.clone(),
        "host" => cfg.ssh_host.clone(),
        "key" => {
            if cfg.ssh_key.is_empty() {
                crate::config::default_ssh_key()
            } else {
                cfg.ssh_key.clone()
            }
        }
        "local" => cfg.local_port.clone(),
        "rhost" => cfg.remote_host.clone(),
        "rport" => cfg.remote_port.clone(),
        "reconnect" => cfg.reconnect.clone(),
        "delay" => cfg.reconnect_delay.clone(),
        _ => String::new(),
    }
}

pub fn char_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

pub fn insert_at_cursor(input: &mut String, cursor: &mut usize, c: char) {
    let i = char_byte_index(input, *cursor);
    input.insert(i, c);
    *cursor = cursor.saturating_add(1);
}

pub fn backspace_at_cursor(input: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    *cursor -= 1;
    let i = char_byte_index(input, *cursor);
    if i < input.len() {
        input.remove(i);
    }
}

pub fn delete_at_cursor(input: &mut String, cursor: &mut usize) {
    let i = char_byte_index(input, *cursor);
    if i < input.len() {
        input.remove(i);
    }
}

pub fn render_with_cursor(input: &str, cursor: usize) -> String {
    let i = char_byte_index(input, cursor);
    format!("{}█{}", &input[..i], &input[i..])
}
