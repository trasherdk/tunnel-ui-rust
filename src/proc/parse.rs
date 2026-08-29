/// Parse `/proc/net/tcp` / `tcp6` listen rows into (local_port, inode).
#[cfg_attr(not(unix), allow(dead_code))]
pub fn parse_proc_net_listen(text: &str) -> Vec<(u16, u64)> {
    parse_proc_net_listen_ext(text)
        .into_iter()
        .map(|(port, ino, _)| (port, ino))
        .collect()
}

/// Listen rows: (local_port, inode, loopback).
#[cfg_attr(not(unix), allow(dead_code))]
pub fn parse_proc_net_listen_ext(text: &str) -> Vec<(u16, u64, bool)> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        if fields[3] != "0A" {
            continue;
        }
        let Some((addr_hex, port_hex)) = fields[1].rsplit_once(':') else {
            continue;
        };
        let Ok(port) = u16::from_str_radix(port_hex, 16) else {
            continue;
        };
        let Ok(inode) = fields[9].parse::<u64>() else {
            continue;
        };
        if inode > 0 {
            out.push((port, inode, local_hex_is_loopback(addr_hex)));
        }
    }
    out
}

fn local_hex_is_loopback(addr_hex: &str) -> bool {
    let h = addr_hex.to_ascii_uppercase();
    h == "0100007F"
        || h == "00000000000000000000000001000000"
        || h == "0000000000000000FFFF00000100007F"
}

#[cfg_attr(not(unix), allow(dead_code))]
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

/// Windows `netstat -ano` LISTENING rows → (port, pid).
pub fn parse_netstat_listen_table(text: &str) -> Vec<(String, i32)> {
    let mut out = Vec::new();
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
        let Some(p) = local
            .rsplit_once(':')
            .or_else(|| local.rsplit_once('.'))
            .map(|(_, p)| p.trim())
        else {
            continue;
        };
        if p.is_empty() || p.parse::<u16>().ok().filter(|n| *n > 0).is_none() {
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
        if pid > 0 {
            out.push((p.to_string(), pid));
        }
    }
    out
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
        let ext = parse_proc_net_listen_ext(SAMPLE_TCP);
        assert!(ext
            .iter()
            .any(|(p, i, lb)| *p == 3306 && *i == 12345 && *lb));
        assert!(ext.iter().any(|(p, _, lb)| *p == 80 && !*lb));
    }

    #[test]
    fn ss_and_netstat_parsers() {
        let ss = r#"LISTEN 0 128 127.0.0.1:3306 0.0.0.0:* users:(("ssh",pid=4242,fd=3))"#;
        assert_eq!(parse_ss_lntp(ss, "3306"), vec![4242]);
        let ns = "tcp 0 0 127.0.0.1:3306 0.0.0.0:* LISTEN 4242/ssh";
        assert_eq!(parse_netstat_tlnp(ns, "3306"), vec![4242]);
        let win = "TCP    127.0.0.1:3306    0.0.0.0:0    LISTENING    4242";
        assert_eq!(parse_netstat_tlnp(win, "3306"), vec![4242]);
        let table = parse_netstat_listen_table(
            "TCP    127.0.0.1:5173    0.0.0.0:0    LISTENING    88\n\
             TCP    [::1]:5173         [::]:0        LISTENING    88\n\
             TCP    0.0.0.0:445        0.0.0.0:0    LISTENING    4\n",
        );
        assert!(table.contains(&("5173".into(), 88)), "got {table:?}");
        assert!(table.contains(&("445".into(), 4)));
    }
}
