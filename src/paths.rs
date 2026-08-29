use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Resolved data directories. Configs and `.state` live under the app root:
/// `~/.tunnel-ui` for an installed binary, cwd for Cargo `target/debug|release`,
/// or `TUNNEL_HOME` if set.
#[derive(Debug, Clone)]
pub struct Paths {
    #[allow(dead_code)]
    pub root: PathBuf,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SetupPaths {
    pub dir: PathBuf,
    pub pid_file: PathBuf,
    pub child_file: PathBuf,
    pub stop_file: PathBuf,
    pub log_file: PathBuf,
    pub err_file: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        from_parts(
            env::var("TUNNEL_HOME").ok(),
            env::var("TUNNEL_CONFIG_DIR").ok(),
            env::var("TUNNEL_STATE_DIR").ok(),
            current_exe_dir(),
            env::current_dir().ok(),
            home_dir(),
        )
    }

    #[allow(dead_code)]
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            config_dir: root.join("configs"),
            state_dir: root.join(".state"),
            root,
        }
    }

    pub fn setup(&self, name: &str) -> SetupPaths {
        let dir = self.state_dir.join(name);
        SetupPaths {
            pid_file: dir.join("tunnel.pid"),
            child_file: dir.join("tunnel.child.pid"),
            stop_file: dir.join("tunnel.stop"),
            log_file: dir.join("tunnel.log"),
            err_file: dir.join("tunnel.err"),
            dir,
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        mkdir_755(&self.config_dir)?;
        mkdir_755(&self.state_dir)?;
        Ok(())
    }

    pub fn saved_config_exists(&self, name: &str) -> bool {
        if crate::config::sanitize_name(name).is_err() {
            return false;
        }
        let p = self.config_dir.join(format!("{name}.conf"));
        p.is_file()
    }
}

pub fn from_parts(
    tunnel_home: Option<String>,
    config_dir: Option<String>,
    state_dir: Option<String>,
    exe_dir: Option<PathBuf>,
    cwd: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Paths {
    let root = resolve_app_root(
        tunnel_home.as_deref().filter(|s| !s.is_empty()),
        exe_dir.as_deref(),
        cwd.as_deref(),
        home.as_deref(),
    );
    let config_dir = nonempty_path(config_dir).unwrap_or_else(|| root.join("configs"));
    let state_dir = nonempty_path(state_dir).unwrap_or_else(|| root.join(".state"));
    Paths {
        root,
        config_dir,
        state_dir,
    }
}

fn nonempty_path(v: Option<String>) -> Option<PathBuf> {
    v.filter(|s| !s.is_empty()).map(PathBuf::from)
}

fn home_dir() -> Option<PathBuf> {
    env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

pub fn resolve_app_root(
    tunnel_home: Option<&str>,
    exe_dir: Option<&Path>,
    cwd: Option<&Path>,
    home: Option<&Path>,
) -> PathBuf {
    if let Some(h) = tunnel_home.filter(|s| !s.is_empty()) {
        return PathBuf::from(h);
    }
    if let Some(dir) = exe_dir {
        if use_cwd_instead(dir) {
            return cwd
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
        }
    }
    if let Some(home) = home {
        return home.join(".tunnel-ui");
    }
    cwd.map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn use_cwd_instead(exe_dir: &Path) -> bool {
    let base = exe_dir
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if base.contains("go-build") || base.starts_with("tmp") {
        return true;
    }
    is_rust_build_dir(exe_dir)
}

fn is_rust_build_dir(dir: &Path) -> bool {
    let names: Vec<String> = dir
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    names
        .windows(2)
        .any(|w| w[0] == "target" && (w[1] == "debug" || w[1] == "release"))
}

fn current_exe_dir() -> Option<PathBuf> {
    let mut exe = env::current_exe().ok()?;
    if let Ok(resolved) = fs::canonicalize(&exe) {
        exe = resolved;
    }
    exe.parent().map(Path::to_path_buf)
}

pub fn mkdir_755(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("mkdir {}", path.display()))?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
    Ok(())
}

pub fn write_mode(path: &Path, contents: impl AsRef<[u8]>, mode: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        mkdir_755(parent)?;
    }
    fs::write(path, contents.as_ref()).with_context(|| format!("write {}", path.display()))?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    Ok(())
}

pub fn read_pid_file(path: &Path) -> i32 {
    let Ok(raw) = fs::read_to_string(path) else {
        return 0;
    };
    let s = raw.trim().trim_end_matches('\r').trim();
    s.parse().unwrap_or(0)
}

pub fn write_pid_file(path: &Path, pid: i32) -> Result<()> {
    write_mode(path, format!("{pid}\n"), 0o644)
}

pub fn write_err_file(path: &Path, msg: &str) -> Result<()> {
    write_mode(path, format!("{}\n", msg.trim()), 0o644)
}

pub fn read_err_file(path: &Path) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub fn clear_err_file(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn file_exists(path: &Path) -> bool {
    path.is_file()
}

pub fn append_log(log_file: &Path, msg: &str) {
    let _ = (|| -> Result<()> {
        if let Some(parent) = log_file.parent() {
            mkdir_755(parent)?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)?;
        writeln!(f, "[{}] {msg}", local_stamp())?;
        Ok(())
    })();
}

pub fn tail_file(path: &Path, n: usize) -> String {
    let Ok(raw) = fs::read_to_string(path) else {
        return String::new();
    };
    let s = raw.replace("\r\n", "\n");
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

fn is_supervisor_noise(line: &str) -> bool {
    let l = line.trim();
    if l.is_empty() {
        return true;
    }
    if l.chars().all(|c| c == '@' || c.is_whitespace()) {
        return true;
    }
    const NOISE: &[&str] = &[
        "Starting SSH tunnel",
        "Supervisor started",
        "reconnecting in",
        "Stop requested",
        "Tunnel stop complete",
        "SSH process exited",
        "Tunnel exited",
        "Reconnect disabled",
        "Fatal SSH error",
        "Cannot assign requested address",
    ];
    NOISE.iter().any(|n| l.contains(n))
}

/// Last ssh/stderr line that is not a supervisor heartbeat.
pub fn last_failure_line(path: &Path) -> String {
    let tail = tail_file(path, 40);
    tail.lines()
        .rev()
        .map(str::trim)
        .find(|l| !is_supervisor_noise(l))
        .unwrap_or("")
        .to_string()
}

/// `YYYY-MM-DDTHH:MM:SS±HHMM` in the local timezone.
pub fn local_stamp() -> String {
    unsafe {
        let mut t: libc::time_t = 0;
        libc::time(&mut t);
        let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
        if libc::localtime_r(&t, tm.as_mut_ptr()).is_null() {
            return "1970-01-01T00:00:00+0000".into();
        }
        let tm = tm.assume_init();
        let off = tm.tm_gmtoff;
        let sign = if off >= 0 { '+' } else { '-' };
        let abs = off.unsigned_abs();
        let oh = abs / 3600;
        let om = (abs % 3600) / 60;
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{oh:02}{om:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_home_wins() {
        let root = resolve_app_root(
            Some("/tmp/th"),
            Some(Path::new("/usr/bin")),
            Some(Path::new("/home/x")),
            Some(Path::new("/home/x")),
        );
        assert_eq!(root, PathBuf::from("/tmp/th"));
    }

    #[test]
    fn cargo_target_dir_uses_cwd() {
        let root = resolve_app_root(
            None,
            Some(Path::new("/home/x/tunnel-ui-rust/target/debug")),
            Some(Path::new("/home/x/tunnel-ui-rust")),
            Some(Path::new("/home/x")),
        );
        assert_eq!(root, PathBuf::from("/home/x/tunnel-ui-rust"));
    }

    #[test]
    fn installed_binary_uses_dot_tunnel_ui() {
        let root = resolve_app_root(
            None,
            Some(Path::new("/usr/local/bin")),
            Some(Path::new("/tmp")),
            Some(Path::new("/home/x")),
        );
        assert_eq!(root, PathBuf::from("/home/x/.tunnel-ui"));
    }

    #[test]
    fn pid_file_strips_cr() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tunnel.pid");
        fs::write(&p, "1234\r\n").unwrap();
        assert_eq!(read_pid_file(&p), 1234);
        fs::write(&p, "nope\n").unwrap();
        assert_eq!(read_pid_file(&p), 0);
    }

    #[test]
    fn last_failure_line_skips_supervisor_noise() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tunnel.log");
        fs::write(
            &p,
            "[2026-08-28T18:44:12+0700] Supervisor started (PID 1, name=x, target=h, reconnect=true)\n\
             [2026-08-28T18:44:12+0700] Starting SSH tunnel: localhost:8081 -> 8081 via h\n\
             Host key verification failed.\n\
             [2026-08-28T18:44:12+0700] SSH process exited (code 255)\n\
             [2026-08-28T18:44:12+0700] reconnecting in 3s\n",
        )
        .unwrap();
        assert_eq!(last_failure_line(&p), "Host key verification failed.");
        fs::write(
            &p,
            "Host key verification failed.\n\
             [2026-08-28T19:00:00+0700] Fatal SSH error (not reconnecting): Host key verification failed.\n",
        )
        .unwrap();
        assert_eq!(last_failure_line(&p), "Host key verification failed.");
    }
}
