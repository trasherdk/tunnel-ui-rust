use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use libc::{self, pid_t};

/// `kill(pid, 0)`: ESRCH → dead, EPERM → alive.
pub fn pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as pid_t, 0) };
    if rc == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn process_image(pid: i32) -> String {
    if pid <= 0 {
        return String::new();
    }
    let exe = PathBuf::from(format!("/proc/{pid}/exe"));
    if let Ok(target) = fs::read_link(&exe) {
        return target.to_string_lossy().replace(" (deleted)", "");
    }
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub fn process_cmdline(pid: i32) -> String {
    if pid <= 0 {
        return String::new();
    }
    let Ok(bytes) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return String::new();
    };
    let s = String::from_utf8_lossy(&bytes);
    s.split('\0')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn ppid_of(pid: i32) -> i32 {
    if pid <= 0 {
        return 0;
    }
    if let Ok(text) = fs::read_to_string(format!("/proc/{pid}/status")) {
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("PPid:") {
                return rest.trim().parse().unwrap_or(0);
            }
        }
    }
    parse_stat_ppid(&fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default())
}

pub fn parse_stat_ppid(stat: &str) -> i32 {
    let Some((_, rest)) = stat.rsplit_once(')') else {
        return 0;
    };
    let mut fields = rest.split_whitespace();
    let _state = fields.next();
    fields.next().and_then(|s| s.parse().ok()).unwrap_or(0)
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
    image_base(pid) == "ssh"
}

pub fn is_supervisor_name(name: &str) -> bool {
    let base = basename(name).to_lowercase();
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

pub fn terminate_pid(pid: i32) {
    if pid <= 0 || !pid_alive(pid) {
        return;
    }
    unsafe {
        libc::kill(pid as pid_t, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    unsafe {
        libc::kill(pid as pid_t, libc::SIGKILL);
    }
}

#[derive(Debug, Clone)]
pub struct OsProc {
    pub pid: i32,
    pub ppid: i32,
    pub name: String,
    pub parent: String,
    pub cmd: String,
}

pub fn list_numeric_pids() -> Vec<i32> {
    let Ok(rd) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for e in rd.flatten() {
        if let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() {
            pids.push(pid);
        }
    }
    pids
}

pub fn unix_ssh_procs() -> Vec<OsProc> {
    let pids = list_numeric_pids();
    let mut by_pid: HashMap<i32, String> = HashMap::new();
    for pid in &pids {
        let comm = fs::read_to_string(format!("/proc/{pid}/comm"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if !comm.is_empty() {
            by_pid.insert(*pid, comm);
        }
    }
    let mut out = Vec::new();
    for pid in pids {
        let name = by_pid.get(&pid).cloned().unwrap_or_default();
        let img = image_base(pid);
        if !name.to_lowercase().contains("ssh") && img != "ssh" {
            continue;
        }
        let cmd = process_cmdline(pid);
        if cmd.is_empty() {
            continue;
        }
        let ppid = ppid_of(pid);
        let parent = by_pid
            .get(&ppid)
            .cloned()
            .unwrap_or_else(|| process_image(ppid));
        out.push(OsProc {
            pid,
            ppid,
            name,
            parent,
            cmd,
        });
    }
    out
}

/// Parse `/proc/net/tcp` / `tcp6` listen rows into (local_port, inode).
pub fn parse_proc_net_listen(text: &str) -> Vec<(u16, u64)> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        if fields[3] != "0A" {
            continue;
        }
        let Some((_, port_hex)) = fields[1].rsplit_once(':') else {
            continue;
        };
        let Ok(port) = u16::from_str_radix(port_hex, 16) else {
            continue;
        };
        let Ok(inode) = fields[9].parse::<u64>() else {
            continue;
        };
        if inode > 0 {
            out.push((port, inode));
        }
    }
    out
}

fn listen_inodes_for_port(port: u16) -> Vec<u64> {
    let mut inodes = Vec::new();
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(text) = fs::read_to_string(path) {
            for (p, ino) in parse_proc_net_listen(&text) {
                if p == port {
                    inodes.push(ino);
                }
            }
        }
    }
    inodes
}

fn pids_for_socket_inodes(inodes: &[u64]) -> Vec<i32> {
    if inodes.is_empty() {
        return Vec::new();
    }
    let want: HashSet<u64> = inodes.iter().copied().collect();
    let mut found: HashSet<i32> = HashSet::new();
    for pid in list_numeric_pids() {
        let fd_dir = format!("/proc/{pid}/fd");
        let Ok(rd) = fs::read_dir(&fd_dir) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(target) = fs::read_link(e.path()) else {
                continue;
            };
            let s = target.to_string_lossy();
            if let Some(rest) = s.strip_prefix("socket:[") {
                if let Some(num) = rest.strip_suffix(']') {
                    if let Ok(ino) = num.parse::<u64>() {
                        if want.contains(&ino) {
                            found.insert(pid);
                        }
                    }
                }
            }
        }
    }
    let mut pids: Vec<i32> = found.into_iter().collect();
    pids.sort_unstable();
    pids
}

pub fn parse_ss_lntp(text: &str, port: &str) -> Vec<i32> {
    let mut pids = Vec::new();
    let needle = format!(":{port}");
    for line in text.lines() {
        let u = line.to_uppercase();
        if !u.contains("LISTEN") {
            continue;
        }
        if !line.contains(&needle) {
            continue;
        }
        for cap in line.split("pid=") {
            if cap == line {
                continue;
            }
            let digits: String = cap.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(pid) = digits.parse::<i32>() {
                if pid > 0 && !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
        }
    }
    pids
}

pub fn parse_netstat_tlnp(text: &str, port: &str) -> Vec<i32> {
    let mut pids = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let listen_idx = fields.iter().position(|f| {
            let u = f.to_uppercase();
            u == "LISTEN" || u == "LISTENING"
        });
        let Some(listen_idx) = listen_idx else {
            continue;
        };
        if listen_idx < 2 {
            continue;
        }
        let local = fields[listen_idx - 2];
        let p = local
            .rsplit_once(':')
            .or_else(|| local.rsplit_once('.'))
            .map(|(_, p)| p.trim());
        if p != Some(port) {
            continue;
        }
        let tok = fields.last().copied().unwrap_or("");
        let pid = if let Ok(n) = tok.parse::<i32>() {
            n
        } else if let Some((n, _)) = tok.split_once('/') {
            n.parse().unwrap_or(0)
        } else {
            0
        };
        if pid > 0 && !pids.contains(&pid) {
            pids.push(pid);
        }
    }
    pids
}

pub fn pids_listening_on(port: &str) -> Vec<i32> {
    let port = port.trim();
    let Ok(pnum) = port.parse::<u16>() else {
        return Vec::new();
    };
    if pnum == 0 {
        return Vec::new();
    }
    let inodes = listen_inodes_for_port(pnum);
    let mut pids = pids_for_socket_inodes(&inodes);
    if pids.is_empty() && !inodes.is_empty() {
        pids = ss_or_netstat_pids(port);
    }
    if pids.is_empty() && inodes.is_empty() {
        pids = ss_or_netstat_pids(port);
    }
    pids
}

fn ss_or_netstat_pids(port: &str) -> Vec<i32> {
    if let Ok(out) = Command::new("ss").args(["-lntp"]).output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let pids = parse_ss_lntp(&text, port);
            if !pids.is_empty() {
                return pids;
            }
        }
    }
    if let Ok(out) = Command::new("netstat").args(["-tlnp"]).output() {
        if out.status.success() {
            return parse_netstat_tlnp(&String::from_utf8_lossy(&out.stdout), port);
        }
    }
    Vec::new()
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

/// Best-effort: our pid files plus live listen map for `[on]` checks.
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

    const SAMPLE_TCP: &str = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0CEA 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12345 1 0000000000000000 100 0 0 10 0
   1: 00000000:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 99 1 0000000000000000 100 0 0 10 0
   2: 0100007F:0CEA 0100007F:C001 01 00000000:00000000 00:00000000 00000000     0        0 77 1 0000000000000000 100 0 0 10 0
";

    #[test]
    fn parse_listen_hex_ports() {
        let rows = parse_proc_net_listen(SAMPLE_TCP);
        assert!(rows.contains(&(3306, 12345)), "got {rows:?}");
        assert!(rows.contains(&(80, 99)));
        assert!(!rows.iter().any(|(p, i)| *p == 3306 && *i == 77));
    }

    #[test]
    fn parse_stat_ppid_with_spaces_in_comm() {
        let stat = "1234 (some name) S 1 1234 1234 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        assert_eq!(parse_stat_ppid(stat), 1);
    }

    #[test]
    fn ss_and_netstat_parsers() {
        let ss = r#"LISTEN 0 128 127.0.0.1:3306 0.0.0.0:* users:(("ssh",pid=4242,fd=3))"#;
        assert_eq!(parse_ss_lntp(ss, "3306"), vec![4242]);
        let ns = "tcp 0 0 127.0.0.1:3306 0.0.0.0:* LISTEN 4242/ssh";
        assert_eq!(parse_netstat_tlnp(ns, "3306"), vec![4242]);
    }

    #[test]
    fn self_is_alive() {
        assert!(pid_alive(std::process::id() as i32));
        assert!(!pid_alive(0));
        assert!(!pid_alive(-1));
        assert!(!pid_alive(999_999_99));
    }

    #[test]
    fn supervisor_name_matches_test_binary() {
        assert!(is_supervisor_name("tunnel-ui"));
        assert!(is_supervisor_name("/tmp/tunnel_ui-abc123"));
        assert!(is_supervisor_name("bash"));
        assert!(!is_supervisor_name("mysqld"));
    }
}
