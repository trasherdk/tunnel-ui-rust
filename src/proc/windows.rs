use std::process::Command;
use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};

use super::{parse_netstat_tlnp, OsProc};

/// GetExitCodeProcess returns this while the process is still running.
const STILL_ACTIVE: u32 = 259;

pub fn pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid as u32) else {
            return false;
        };
        let mut code = 0u32;
        let alive = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE;
        let _ = CloseHandle(h);
        alive
    }
}

pub fn process_image(pid: i32) -> String {
    if pid <= 0 {
        return String::new();
    }
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid as u32) else {
            return String::new();
        };
        let mut buf = [0u16; 32768];
        let mut size = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size)
                .is_ok();
        let _ = CloseHandle(h);
        if !ok || size == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..size as usize])
    }
}

pub fn process_cmdline(_pid: i32) -> String {
    String::new()
}

#[allow(dead_code)]
pub fn ppid_of(pid: i32) -> i32 {
    if pid <= 0 {
        return 0;
    }
    for p in snapshot_processes() {
        if p.0 == pid {
            return p.1;
        }
    }
    0
}

pub fn terminate_pid(pid: i32) {
    if pid <= 0 || !pid_alive(pid) {
        return;
    }
    unsafe {
        if let Ok(h) = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid as u32,
        ) {
            let _ = TerminateProcess(h, 1);
            let _ = CloseHandle(h);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output();
}

pub fn list_numeric_pids() -> Vec<i32> {
    snapshot_processes().into_iter().map(|p| p.0).collect()
}

pub fn unix_ssh_procs() -> Vec<OsProc> {
    let rows = snapshot_processes();
    let mut names = std::collections::HashMap::new();
    for (pid, _ppid, name) in &rows {
        names.insert(*pid, name.clone());
    }
    let mut out = Vec::new();
    for (pid, ppid, name) in rows {
        let lower = name.to_lowercase();
        if lower != "ssh.exe" && !lower.contains("ssh") {
            continue;
        }
        out.push(OsProc {
            pid,
            ppid,
            name: name.clone(),
            parent: names.get(&ppid).cloned().unwrap_or_default(),
            cmd: process_image(pid),
        });
    }
    out
}

pub fn pids_listening_on(port: &str) -> Vec<i32> {
    let port = port.trim();
    if port.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
        return Vec::new();
    }
    if let Ok(out) = Command::new("netstat").args(["-ano"]).output() {
        if out.status.success() {
            return parse_netstat_tlnp(&String::from_utf8_lossy(&out.stdout), port);
        }
    }
    Vec::new()
}

fn snapshot_processes() -> Vec<(i32, i32, String)> {
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Vec::new();
        };
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut out = Vec::new();
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let name = wstr_from_u16(&pe.szExeFile);
                out.push((pe.th32ProcessID as i32, pe.th32ParentProcessID as i32, name));
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        out
    }
}

fn wstr_from_u16(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
