use std::env;
use std::path::Path;

use anyhow::{bail, Result};

use crate::config::Config;

pub fn find_ssh() -> Result<String> {
    if let Ok(v) = env::var("SSH") {
        if !v.is_empty() {
            let p = Path::new(&v);
            if p.is_file() {
                return Ok(v);
            }
            bail!("SSH={v} not found");
        }
    }
    if let Ok(p) = which_ssh() {
        return Ok(p);
    }
    bail!("ssh not found (install OpenSSH and put ssh on PATH, or set SSH)");
}

fn which_ssh() -> Result<String, ()> {
    let path = env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let cand = Path::new(dir).join("ssh");
        if cand.is_file() {
            return Ok(cand.to_string_lossy().into_owned());
        }
    }
    Err(())
}

impl Config {
    pub fn ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-N".into(),
            "-L".into(),
            format!(
                "{}:{}:{}",
                self.local_port, self.remote_host, self.remote_port
            ),
            "-o".into(),
            "ExitOnForwardFailure=yes".into(),
            "-o".into(),
            "ServerAliveInterval=30".into(),
            "-o".into(),
            "ServerAliveCountMax=3".into(),
            "-o".into(),
            "TCPKeepAlive=yes".into(),
            "-o".into(),
            "ConnectTimeout=15".into(),
        ];
        if !self.ssh_key.is_empty() {
            args.push("-i".into());
            args.push(self.ssh_key.clone());
        }
        args.push("--".into());
        args.push(self.via());
        args
    }
}

/// OpenSSH errors that will not fix themselves by reconnecting.
pub fn fatal_ssh_message(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && is_fatal_ssh_line(line))
        .map(str::to_string)
}

fn is_fatal_ssh_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    if l.contains("host key verification failed")
        || l.contains("remote host identification has changed")
        || l.contains("permission denied")
        || l.contains("too many authentication failures")
        || l.contains("could not resolve hostname")
        || l.contains("name or service not known")
        || l.contains("no matching host key type")
        || l.contains("bad configuration option")
        || l.contains("unprotected private key file")
        || l.contains("invalid format")
        || l.contains("not a valid key file")
        || l.contains("error loading key")
        || l.contains("could not request local forwarding")
        || l.contains("cannot listen to port")
        || l.contains("address already in use")
    {
        return true;
    }
    if l.contains("identity file")
        && (l.contains("not accessible") || l.contains("no such file or directory"))
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_args_alias() {
        let c = Config {
            auth_mode: "alias".into(),
            ssh_target: "asus-test-root".into(),
            local_port: "3306".into(),
            remote_host: "127.0.0.1".into(),
            remote_port: "3306".into(),
            ..Config::default()
        };
        let got = c.ssh_args().join(" ");
        for p in ["-N", "-L 3306:127.0.0.1:3306", "-- asus-test-root"] {
            assert!(got.contains(p), "missing {p:?} in {got}");
        }
    }

    #[test]
    fn ssh_args_key() {
        let c = Config {
            auth_mode: "explicit".into(),
            remote_user: "root".into(),
            ssh_host: "example.com".into(),
            local_port: "6150".into(),
            remote_host: "127.0.0.1".into(),
            remote_port: "6150".into(),
            ssh_key: r"C:\Users\x\.ssh\id_ed25519".into(),
            ..Config::default()
        };
        let args = c.ssh_args();
        let joined = args.join(" ");
        assert!(joined.contains("-i"));
        assert!(joined.contains(r"C:\Users\x\.ssh\id_ed25519"));
        assert!(joined.contains("root@example.com"));
    }

    #[test]
    fn fatal_ssh_catches_host_key_and_auth() {
        assert_eq!(
            fatal_ssh_message("Host key verification failed.\n"),
            Some("Host key verification failed.".into())
        );
        assert!(fatal_ssh_message(
            "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
             @    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n"
        )
        .is_some());
        assert!(fatal_ssh_message("Permission denied (publickey).\n").is_some());
        assert!(fatal_ssh_message(
            "ssh: Could not resolve hostname ghost: Name or service not known\n"
        )
        .is_some());
        assert!(fatal_ssh_message(
            "bind [127.0.0.1]:8081: Address already in use\n\
             channel_setup_fwd_listener_tcpip: cannot listen to port: 8081\n\
             Could not request local forwarding.\n"
        )
        .is_some());
    }

    #[test]
    fn fatal_ssh_allows_transient_drops() {
        assert_eq!(
            fatal_ssh_message("ssh: connect to host x port 22: Connection timed out\n"),
            None
        );
        assert_eq!(
            fatal_ssh_message("ssh: connect to host x port 22: Connection refused\n"),
            None
        );
        assert_eq!(fatal_ssh_message("Connection reset by peer\n"), None);
        assert_eq!(fatal_ssh_message("Broken pipe\n"), None);
    }
}
