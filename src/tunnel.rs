use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::config::{load_named_config, Config};
use crate::detect::cmd_forwards_port;
use crate::paths::{
    append_log, clear_err_file, file_exists, read_err_file, read_pid_file, write_err_file,
    write_pid_file, Paths,
};
use crate::proc::{
    is_managed_process, is_ssh_process, is_supervisor_name, pid_alive, pids_listening_on,
    port_listeners_named, process_image, ssh_holding_port, ssh_holding_port_live, terminate_pid,
    unix_ssh_procs,
};
use crate::ssh::{fatal_ssh_message, find_ssh};

pub fn setup_on(paths: &Paths, c: &Config) -> bool {
    let sp = paths.setup(&c.name);
    if pid_alive(read_pid_file(&sp.pid_file)) {
        return true;
    }
    let child = read_pid_file(&sp.child_file);
    if pid_alive(child) && is_ssh_process(child) {
        return true;
    }
    ssh_holding_port(&c.local_port)
}

pub fn setup_listening(c: &Config) -> bool {
    ssh_holding_port(&c.local_port)
}

pub fn setup_error_hint(paths: &Paths, name: &str) -> String {
    let sp = paths.setup(name);
    let stored = read_err_file(&sp.err_file);
    if !stored.is_empty() {
        return stored;
    }
    crate::paths::last_failure_line(&sp.log_file)
}

pub fn setup_has_error(paths: &Paths, name: &str) -> bool {
    !read_err_file(&paths.setup(name).err_file).is_empty()
}

fn persist_fatal(sp: &crate::paths::SetupPaths, msg: &str) {
    let _ = write_err_file(&sp.err_file, msg);
    append_log(
        &sp.log_file,
        &format!("Fatal SSH error (not reconnecting): {msg}"),
    );
}

fn ssh_output_after_last_start(log: &str) -> &str {
    const MARK: &str = "] Starting SSH tunnel:";
    let Some(pos) = log.rfind(MARK) else {
        return "";
    };
    match log[pos..].find('\n') {
        Some(i) => &log[pos + i + 1..],
        None => "",
    }
}

fn fatal_from_ssh_run(captured: &str, log_file: &Path) -> Option<String> {
    if let Some(msg) = fatal_ssh_message(captured) {
        return Some(msg);
    }
    let log = fs::read_to_string(log_file).unwrap_or_default();
    fatal_ssh_message(ssh_output_after_last_start(&log))
}

fn tee_to_log(mut r: impl Read, log_file: PathBuf, sink: Arc<Mutex<String>>) {
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Ok(mut s) = sink.lock() {
                    s.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
                if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&log_file) {
                    use std::io::Write;
                    let _ = f.write_all(&buf[..n]);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn start_failure_text(paths: &Paths, name: &str, local_port: &str) -> String {
    let sp = paths.setup(name);
    let hint = setup_error_hint(paths, name);
    let tail = crate::paths::tail_file(&sp.log_file, 8);
    if hint.is_empty() {
        format!(
            "failed to start tunnel {name:?}; ssh is not listening on port {local_port}.\n{tail}"
        )
    } else {
        format!("failed to start tunnel {name:?}: {hint}\n{tail}")
    }
}

pub fn supervisor_running(paths: &Paths, name: &str) -> (i32, bool) {
    let sp = paths.setup(name);
    let pid = read_pid_file(&sp.pid_file);
    if pid_alive(pid) {
        (pid, true)
    } else {
        (pid, false)
    }
}

fn ssh_child_alive(paths: &Paths, name: &str) -> bool {
    let child = read_pid_file(&paths.setup(name).child_file);
    pid_alive(child) && is_ssh_process(child)
}

fn setsid_command(cmd: &mut Command) {
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

pub fn start_tunnel(paths: &Paths, name: &str) -> Result<String> {
    let c = load_named_config(paths, name)?;
    find_ssh()?;
    let sp = paths.setup(name);
    crate::paths::mkdir_755(&sp.dir)?;
    if let (pid, true) = supervisor_running(paths, name) {
        if ssh_holding_port(&c.local_port) || ssh_child_alive(paths, name) {
            return Ok(format!("Tunnel {name:?} already running (PID {pid})"));
        }
        let tail = crate::paths::tail_file(&sp.log_file, 8);
        bail!(
            "Tunnel {name:?} supervisor is running (PID {pid}) but ssh is not listening on port {}.\n{}",
            c.local_port,
            tail
        );
    }
    let _ = fs::remove_file(&sp.stop_file);
    clear_err_file(&sp.err_file);

    let exe = std::env::current_exe().context("current_exe")?;
    let log_f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&sp.log_file)
        .with_context(|| format!("open {}", sp.log_file.display()))?;
    let mut cmd = Command::new(&exe);
    cmd.arg("--supervisor")
        .arg(name)
        .env("TUNNEL_CONFIG_DIR", &paths.config_dir)
        .env("TUNNEL_STATE_DIR", &paths.state_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_f.try_clone()?))
        .stderr(Stdio::from(log_f));
    setsid_command(&mut cmd);
    let child = cmd.spawn().context("start supervisor")?;
    let pid = child.id() as i32;
    write_pid_file(&sp.pid_file, pid)?;
    thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    thread::sleep(Duration::from_millis(400));
    if let (pid, true) = supervisor_running(paths, name) {
        // Supervisor can stay up while ssh dies immediately (reconnect loop).
        // Wait briefly so a fast failure (host key, auth, bind) is visible.
        for _ in 0..10 {
            if ssh_holding_port(&c.local_port) {
                clear_err_file(&sp.err_file);
                return Ok(format!(
                    "Starting tunnel {name:?}: {}\nTunnel {name:?} started (PID {pid}). Logs: {}",
                    c.forward_summary(),
                    sp.log_file.display()
                ));
            }
            if !supervisor_running(paths, name).1 {
                break;
            }
            thread::sleep(Duration::from_millis(120));
        }
        if ssh_holding_port(&c.local_port) {
            clear_err_file(&sp.err_file);
            return Ok(format!(
                "Starting tunnel {name:?}: {}\nTunnel {name:?} started (PID {pid}). Logs: {}",
                c.forward_summary(),
                sp.log_file.display()
            ));
        }
        if supervisor_running(paths, name).1 && ssh_child_alive(paths, name) {
            return Ok(format!(
                "Starting tunnel {name:?}: {}\nTunnel {name:?} started (PID {pid}). Logs: {}",
                c.forward_summary(),
                sp.log_file.display()
            ));
        }
        if !supervisor_running(paths, name).1 {
            let _ = fs::remove_file(&sp.pid_file);
        }
        bail!("{}", start_failure_text(paths, name, &c.local_port));
    }
    let _ = fs::remove_file(&sp.pid_file);
    bail!("{}", start_failure_text(paths, name, &c.local_port));
}

pub fn run_supervisor(paths: &Paths, name: &str) -> Result<()> {
    let c = load_named_config(paths, name)?;
    let ssh_path = find_ssh()?;
    let sp = paths.setup(name);
    let delay = c.reconnect_delay_secs();
    let reconnect = c.reconnect_enabled();
    append_log(
        &sp.log_file,
        &format!(
            "Supervisor started (PID {}, name={name}, target={}, reconnect={reconnect})",
            std::process::id(),
            c.via()
        ),
    );

    loop {
        if file_exists(&sp.stop_file) {
            append_log(&sp.log_file, "Stop requested; exiting supervisor");
            return Ok(());
        }
        append_log(
            &sp.log_file,
            &format!("Starting SSH tunnel: {}", c.forward_summary()),
        );
        let mut cmd = Command::new(&ssh_path);
        cmd.args(c.ssh_args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        setsid_command(&mut cmd);
        match cmd.spawn() {
            Err(err) => {
                let msg = format!("ssh start failed: {err}");
                persist_fatal(&sp, &msg);
                return Err(err.into());
            }
            Ok(mut child) => {
                let captured = Arc::new(Mutex::new(String::new()));
                let mut tees = Vec::new();
                if let Some(out) = child.stdout.take() {
                    let log = sp.log_file.clone();
                    let sink = captured.clone();
                    tees.push(thread::spawn(move || tee_to_log(out, log, sink)));
                }
                if let Some(err) = child.stderr.take() {
                    let log = sp.log_file.clone();
                    let sink = captured.clone();
                    tees.push(thread::spawn(move || tee_to_log(err, log, sink)));
                }
                write_pid_file(&sp.child_file, child.id() as i32)?;
                let status = child.wait()?;
                for t in tees {
                    let _ = t.join();
                }
                let captured = captured.lock().map(|s| s.clone()).unwrap_or_default();
                let _ = fs::remove_file(&sp.child_file);
                let exit = status.code().unwrap_or(-1);
                if file_exists(&sp.stop_file) {
                    append_log(
                        &sp.log_file,
                        &format!("Stop requested after tunnel exit ({exit}); exiting supervisor"),
                    );
                    return Ok(());
                }
                if let Some(msg) = fatal_from_ssh_run(&captured, &sp.log_file) {
                    persist_fatal(&sp, &msg);
                    bail!("{msg}");
                }
                if !reconnect {
                    append_log(
                        &sp.log_file,
                        &format!("Tunnel exited ({exit}); reconnect disabled"),
                    );
                    if !status.success() {
                        let msg = setup_error_hint(paths, name);
                        let msg = if msg.is_empty() {
                            format!("ssh exited {exit}")
                        } else {
                            msg
                        };
                        persist_fatal(&sp, &msg);
                        bail!("{msg}");
                    }
                    return Ok(());
                }
                append_log(
                    &sp.log_file,
                    &format!("Tunnel exited ({exit}); reconnecting in {delay}s"),
                );
                thread::sleep(Duration::from_secs(delay));
            }
        }
    }
}

pub fn stop_tunnel(paths: &Paths, name: &str) -> Result<String> {
    let c = load_named_config(paths, name)?;
    let sp = paths.setup(name);
    if let Some(parent) = sp.stop_file.parent() {
        crate::paths::mkdir_755(parent)?;
    }
    crate::paths::write_mode(&sp.stop_file, "1\n", 0o644)?;
    append_log(
        &sp.log_file,
        &format!(
            "Stop requested (supervisor + ssh forward on port {})",
            c.local_port
        ),
    );

    let mut b = String::new();
    let pid = read_pid_file(&sp.pid_file);
    if pid_alive(pid) {
        if is_managed_process(pid) {
            b.push_str(&format!("Stopping tunnel supervisor (PID {pid})\n"));
            terminate_pid(pid);
            for _ in 0..8 {
                if !pid_alive(pid) {
                    break;
                }
                thread::sleep(Duration::from_millis(25));
            }
        } else {
            b.push_str(&format!(
                "Ignoring stale supervisor PID {pid} ({})\n",
                crate::proc::basename(&process_image(pid))
            ));
        }
    } else if pid > 0 {
        b.push_str("Tunnel supervisor not running; cleaning up PID file\n");
    } else {
        b.push_str("No supervisor PID file; checking for leftover ssh forward\n");
    }

    let child = read_pid_file(&sp.child_file);
    if pid_alive(child) && is_managed_process(child) {
        b.push_str(&format!("Stopping tunnel ssh (PID {child})\n"));
        terminate_pid(child);
    }

    let _ = fs::remove_file(&sp.pid_file);
    let _ = fs::remove_file(&sp.child_file);

    let killed = kill_ssh_forwards_on_port(&c);
    if !killed.is_empty() {
        b.push_str(&format!(
            "Stopped ssh/supervisor PIDs on port {}: {killed:?}\n",
            c.local_port
        ));
    }

    let _ = fs::remove_file(&sp.stop_file);
    append_log(&sp.log_file, "Tunnel stop complete");

    thread::sleep(Duration::from_millis(50));
    if ssh_holding_port_live(&c.local_port) {
        let killed = kill_ssh_forwards_on_port(&c);
        if !killed.is_empty() {
            b.push_str(&format!("Stopped remaining ssh PIDs: {killed:?}\n"));
            thread::sleep(Duration::from_millis(50));
        }
    }
    if ssh_holding_port_live(&c.local_port) {
        b.push_str(&format!(
            "Warning: ssh is still holding port {}\nTunnel stop incomplete",
            c.local_port
        ));
        bail!("ssh still holding port {}", c.local_port);
    }
    b.push_str("Tunnel stopped");
    Ok(b.trim().to_string())
}

fn unix_supervisor_pids(name: &str, target: &str) -> Vec<i32> {
    let mut out = Vec::new();
    for pid in crate::proc::list_numeric_pids() {
        let cmd = crate::proc::process_cmdline(pid);
        if crate::detect::cmdline_supervises(&cmd, name, target) {
            out.push(pid);
        }
    }
    out
}

fn kill_ssh_forwards_on_port(c: &Config) -> Vec<i32> {
    let port = &c.local_port;
    let listen: HashSet<i32> = pids_listening_on(port).into_iter().collect();
    let procs = unix_ssh_procs();

    let mut ssh_set = HashSet::new();
    let mut parent_set = HashSet::new();
    for p in procs {
        if crate::detect::cmdline_supervises(&p.cmd, &c.name, &c.via()) {
            parent_set.insert(p.pid);
        }
        let matched = cmd_forwards_port(&p.cmd, port) || listen.contains(&p.pid);
        if !matched {
            continue;
        }
        ssh_set.insert(p.pid);
        if is_supervisor_name(&p.parent) || is_supervisor_name(&process_image(p.ppid)) {
            parent_set.insert(p.ppid);
        }
    }
    for pid in &listen {
        if is_ssh_process(*pid) || is_supervisor_name(&process_image(*pid)) {
            ssh_set.insert(*pid);
        }
    }
    for pid in unix_supervisor_pids(&c.name, &c.via()) {
        parent_set.insert(pid);
    }

    let mut seen = HashSet::new();
    let mut killed = Vec::new();
    let mut kill = |pid: i32| {
        if pid <= 0 || !seen.insert(pid) {
            return;
        }
        terminate_pid(pid);
        killed.push(pid);
    };
    for pid in parent_set {
        kill(pid);
    }
    for pid in ssh_set {
        kill(pid);
    }
    killed
}

pub fn status_tunnel(paths: &Paths, name: &str) -> Result<String> {
    let c = load_named_config(paths, name)?;
    let sp = paths.setup(name);
    let mut b = String::new();
    b.push_str(&format!("Tunnel: {name}\n"));
    if !c.comment_line().is_empty() {
        b.push_str(&format!("  Comment: {}\n", c.comment_line()));
    }
    b.push_str(&format!("  Forward: {}\n", c.forward_summary()));
    if let (pid, true) = supervisor_running(paths, name) {
        b.push_str(&format!("  Supervisor: running (PID {pid})\n"));
        b.push_str(&format!("  Logs: {}\n", sp.log_file.display()));
        let listening = !port_listeners_named(&c.local_port).0.is_empty();
        if !listening {
            let tail = crate::paths::tail_file(&sp.log_file, 8);
            if !tail.is_empty() {
                b.push_str("  Last log lines:\n");
                for line in tail.lines() {
                    b.push_str(&format!("    {line}\n"));
                }
            }
        }
    } else {
        b.push_str("  Supervisor: not running\n");
        let err = read_err_file(&sp.err_file);
        if !err.is_empty() {
            b.push_str(&format!("  Last error: {err}\n"));
        }
        let tail = crate::paths::tail_file(&sp.log_file, 5);
        if !tail.is_empty() {
            b.push_str("  Last log lines:\n");
            for line in tail.lines() {
                b.push_str(&format!("    {line}\n"));
            }
        }
    }
    let (ssh_pids, other) = port_listeners_named(&c.local_port);
    if !ssh_pids.is_empty() {
        b.push_str(&format!(
            "  Port {}: ssh listening ({})\n",
            c.local_port,
            ssh_pids.join(", ")
        ));
    } else if !other.is_empty() {
        b.push_str(&format!(
            "  Port {}: in use by {} (not ssh)\n",
            c.local_port,
            other.join(", ")
        ));
    } else {
        b.push_str(&format!("  Port {}: not listening\n", c.local_port));
    }
    Ok(b.trim().to_string())
}

pub fn status_all(paths: &Paths) -> Result<String> {
    let cfgs = crate::config::load_configs(paths)?;
    if cfgs.is_empty() {
        return Ok(format!(
            "No saved tunnels in {}",
            paths.config_dir.display()
        ));
    }
    let mut parts = Vec::new();
    for c in cfgs {
        match status_tunnel(paths, &c.name) {
            Ok(s) => parts.push(s),
            Err(e) => parts.push(e.to_string()),
        }
    }
    Ok(parts.join("\n\n"))
}

pub fn delete_tunnel(paths: &Paths, name: &str) -> Result<String> {
    crate::config::sanitize_name(name)?;
    let path = paths.config_dir.join(format!("{name}.conf"));
    if !path.is_file() {
        bail!("no saved tunnel named {name:?}");
    }
    let mut b = String::new();
    if load_named_config(paths, name).is_ok() {
        if let Ok(out) = stop_tunnel(paths, name) {
            if !out.is_empty() {
                b.push_str(&out);
                b.push('\n');
            }
        }
    }
    fs::remove_file(&path)?;
    let sp = paths.setup(name);
    let _ = fs::remove_dir_all(&sp.dir);
    b.push_str(&format!("Deleted tunnel config {name:?}"));
    Ok(b.trim().to_string())
}

pub fn run_cli(paths: &Paths, args: &[String]) -> Result<String> {
    if args.is_empty() {
        bail!("missing command");
    }
    match args[0].as_str() {
        "start" => {
            if args.len() < 2 {
                bail!("start requires a name");
            }
            start_tunnel(paths, &args[1])
        }
        "stop" => {
            if args.len() < 2 {
                bail!("stop requires a name");
            }
            stop_tunnel(paths, &args[1])
        }
        "status" => {
            if args.len() >= 2 {
                status_tunnel(paths, &args[1])
            } else {
                status_all(paths)
            }
        }
        "delete" => {
            if args.len() < 2 {
                bail!("delete requires a name");
            }
            delete_tunnel(paths, &args[1])
        }
        other => bail!("unknown command {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::save_config;
    use crate::paths::Paths;

    #[test]
    fn delete_removes_conf_and_state() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::from_root(home.path());
        paths.ensure_dirs().unwrap();
        let name = "gone-setup-1";
        let sp = paths.setup(name);
        fs::create_dir_all(&sp.dir).unwrap();
        fs::write(&sp.log_file, "x\n").unwrap();
        let c = Config {
            name: name.into(),
            auth_mode: "alias".into(),
            ssh_target: "h".into(),
            local_port: "1".into(),
            ..Config::default()
        };
        save_config(&paths, c).unwrap();
        delete_tunnel(&paths, name).unwrap();
        assert!(!paths.config_dir.join(format!("{name}.conf")).exists());
        assert!(!sp.dir.exists());
    }

    #[test]
    fn status_missing_name() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::from_root(home.path());
        paths.ensure_dirs().unwrap();
        let err = status_tunnel(&paths, "nope").unwrap_err().to_string();
        assert!(err.contains("no saved tunnel"));
    }

    #[test]
    fn ssh_chunk_after_start_marker() {
        let log = "[t] Supervisor started\n[t] Starting SSH tunnel: x\nHost key verification failed.\n";
        assert!(ssh_output_after_last_start(log).contains("Host key verification failed"));
    }

    #[test]
    fn supervisor_stops_on_host_key_failure() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::mpsc;

        static SSH_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _env = SSH_ENV.lock().unwrap_or_else(|e| e.into_inner());

        struct RestoreSsh(Option<String>);
        impl Drop for RestoreSsh {
            fn drop(&mut self) {
                match &self.0 {
                    Some(v) => std::env::set_var("SSH", v),
                    None => std::env::remove_var("SSH"),
                }
            }
        }

        let home = tempfile::tempdir().unwrap();
        let paths = Paths::from_root(home.path());
        paths.ensure_dirs().unwrap();
        let name = "hkfail-1";
        let ssh = home.path().join("fake-ssh");
        fs::write(
            &ssh,
            "#!/bin/sh\necho 'Host key verification failed.' >&2\nexit 255\n",
        )
        .unwrap();
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).unwrap();
        let _restore = RestoreSsh(std::env::var("SSH").ok());
        std::env::set_var("SSH", &ssh);

        save_config(
            &paths,
            Config {
                name: name.into(),
                auth_mode: "alias".into(),
                ssh_target: "h".into(),
                local_port: "19991".into(),
                remote_host: "127.0.0.1".into(),
                remote_port: "19991".into(),
                reconnect: "1".into(),
                reconnect_delay: "30".into(),
                ..Config::default()
            },
        )
        .unwrap();

        let paths2 = paths.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(run_supervisor(&paths2, name));
        });
        let res = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("supervisor should stop instead of reconnecting");
        let err = res.unwrap_err().to_string();
        assert!(
            err.to_ascii_lowercase().contains("host key"),
            "unexpected error: {err}"
        );
        let stored = crate::paths::read_err_file(&paths.setup(name).err_file);
        assert!(
            stored.to_ascii_lowercase().contains("host key"),
            "missing err file: {stored}"
        );
        let log = fs::read_to_string(&paths.setup(name).log_file).unwrap();
        assert!(log.contains("not reconnecting"), "{log}");
        assert!(!log.contains("reconnecting in"), "{log}");
    }
}
