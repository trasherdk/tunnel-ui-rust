use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{bail, Result};

use crate::paths::{local_stamp, Paths};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub name: String,
    pub comment: String,
    pub auth_mode: String,
    pub ssh_target: String,
    pub remote_user: String,
    pub ssh_host: String,
    pub local_port: String,
    pub remote_host: String,
    pub remote_port: String,
    pub ssh_key: String,
    pub reconnect: String,
    pub reconnect_delay: String,
}

impl Config {
    pub fn via(&self) -> String {
        if self.auth_mode == "explicit" && !self.remote_user.is_empty() && !self.ssh_host.is_empty()
        {
            return format!("{}@{}", self.remote_user, self.ssh_host);
        }
        if !self.ssh_target.is_empty() {
            return self.ssh_target.clone();
        }
        "?".into()
    }

    pub fn forward_summary(&self) -> String {
        let lh = if self.remote_host.is_empty() {
            "127.0.0.1"
        } else {
            &self.remote_host
        };
        let rp = if self.remote_port.is_empty() {
            &self.local_port
        } else {
            &self.remote_port
        };
        format!(
            "localhost:{} -> {lh}:{rp} via {}",
            self.local_port,
            self.via()
        )
    }

    pub fn comment_line(&self) -> String {
        collapse_ws(&self.comment)
    }

    pub fn reconnect_enabled(&self) -> bool {
        self.reconnect != "0"
    }

    pub fn reconnect_delay_secs(&self) -> u64 {
        match self.reconnect_delay.parse::<i64>() {
            Ok(n) if n >= 0 => n as u64,
            _ => 3,
        }
    }
}

pub fn blank_setup() -> Config {
    Config {
        auth_mode: "alias".into(),
        local_port: "3306".into(),
        remote_host: "127.0.0.1".into(),
        remote_port: "3306".into(),
        reconnect: "1".into(),
        reconnect_delay: "3".into(),
        ..Config::default()
    }
}

pub fn apply_defaults(c: &mut Config) {
    if c.auth_mode.is_empty() {
        c.auth_mode = if !c.remote_user.is_empty() && !c.ssh_host.is_empty() {
            "explicit".into()
        } else {
            "alias".into()
        };
    }
    if c.local_port.is_empty() {
        c.local_port = "3306".into();
    }
    if c.remote_host.is_empty() {
        c.remote_host = "127.0.0.1".into();
    }
    if c.remote_port.is_empty() {
        c.remote_port = c.local_port.clone();
    }
    if c.reconnect.is_empty() {
        c.reconnect = "1".into();
    }
    if c.reconnect_delay.is_empty() {
        c.reconnect_delay = "3".into();
    }
    if c.auth_mode == "explicit"
        && c.ssh_target.is_empty()
        && !c.remote_user.is_empty()
        && !c.ssh_host.is_empty()
    {
        c.ssh_target = format!("{}@{}", c.remote_user, c.ssh_host);
    }
    if c.auth_mode == "explicit" && c.ssh_key.trim().is_empty() {
        c.ssh_key = default_ssh_key();
    }
    if c.auth_mode != "explicit" {
        c.auth_mode = "alias".into();
        c.remote_user.clear();
        c.ssh_host.clear();
    }
    c.comment = collapse_ws(&c.comment);
}

/// Linux default private key: `$HOME/.ssh/identity-rsa`.
pub fn default_ssh_key() -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => format!("{}/.ssh/identity-rsa", h.trim_end_matches('/')),
        _ => "~/.ssh/identity-rsa".into(),
    }
}

pub fn expand_ssh_key(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
    if let Some(home) = home.as_deref() {
        let home = home.trim_end_matches('/');
        if let Some(rest) = s.strip_prefix("~/") {
            return format!("{home}/{rest}");
        }
        if let Some(rest) = s.strip_prefix("$HOME/") {
            return format!("{home}/{rest}");
        }
        if let Some(rest) = s.strip_prefix("${HOME}/") {
            return format!("{home}/{rest}");
        }
    }
    s.to_string()
}

pub fn sanitize_name(name: &str) -> Result<()> {
    let err = || format!("invalid tunnel name {name:?} (use letters, digits, . _ -)");
    if name.is_empty() {
        bail!("{}", err());
    }
    for (i, ch) in name.chars().enumerate() {
        let alnum = ch.is_ascii_alphanumeric();
        let ok = alnum || ch == '_' || ch == '.' || ch == '-';
        if i == 0 && !alnum {
            bail!("{}", err());
        }
        if !ok {
            bail!("{}", err());
        }
    }
    Ok(())
}

pub fn conf_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    let simple = s.chars().all(|r| {
        r.is_ascii_alphanumeric() || matches!(r, '_' | '.' | '-' | '/' | ':' | '@' | '\\')
    });
    if simple {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

pub fn unquote_bash(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return s[1..s.len() - 1].replace(r"'\''", "'");
    }
    if let Some(u) = unquote_go_string(s) {
        return u;
    }
    s.to_string()
}

fn unquote_go_string(s: &str) -> Option<String> {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next()? {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    '\'' => out.push('\''),
                    other => out.push(other),
                }
            } else {
                out.push(c);
            }
        }
        return Some(out);
    }
    None
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn load_config_file(path: &Path) -> Result<Config> {
    let text = fs::read_to_string(path)?;
    Ok(parse_config_text(&text))
}

pub fn parse_config_text(text: &str) -> Config {
    let mut kv = std::collections::HashMap::<String, String>::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        kv.insert(k.trim().to_string(), unquote_bash(v.trim()));
    }
    Config {
        name: kv.remove("NAME").unwrap_or_default(),
        comment: kv.remove("COMMENT").unwrap_or_default(),
        auth_mode: kv.remove("AUTH_MODE").unwrap_or_default(),
        ssh_target: kv.remove("SSH_TARGET").unwrap_or_default(),
        remote_user: kv.remove("REMOTE_USER").unwrap_or_default(),
        ssh_host: kv.remove("SSH_HOST").unwrap_or_default(),
        local_port: kv.remove("LOCAL_PORT").unwrap_or_default(),
        remote_host: kv.remove("REMOTE_HOST").unwrap_or_default(),
        remote_port: kv.remove("REMOTE_PORT").unwrap_or_default(),
        ssh_key: kv.remove("SSH_KEY").unwrap_or_default(),
        reconnect: kv.remove("RECONNECT").unwrap_or_default(),
        reconnect_delay: kv.remove("RECONNECT_DELAY").unwrap_or_default(),
    }
}

pub fn load_configs(paths: &Paths) -> Result<Vec<Config>> {
    let dir = &paths.config_dir;
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    let mut names: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    names.sort_by_key(|e| e.file_name());
    for e in names {
        if e.path().is_dir() {
            continue;
        }
        let fname = e.file_name();
        let fname = fname.to_string_lossy();
        if !fname.ends_with(".conf") {
            continue;
        }
        let Ok(mut c) = load_config_file(&e.path()) else {
            continue;
        };
        if c.name.is_empty() {
            c.name = fname.trim_end_matches(".conf").to_string();
        }
        apply_defaults(&mut c);
        out.push(c);
    }
    Ok(out)
}

pub fn load_named_config(paths: &Paths, name: &str) -> Result<Config> {
    for c in load_configs(paths)? {
        if c.name == name {
            return Ok(c);
        }
    }
    bail!(
        "no saved tunnel named {name:?} (configs: {})",
        paths.config_dir.display()
    );
}

pub fn save_config(paths: &Paths, mut c: Config) -> Result<String> {
    sanitize_name(&c.name)?;
    apply_defaults(&mut c);
    if c.auth_mode == "explicit" {
        if c.remote_user.trim().is_empty() || c.ssh_host.trim().is_empty() {
            bail!("explicit setup needs user and host");
        }
    } else if c.ssh_target.trim().is_empty() {
        bail!("alias setup needs an SSH config alias");
    }
    crate::paths::mkdir_755(&paths.config_dir)?;
    let path = paths.config_dir.join(format!("{}.conf", c.name));
    let body = format!(
        "# Tunnel config: {}\n\
         # Generated by tunnel-ui {}\n\
         NAME={}\n\
         COMMENT={}\n\
         AUTH_MODE={}\n\
         SSH_TARGET={}\n\
         REMOTE_USER={}\n\
         SSH_HOST={}\n\
         LOCAL_PORT={}\n\
         REMOTE_HOST={}\n\
         REMOTE_PORT={}\n\
         SSH_KEY={}\n\
         RECONNECT={}\n\
         RECONNECT_DELAY={}\n",
        c.name,
        local_stamp(),
        conf_quote(&c.name),
        conf_quote(&c.comment),
        conf_quote(&c.auth_mode),
        conf_quote(&c.ssh_target),
        conf_quote(&c.remote_user),
        conf_quote(&c.ssh_host),
        conf_quote(&c.local_port),
        conf_quote(&c.remote_host),
        conf_quote(&c.remote_port),
        conf_quote(&c.ssh_key),
        conf_quote(&c.reconnect),
        conf_quote(&c.reconnect_delay),
    );
    fs::write(&path, body.as_bytes())?;
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o644));
    Ok(format!(
        "Saved tunnel {:?} -> {}\n  {}",
        c.name,
        path.display(),
        c.forward_summary()
    ))
}

pub fn edit_menu_keys(c: &Config, name_locked: bool) -> Vec<&'static str> {
    let mut keys = Vec::new();
    if !name_locked {
        keys.push("name");
    }
    keys.push("comment");
    keys.push("auth");
    if c.auth_mode == "explicit" {
        keys.extend(["user", "host", "key"]);
    } else {
        keys.push("target");
    }
    keys.extend([
        "local",
        "rhost",
        "rport",
        "reconnect",
        "delay",
        "save",
        "cancel",
    ]);
    keys
}

pub fn auth_label(mode: &str) -> &'static str {
    if mode == "explicit" {
        "User, host, and key"
    } else {
        "SSH config alias"
    }
}

pub fn reconnect_label(v: &str) -> &'static str {
    if v == "0" {
        "No"
    } else {
        "Yes"
    }
}

pub fn dash(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        "—".into()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_ok_and_bad() {
        sanitize_name("asus-test-root-3306").unwrap();
        assert!(sanitize_name("../etc").is_err());
        assert!(sanitize_name("").is_err());
        assert!(sanitize_name("-leading").is_err());
    }

    #[test]
    fn quote_round_trip() {
        let raw = "don't";
        assert_eq!(unquote_bash(&conf_quote(raw)), raw);
        let comment = "videoscan MySQL";
        assert_eq!(unquote_bash(&conf_quote(comment)), comment);
        assert_eq!(conf_quote(""), "''");
        assert_eq!(conf_quote("asus-test-root"), "asus-test-root");
    }

    #[test]
    fn load_comment_quoted() {
        let text = "NAME=x\nCOMMENT='videoscan MySQL'\nSSH_TARGET=alias\nLOCAL_PORT=3306\n";
        let mut c = parse_config_text(text);
        apply_defaults(&mut c);
        assert_eq!(c.comment_line(), "videoscan MySQL");
    }

    #[test]
    fn parse_mysql_example() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/configs/mysql-example.conf.example"
        );
        let mut c = load_config_file(Path::new(path)).unwrap();
        apply_defaults(&mut c);
        assert_eq!(c.name, "mysql-example");
        assert_eq!(c.comment_line(), "videoscan");
        assert_eq!(c.ssh_target, "my-ssh-alias");
        assert_eq!(c.local_port, "3306");
        assert_eq!(c.remote_host, "127.0.0.1");
        assert_eq!(c.auth_mode, "alias");
        assert_eq!(c.reconnect, "1");
        assert!(c
            .forward_summary()
            .contains("localhost:3306 -> 127.0.0.1:3306 via my-ssh-alias"));
    }

    #[test]
    fn defaults_explicit_and_alias() {
        let mut c = Config {
            remote_user: "root".into(),
            ssh_host: "h".into(),
            ..Config::default()
        };
        apply_defaults(&mut c);
        assert_eq!(c.auth_mode, "explicit");
        assert_eq!(c.ssh_target, "root@h");
        assert_eq!(c.local_port, "3306");
        assert!(
            c.ssh_key.ends_with("/.ssh/identity-rsa"),
            "ssh_key {}",
            c.ssh_key
        );

        let mut a = Config {
            auth_mode: "alias".into(),
            ssh_target: "alias".into(),
            remote_user: "keep".into(),
            ssh_host: "no".into(),
            ..Config::default()
        };
        apply_defaults(&mut a);
        assert_eq!(a.auth_mode, "alias");
        assert!(a.remote_user.is_empty());
        assert!(a.ssh_host.is_empty());
        assert!(a.ssh_key.is_empty());
    }

    #[test]
    fn save_reload_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_root(dir.path());
        paths.ensure_dirs().unwrap();
        let c = Config {
            name: "asus-test-root-3306".into(),
            comment: "videoscan - DLNA server with duplicate search/delete.".into(),
            auth_mode: "alias".into(),
            ssh_target: "asus-test-root".into(),
            local_port: "3306".into(),
            remote_host: "127.0.0.1".into(),
            remote_port: "3306".into(),
            reconnect: "1".into(),
            reconnect_delay: "3".into(),
            ..Config::default()
        };
        save_config(&paths, c.clone()).unwrap();
        let loaded = load_named_config(&paths, "asus-test-root-3306").unwrap();
        assert_eq!(loaded.comment_line(), c.comment);
        assert_eq!(loaded.ssh_target, "asus-test-root");
        assert_eq!(loaded.auth_mode, "alias");
        assert!(loaded.remote_user.is_empty());
        let body = fs::read_to_string(paths.config_dir.join("asus-test-root-3306.conf")).unwrap();
        assert!(body.contains("Generated by tunnel-ui"));
        assert!(body.contains("REMOTE_USER=''"));
    }

    #[test]
    fn edit_menu_keys_alias_and_explicit() {
        let mut alias = Config {
            auth_mode: "alias".into(),
            ssh_target: "host".into(),
            ..Config::default()
        };
        apply_defaults(&mut alias);
        let got = edit_menu_keys(&alias, true).join(",");
        assert!(got.contains("comment,auth,target,"));
        assert!(got.ends_with(",save,cancel"));
        assert!(!got.contains("name,"));

        let mut explicit = Config {
            auth_mode: "explicit".into(),
            remote_user: "root".into(),
            ssh_host: "h".into(),
            ..Config::default()
        };
        apply_defaults(&mut explicit);
        let got = edit_menu_keys(&explicit, false).join(",");
        assert!(got.contains("name,comment,auth,user,host,key,"));
        assert!(!got.contains("target"));
    }

    #[test]
    fn expand_home_ssh_key() {
        let got = default_ssh_key();
        assert!(got.ends_with("/.ssh/identity-rsa"), "{got}");
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(expand_ssh_key("${HOME}/.ssh/identity-rsa"), got);
            assert_eq!(expand_ssh_key(&format!("{home}/.ssh/identity-rsa")), got);
            assert_eq!(expand_ssh_key("~/.ssh/identity-rsa"), got);
        }
    }
}
