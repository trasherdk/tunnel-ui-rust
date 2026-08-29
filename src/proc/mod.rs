mod parse;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::path::Path;

pub use parse::parse_netstat_tlnp;
#[cfg(unix)]
pub use parse::{parse_proc_net_listen, parse_ss_lntp};
#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use windows::*;

#[derive(Debug, Clone)]
pub struct OsProc {
    pub pid: i32,
    pub ppid: i32,
    pub name: String,
    pub parent: String,
    pub cmd: String,
}

pub fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn image_base(pid: i32) -> String {
    basename(&process_image(pid)).to_lowercase()
}

pub fn is_ssh_process(pid: i32) -> bool {
    let b = image_base(pid);
    let b = b.strip_suffix(".exe").unwrap_or(&b);
    b == "ssh"
}

pub fn is_supervisor_name(name: &str) -> bool {
    let mut base = basename(name).to_lowercase();
    if let Some(stripped) = base.strip_suffix(".exe") {
        base = stripped.to_string();
    }
    matches!(base.as_str(), "bash" | "sh" | "dash")
        || base.contains("tunnel-ui")
        || base.contains("tunnel_ui")
}

pub fn is_managed_process(pid: i32) -> bool {
    if is_ssh_process(pid) {
        return true;
    }
    is_supervisor_name(&process_image(pid))
}

pub fn ssh_holding_port(port: &str) -> bool {
    pids_listening_on(port)
        .iter()
        .any(|pid| is_ssh_process(*pid))
}

pub fn ssh_holding_port_live(port: &str) -> bool {
    pids_listening_on(port)
        .iter()
        .any(|pid| is_ssh_process(*pid) || is_supervisor_name(&process_image(*pid)))
}

pub fn port_listeners_named(port: &str) -> (Vec<String>, Vec<String>) {
    let mut ssh = Vec::new();
    let mut other = Vec::new();
    for pid in pids_listening_on(port) {
        let mut name = basename(&process_image(pid));
        if name.is_empty() {
            name = "?".into();
        }
        let item = format!("{name} ({pid})");
        if is_ssh_process(pid) {
            ssh.push(item);
        } else {
            other.push(item);
        }
    }
    (ssh, other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervisor_name_matches_test_binary() {
        assert!(is_supervisor_name("tunnel-ui"));
        assert!(is_supervisor_name("/tmp/tunnel_ui-abc123"));
        assert!(is_supervisor_name("bash"));
        assert!(is_supervisor_name("tunnel-ui.exe"));
        assert!(!is_supervisor_name("mysqld"));
    }

    #[test]
    fn self_is_alive() {
        assert!(pid_alive(std::process::id() as i32));
        assert!(!pid_alive(0));
        assert!(!pid_alive(-1));
        assert!(!pid_alive(999_999_99));
    }
}
