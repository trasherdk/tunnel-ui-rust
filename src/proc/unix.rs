use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use libc::{self, pid_t};

use super::{basename, parse_netstat_tlnp, parse_proc_net_listen, parse_ss_lntp};

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

fn image_base(pid: i32) -> String {
    basename(&process_image(pid)).to_lowercase()
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

pub fn unix_ssh_procs() -> Vec<super::OsProc> {
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
        out.push(super::OsProc {
            pid,
            ppid,
            name,
            parent,
            cmd,
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stat_ppid_with_spaces_in_comm() {
        let stat = "1234 (some name) S 1 1234 1234 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        assert_eq!(parse_stat_ppid(stat), 1);
    }
}
