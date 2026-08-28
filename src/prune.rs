use std::fs;

use crate::config::sanitize_name;
use crate::paths::{read_pid_file, Paths};
use crate::proc::{is_managed_process, pid_alive, terminate_pid};

/// Remove leftover `.state` entries that do not belong to a saved setup.
/// Not called from `--supervisor`.
pub fn prune_orphan_state(paths: &Paths) {
    let Ok(entries) = fs::read_dir(&paths.state_dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name == ".gitignore" {
            continue;
        }
        let path = e.path();
        if !path.is_dir() {
            let _ = fs::remove_file(&path);
            continue;
        }
        if sanitize_name(&name).is_err() || !paths.saved_config_exists(&name) {
            reap_state_dir(paths, &name);
            continue;
        }
        drop_stale_pid_files(paths, &name);
    }
}

fn drop_stale_pid_files(paths: &Paths, name: &str) {
    let sp = paths.setup(name);
    if !pid_file_still_valid(&sp.pid_file) {
        let _ = fs::remove_file(&sp.pid_file);
    }
    if !pid_file_still_valid(&sp.child_file) {
        let _ = fs::remove_file(&sp.child_file);
    }
    if !pid_file_still_valid(&sp.pid_file) && !pid_file_still_valid(&sp.child_file) {
        let _ = fs::remove_file(&sp.stop_file);
    }
}

fn pid_file_still_valid(path: &std::path::Path) -> bool {
    let pid = read_pid_file(path);
    pid > 0 && pid_alive(pid) && is_managed_process(pid)
}

fn reap_state_dir(paths: &Paths, name: &str) {
    let sp = paths.setup(name);
    for path in [&sp.pid_file, &sp.child_file] {
        let pid = read_pid_file(path);
        if pid_alive(pid) && is_managed_process(pid) {
            terminate_pid(pid);
        }
    }
    let _ = fs::remove_dir_all(&sp.dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn prune_orphan_state_keeps_log_drops_dead_pid() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::from_root(home.path());
        paths.ensure_dirs().unwrap();

        let keep = paths.setup("asus-keep-3306");
        let orphan = paths.setup("definitely-invalid-ssh-host-39999");
        fs::create_dir_all(&keep.dir).unwrap();
        fs::create_dir_all(&orphan.dir).unwrap();
        fs::write(&keep.log_file, "ok\n").unwrap();
        fs::write(&orphan.log_file, "junk\n").unwrap();
        fs::write(&keep.pid_file, "99999999\n").unwrap();
        fs::write(&keep.child_file, format!("{}\n", std::process::id())).unwrap();
        fs::write(paths.state_dir.join("stray.txt"), "nope\n").unwrap();
        fs::write(paths.state_dir.join(".gitignore"), "*\n").unwrap();
        fs::write(
            paths.config_dir.join("asus-keep-3306.conf"),
            "NAME=asus-keep-3306\nSSH_TARGET=h\nLOCAL_PORT=3306\n",
        )
        .unwrap();

        prune_orphan_state(&paths);

        assert!(!orphan.dir.exists(), "orphan state dir should be removed");
        assert!(!paths.state_dir.join("stray.txt").exists());
        assert!(paths.state_dir.join(".gitignore").exists());
        assert!(
            keep.log_file.exists(),
            "log for a saved setup should be kept"
        );
        assert!(
            !keep.pid_file.exists(),
            "dead supervisor PID file should be removed"
        );
        assert!(
            keep.child_file.exists(),
            "live managed PID file should be kept"
        );
    }
}
