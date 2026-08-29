use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

use crate::config::Config;
use crate::paths::Paths;
use crate::proc;

#[derive(Debug, Clone)]
pub struct DetectedTunnel {
    pub pid: i32,
    #[allow(dead_code)]
    pub kind: String,
    pub label: String,
    pub detail: String,
    pub command: String,
    #[allow(dead_code)]
    pub local_ports: Vec<String>,
}

fn re_local_fwd() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"-L\s*(?:\S+@)?(?:\[[^\]]+\]:)?(?:[^:\s]+:)?(\d+):").unwrap())
}

fn re_dyn_fwd() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s)-D\s*(\d+)").unwrap())
}

fn re_has_t() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s)-T(?:\s|$)").unwrap())
}

pub fn cmd_forwards_port(cmd: &str, port: &str) -> bool {
    let port = port.trim();
    if port.is_empty() || cmd.trim().is_empty() {
        return false;
    }
    for lp in unique_caps(re_local_fwd().captures_iter(cmd)) {
        if lp == port {
            return true;
        }
    }
    for n in [
        format!("-L{port}:"),
        format!("-L {port}:"),
        format!("-L{port} "),
        format!("-L {port} "),
    ] {
        if cmd.contains(&n) {
            return true;
        }
    }
    false
}

pub fn cmdline_supervises(cmd: &str, name: &str, target: &str) -> bool {
    let Some(i) = cmd.find("_supervisor") else {
        return false;
    };
    let rest = cmd[i + "_supervisor".len()..].trim();
    let arg = rest.split_whitespace().next().unwrap_or("");
    arg == name || (!target.is_empty() && arg == target)
}

fn unique_caps<'a>(iter: impl Iterator<Item = regex::Captures<'a>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for cap in iter {
        let Some(m) = cap.get(1) else { continue };
        let v = m.as_str();
        if v.is_empty() || !seen.insert(v.to_string()) {
            continue;
        }
        out.push(v.to_string());
    }
    out
}

pub fn last_ssh_host(cmd: &str) -> String {
    let fields = split_command_line(cmd);
    let mut host = String::new();
    let mut skip_next = false;
    for (i, f) in fields.iter().enumerate() {
        if i == 0 {
            continue;
        }
        if skip_next {
            skip_next = false;
            continue;
        }
        if f == "--" && i + 1 < fields.len() {
            return fields[i + 1].clone();
        }
        if f.starts_with('-') {
            if f.len() == 2 && f.chars().nth(1).is_some_and(|c| "ioFLEDR".contains(c)) {
                skip_next = true;
            }
            continue;
        }
        host = f.clone();
    }
    host
}

pub fn split_command_line(cmd: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut buf = String::new();
    let mut in_quote: Option<char> = None;
    for c in cmd.chars() {
        if let Some(q) = in_quote {
            if c == q {
                in_quote = None;
            } else {
                buf.push(c);
            }
            continue;
        }
        match c {
            ' ' | '\t' => {
                if !buf.is_empty() {
                    fields.push(std::mem::take(&mut buf));
                }
            }
            '\'' | '"' => in_quote = Some(c),
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        fields.push(buf);
    }
    fields
}

pub fn classify_ssh(
    cmd: &str,
    parent: &str,
    local_ports: &[String],
    dyn_ports: &[String],
) -> (String, String) {
    let has_t = re_has_t().is_match(cmd);
    let parent_l = parent.to_lowercase();
    let is_cursor_parent = parent_l.contains("cursor") || parent_l.contains("code");
    if (has_t && !dyn_ports.is_empty()) || (is_cursor_parent && !dyn_ports.is_empty()) {
        let host = last_ssh_host(cmd);
        let mut label = "Cursor Remote-SSH".to_string();
        if parent_l.contains("code") && !parent_l.contains("cursor") {
            label = "VS Code Remote-SSH".into();
        }
        if !host.is_empty() {
            label.push(' ');
            label.push_str(&host);
        }
        if !dyn_ports.is_empty() {
            label.push_str(" (socks :");
            label.push_str(&dyn_ports[0]);
            label.push(')');
        }
        return ("remote-ssh".into(), label);
    }
    if !local_ports.is_empty() {
        return (
            "forward".into(),
            format!("other forward localhost:{}", local_ports.join(",")),
        );
    }
    (String::new(), String::new())
}

fn summarize_detected(cmd: &str, local_ports: &[String], dyn_ports: &[String]) -> String {
    let mut parts = Vec::new();
    if !local_ports.is_empty() {
        parts.push(format!("-L :{}", local_ports.join(",")));
    }
    if !dyn_ports.is_empty() {
        parts.push(format!("-D :{}", dyn_ports.join(",")));
    }
    let host = last_ssh_host(cmd);
    if !host.is_empty() {
        parts.push(host);
    }
    if parts.is_empty() {
        if cmd.len() > 120 {
            format!("{}...", &cmd[..117])
        } else {
            cmd.to_string()
        }
    } else {
        parts.join("  ")
    }
}

pub fn known_child_pids(paths: &Paths) -> HashSet<i32> {
    let mut out = HashSet::new();
    let Ok(rd) = std::fs::read_dir(&paths.state_dir) else {
        return out;
    };
    for e in rd.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let sp = paths.setup(&e.file_name().to_string_lossy());
        for f in [sp.child_file, sp.pid_file] {
            let pid = crate::paths::read_pid_file(&f);
            if pid > 0 {
                out.insert(pid);
            }
        }
    }
    out
}

pub fn detect_others(paths: &Paths, known: &[Config]) -> Vec<DetectedTunnel> {
    let ours = known_child_pids(paths);
    let claimed: HashMap<String, bool> = known
        .iter()
        .filter(|c| !c.local_port.is_empty())
        .map(|c| (c.local_port.clone(), true))
        .collect();

    let procs = proc::unix_ssh_procs();
    let rows = proc::process_rows();
    let mut ppid_of = HashMap::new();
    let mut names = HashMap::new();
    for (pid, ppid, name) in &rows {
        ppid_of.insert(*pid, *ppid);
        names.insert(*pid, name.clone());
    }
    let mut listen_pids = Vec::new();
    for p in &procs {
        listen_pids.push(p.pid);
        if let Some(root) = top_editor_ancestor(p.ppid, &ppid_of, &names) {
            for (pid, _, name) in &rows {
                if is_editor_process_name(name) && is_under(*pid, root, &ppid_of) {
                    listen_pids.push(*pid);
                }
            }
        }
    }
    listen_pids.sort_unstable();
    listen_pids.dedup();
    let socks = proc::listen_ports_for_pids(&listen_pids);

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for p in procs {
        if p.pid <= 0 || ours.contains(&p.pid) || seen.contains(&p.pid) {
            continue;
        }
        let cmd = p.cmd.trim();
        if cmd.is_empty() {
            continue;
        }
        if !cmd.to_lowercase().contains("ssh") {
            continue;
        }
        let mut ports = unique_caps(re_local_fwd().captures_iter(cmd));
        let dyn_ports = unique_caps(re_dyn_fwd().captures_iter(cmd));
        for extra in extra_listen_ports(p.pid, p.ppid, &dyn_ports, &ppid_of, &names, &rows, &socks)
        {
            if !ports.iter().any(|x| x == &extra) {
                ports.push(extra);
            }
        }
        let mut all_claimed = !ports.is_empty();
        for port in &ports {
            if !claimed.get(port).copied().unwrap_or(false) {
                all_claimed = false;
                break;
            }
        }
        if all_claimed && dyn_ports.is_empty() {
            continue;
        }
        let parent = if p.parent.is_empty() {
            p.name.to_lowercase()
        } else {
            p.parent.to_lowercase()
        };
        let (kind, label) = classify_ssh(cmd, &parent, &ports, &dyn_ports);
        if kind.is_empty() {
            continue;
        }
        seen.insert(p.pid);
        let mut local_ports = ports.clone();
        local_ports.extend(dyn_ports.clone());
        out.push(DetectedTunnel {
            pid: p.pid,
            kind,
            label,
            detail: summarize_detected(cmd, &ports, &dyn_ports),
            command: cmd.to_string(),
            local_ports,
        });
    }
    out
}

/// Cursor/VS Code Port Forward listens in the editor process, not ssh.exe.
/// Also picks up ssh LocalForward sockets that never appear as `-L` on argv.
fn extra_listen_ports(
    ssh_pid: i32,
    ssh_ppid: i32,
    dyn_ports: &[String],
    ppid_of: &HashMap<i32, i32>,
    names: &HashMap<i32, String>,
    rows: &[(i32, i32, String)],
    socks: &HashMap<i32, Vec<proc::ListenPort>>,
) -> Vec<String> {
    let mut want = vec![ssh_pid];
    if let Some(root) = top_editor_ancestor(ssh_ppid, ppid_of, names) {
        for (pid, _, name) in rows {
            if is_editor_process_name(name) && is_under(*pid, root, ppid_of) {
                want.push(*pid);
            }
        }
    }
    let mut extra = Vec::new();
    for pid in want {
        let Some(ports) = socks.get(&pid) else {
            continue;
        };
        let editor = pid != ssh_pid;
        for lp in ports {
            if dyn_ports.iter().any(|d| d == &lp.port) {
                continue;
            }
            if extra.iter().any(|e| e == &lp.port) {
                continue;
            }
            if editor {
                if !lp.loopback {
                    continue;
                }
                if lp.port.parse::<u16>().ok().is_some_and(|p| p >= 49152) {
                    continue;
                }
            }
            extra.push(lp.port.clone());
        }
    }
    extra
}

pub(crate) fn is_editor_process_name(name: &str) -> bool {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_lowercase();
    let n = base.strip_suffix(".exe").unwrap_or(&base);
    n == "cursor"
        || n.starts_with("cursor helper")
        || n == "code"
        || n.starts_with("code helper")
        || n.starts_with("code -")
}

fn top_editor_ancestor(
    start: i32,
    ppid_of: &HashMap<i32, i32>,
    names: &HashMap<i32, String>,
) -> Option<i32> {
    let mut cur = start;
    let mut found = None;
    for _ in 0..64 {
        if cur <= 0 {
            break;
        }
        if names.get(&cur).is_some_and(|n| is_editor_process_name(n)) {
            found = Some(cur);
        }
        let Some(&next) = ppid_of.get(&cur) else {
            break;
        };
        if next == cur {
            break;
        }
        cur = next;
    }
    found
}

fn is_under(pid: i32, root: i32, ppid_of: &HashMap<i32, i32>) -> bool {
    let mut cur = pid;
    let mut guard = HashSet::new();
    for _ in 0..64 {
        if cur == root {
            return true;
        }
        if cur <= 0 || !guard.insert(cur) {
            return false;
        }
        cur = ppid_of.get(&cur).copied().unwrap_or(0);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_ssh_host_cursor_style() {
        let cmd = r"C:\Windows\System32\OpenSSH\ssh.exe -T -D 6490 -o RemoteCommand=none -F C:\Users\x\.ssh\config asus-test-root";
        assert_eq!(last_ssh_host(cmd), "asus-test-root");
    }

    #[test]
    fn local_forward_no_space() {
        let cmd = r"ssh.exe -N -L6150:127.0.0.1:6150 asus-cctv-mogens";
        let ports = unique_caps(re_local_fwd().captures_iter(cmd));
        assert_eq!(ports, vec!["6150"]);
    }

    #[test]
    fn classify_remote_ssh() {
        let cmd = "ssh -T -D 55822 -o ConnectTimeout=60 myhost";
        let (kind, label) = classify_ssh(cmd, "cursor.exe", &[], &["55822".into()]);
        assert_eq!(kind, "remote-ssh");
        assert!(!label.is_empty());
        assert!(label.contains("55822"));
    }

    #[test]
    fn cmd_forwards_port_local_only() {
        assert!(cmd_forwards_port(
            "ssh -N -L 3306:127.0.0.1:3306 host",
            "3306"
        ));
        assert!(cmd_forwards_port(
            "ssh.exe -N -L3306:127.0.0.1:3306 host",
            "3306"
        ));
        assert!(!cmd_forwards_port(
            "ssh -N -L 3307:127.0.0.1:3306 host",
            "3306"
        ));
    }

    #[test]
    fn cmdline_supervises_legacy() {
        let cmd = r"C:\Apps\Dev\Git\usr\bin\bash.exe /c/TEMP/GitHub/videoscan/scripts/mysql-tunnel.sh _supervisor asus-test-root";
        assert!(cmdline_supervises(
            cmd,
            "asus-test-root-3306",
            "asus-test-root"
        ));
        let cmd2 = r"bash /c/TEMP/GitHub/tunnel/tunnel.sh _supervisor asus-cctv-6150";
        assert!(cmdline_supervises(
            cmd2,
            "asus-cctv-6150",
            "asus-cctv-mogens"
        ));
        assert!(!cmdline_supervises(
            cmd2,
            "asus-test-root-3306",
            "asus-test-root"
        ));
    }

    #[test]
    fn editor_process_names() {
        assert!(is_editor_process_name("Cursor.exe"));
        assert!(is_editor_process_name(
            r"C:\Users\x\AppData\Local\Programs\cursor\Cursor.exe"
        ));
        assert!(is_editor_process_name("Cursor Helper (Plugin).exe"));
        assert!(is_editor_process_name("Code.exe"));
        assert!(is_editor_process_name("Code - Insiders.exe"));
        assert!(is_editor_process_name("Code Helper (GPU).exe"));
        assert!(!is_editor_process_name("ssh.exe"));
        assert!(!is_editor_process_name("chrome.exe"));
    }

    #[test]
    fn editor_tree_picks_cursor_instance() {
        let mut ppid = HashMap::new();
        let mut names = HashMap::new();
        ppid.insert(10, 1);
        names.insert(10, "Cursor.exe".into());
        ppid.insert(11, 10);
        names.insert(11, "Cursor.exe".into());
        ppid.insert(12, 11);
        names.insert(12, "ssh.exe".into());
        ppid.insert(20, 1);
        names.insert(20, "Cursor.exe".into());
        ppid.insert(21, 20);
        names.insert(21, "ssh.exe".into());
        assert_eq!(top_editor_ancestor(11, &ppid, &names), Some(10));
        assert!(is_under(11, 10, &ppid));
        assert!(!is_under(20, 10, &ppid));
        assert_eq!(top_editor_ancestor(20, &ppid, &names), Some(20));
    }

    #[test]
    fn classify_keeps_remote_ssh_when_local_forward_present() {
        let (kind, label) = classify_ssh(
            "ssh -T -D 6490 host",
            "cursor.exe",
            &["5173".into()],
            &["6490".into()],
        );
        assert_eq!(kind, "remote-ssh");
        assert!(label.contains("6490"));
        let detail = summarize_detected("ssh -T -D 6490 host", &["5173".into()], &["6490".into()]);
        assert!(detail.contains("5173"), "{detail}");
        assert!(detail.contains("6490"), "{detail}");
    }

    #[test]
    fn extra_listen_includes_cursor_5173_not_other_instance() {
        let mut ppid = HashMap::new();
        let mut names = HashMap::new();
        let rows: Vec<(i32, i32, String)> = vec![
            (10, 1, "Cursor.exe".into()),
            (11, 10, "Cursor.exe".into()),
            (12, 11, "ssh.exe".into()),
            (20, 1, "Cursor.exe".into()),
        ];
        for (pid, parent, name) in &rows {
            ppid.insert(*pid, *parent);
            names.insert(*pid, name.clone());
        }
        let mut socks = HashMap::new();
        socks.insert(
            12,
            vec![proc::ListenPort {
                port: "6490".into(),
                loopback: true,
            }],
        );
        socks.insert(
            11,
            vec![proc::ListenPort {
                port: "5173".into(),
                loopback: true,
            }],
        );
        socks.insert(
            20,
            vec![proc::ListenPort {
                port: "3000".into(),
                loopback: true,
            }],
        );
        let extra = extra_listen_ports(12, 11, &["6490".into()], &ppid, &names, &rows, &socks);
        assert_eq!(extra, vec!["5173".to_string()]);
    }
}
