use std::collections::HashMap;
use std::ffi::c_void;
use std::process::Command;
use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
};

use super::{parse_netstat_listen_table, parse_netstat_tlnp, ListenPort, OsProc};

/// GetExitCodeProcess returns this while the process is still running.
const STILL_ACTIVE: u32 = 259;
const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

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

pub fn process_cmdline(pid: i32) -> String {
    if pid <= 0 {
        return String::new();
    }
    unsafe {
        let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ;
        let h = match OpenProcess(access, false, pid as u32) {
            Ok(h) => h,
            Err(_) => match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid as u32) {
                Ok(h) => h,
                Err(_) => return String::new(),
            },
        };
        let mut ret_len = 0u32;
        let _ = NtQueryInformationProcess(
            h,
            ProcessCommandLineInformation,
            std::ptr::null_mut(),
            0,
            &mut ret_len,
        );
        if ret_len == 0 {
            ret_len = 4096;
        }
        let mut buf = vec![0u8; ret_len as usize];
        let status = NtQueryInformationProcess(
            h,
            ProcessCommandLineInformation,
            buf.as_mut_ptr() as *mut c_void,
            ret_len,
            &mut ret_len,
        );
        if status.is_err() {
            if (ret_len as usize) > buf.len() {
                buf.resize(ret_len as usize, 0);
                let status = NtQueryInformationProcess(
                    h,
                    ProcessCommandLineInformation,
                    buf.as_mut_ptr() as *mut c_void,
                    ret_len,
                    &mut ret_len,
                );
                if status.is_err() {
                    let _ = CloseHandle(h);
                    return String::new();
                }
            } else {
                let _ = CloseHandle(h);
                return String::new();
            }
        }
        let _ = CloseHandle(h);
        if (ret_len as usize) < std::mem::size_of::<UnicodeString>() {
            return String::new();
        }
        let us = &*(buf.as_ptr() as *const UnicodeString);
        utf16_from_cmd_buf(&buf, us)
    }
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
    let _ = hidden_output(Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]));
}

pub fn list_numeric_pids() -> Vec<i32> {
    snapshot_processes().into_iter().map(|p| p.0).collect()
}

pub fn process_rows() -> Vec<(i32, i32, String)> {
    snapshot_processes()
}

pub fn unix_ssh_procs() -> Vec<OsProc> {
    let rows = snapshot_processes();
    let mut names = HashMap::new();
    for (pid, _ppid, name) in &rows {
        names.insert(*pid, name.clone());
    }
    let mut out = Vec::new();
    for (pid, ppid, name) in rows {
        let lower = name.to_lowercase();
        if lower != "ssh.exe" && !lower.contains("ssh") {
            continue;
        }
        let cmd = process_cmdline(pid);
        if cmd.is_empty() {
            continue;
        }
        out.push(OsProc {
            pid,
            ppid,
            name: name.clone(),
            parent: names.get(&ppid).cloned().unwrap_or_default(),
            cmd,
        });
    }
    out
}

pub fn pids_listening_on(port: &str) -> Vec<i32> {
    let port = port.trim();
    if port.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
        return Vec::new();
    }
    let mut pids: Vec<i32> = tcp_listeners()
        .into_iter()
        .filter(|(p, _, _)| p == port)
        .map(|(_, pid, _)| pid)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    if !pids.is_empty() {
        return pids;
    }
    if let Ok(out) = hidden_output(Command::new("netstat").args(["-ano"])) {
        if out.status.success() {
            return parse_netstat_tlnp(&String::from_utf8_lossy(&out.stdout), port);
        }
    }
    Vec::new()
}

pub fn listen_ports_for_pids(pids: &[i32]) -> HashMap<i32, Vec<ListenPort>> {
    let want: std::collections::HashSet<i32> = pids.iter().copied().filter(|p| *p > 0).collect();
    if want.is_empty() {
        return HashMap::new();
    }
    let mut out: HashMap<i32, Vec<ListenPort>> = HashMap::new();
    for (port, pid, loopback) in tcp_listeners() {
        if !want.contains(&pid) {
            continue;
        }
        let row = ListenPort { port, loopback };
        let list = out.entry(pid).or_default();
        if !list.iter().any(|p| p.port == row.port) {
            list.push(row);
        }
    }
    if !out.is_empty() {
        return out;
    }
    if let Ok(ns) = hidden_output(Command::new("netstat").args(["-ano"])) {
        if ns.status.success() {
            let text = String::from_utf8_lossy(&ns.stdout);
            for (port, pid) in parse_netstat_listen_table(&text) {
                if !want.contains(&pid) {
                    continue;
                }
                let row = ListenPort {
                    port,
                    loopback: true,
                };
                let list = out.entry(pid).or_default();
                if !list.iter().any(|p| p.port == row.port) {
                    list.push(row);
                }
            }
        }
    }
    out
}

fn tcp_listeners() -> Vec<(String, i32, bool)> {
    let mut out = Vec::new();
    out.extend(tcp4_listeners());
    out.extend(tcp6_listeners());
    out
}

fn tcp4_listeners() -> Vec<(String, i32, bool)> {
    unsafe {
        let mut size = 0u32;
        let probe = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if probe != 0 && probe != ERROR_INSUFFICIENT_BUFFER {
            return Vec::new();
        }
        if size == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size as usize];
        let err = GetExtendedTcpTable(
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut size,
            false,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if err != 0 {
            return Vec::new();
        }
        let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        let max = buf.len().saturating_sub(4) / row_size.max(1);
        let n = (table.dwNumEntries as usize).min(max);
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), n);
        let mut out = Vec::new();
        for row in rows {
            let port = u16::from_be(row.dwLocalPort as u16);
            if port == 0 {
                continue;
            }
            let pid = row.dwOwningPid as i32;
            if pid <= 0 {
                continue;
            }
            out.push((
                port.to_string(),
                pid,
                row.dwLocalAddr.to_ne_bytes() == [127, 0, 0, 1],
            ));
        }
        out
    }
}

fn tcp6_listeners() -> Vec<(String, i32, bool)> {
    unsafe {
        let mut size = 0u32;
        let probe = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET6,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if probe != 0 && probe != ERROR_INSUFFICIENT_BUFFER {
            return Vec::new();
        }
        if size == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size as usize];
        let err = GetExtendedTcpTable(
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut size,
            false,
            AF_INET6,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if err != 0 {
            return Vec::new();
        }
        let table = &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
        let row_size = std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
        let max = buf.len().saturating_sub(4) / row_size.max(1);
        let n = (table.dwNumEntries as usize).min(max);
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), n);
        let mut out = Vec::new();
        for row in rows {
            let port = u16::from_be(row.dwLocalPort as u16);
            if port == 0 {
                continue;
            }
            let pid = row.dwOwningPid as i32;
            if pid <= 0 {
                continue;
            }
            out.push((port.to_string(), pid, ipv6_loopback(&row)));
        }
        out
    }
}

fn ipv6_loopback(row: &MIB_TCP6ROW_OWNER_PID) -> bool {
    let a = row.ucLocalAddr;
    a == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
        || a == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 127, 0, 0, 1]
}

fn utf16_from_cmd_buf(buf: &[u8], us: &UnicodeString) -> String {
    if us.length == 0 {
        return String::new();
    }
    let n = (us.length / 2) as usize;
    if n == 0 {
        return String::new();
    }
    let bytes = n.saturating_mul(2);
    let buf_start = buf.as_ptr() as usize;
    let buf_end = buf_start.saturating_add(buf.len());
    let p = us.buffer as usize;
    let src = if !us.buffer.is_null() && p >= buf_start && p.saturating_add(bytes) <= buf_end {
        us.buffer
    } else {
        let fallback = buf_start.saturating_add(std::mem::size_of::<UnicodeString>());
        if fallback.saturating_add(bytes) > buf_end {
            return String::new();
        }
        fallback as *const u16
    };
    unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(src, n)) }
        .trim()
        .to_string()
}

fn hidden_output(cmd: &mut Command) -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.output()
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
