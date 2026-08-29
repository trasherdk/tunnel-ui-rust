use anyhow::{bail, Result};

#[cfg(not(windows))]
pub fn install_shortcut() -> Result<String> {
    bail!("Start Menu shortcuts are only supported on Windows");
}

#[cfg(windows)]
pub fn install_shortcut() -> Result<String> {
    use std::process::Command;

    use crate::paths::Paths;

    let exe = std::env::current_exe()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let exe_s = exe.to_string_lossy().into_owned();
    let dir = Paths::from_env().root.to_string_lossy().into_owned();
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let userprofile = std::env::var("USERPROFILE").unwrap_or_default();
    let start_menu =
        format!("{appdata}\\Microsoft\\Windows\\Start Menu\\Programs\\SSH tunnels.lnk");
    let desktop = format!("{userprofile}\\Desktop\\SSH tunnels.lnk");

    let ps = format!(
        r#"
$exe = {exe}
$dir = {dir}
function New-TunnelShortcut([string]$path) {{
  $ws = New-Object -ComObject WScript.Shell
  $s = $ws.CreateShortcut($path)
  $s.TargetPath = $exe
  $s.WorkingDirectory = $dir
  $s.WindowStyle = 1
  $s.IconLocation = "$exe,0"
  $s.Description = "SSH local-forward tunnels"
  $s.Save()
}}
New-TunnelShortcut {start}
New-TunnelShortcut {desk}
"#,
        exe = ps_quote(&exe_s),
        dir = ps_quote(&dir),
        start = ps_quote(&start_menu),
        desk = ps_quote(&desktop),
    );

    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps]);
    crate::spawn::detach_command(&mut cmd);
    let out = cmd.output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("create shortcut: {}", err.trim());
    }
    Ok(format!(
        "Shortcuts created:\n  {start_menu}\n  {desktop}\nPin from Start or the taskbar after you open it once."
    ))
}

#[cfg(windows)]
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
