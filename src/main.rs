mod config;
mod detect;
mod paths;
mod proc;
mod prune;
mod ssh;
mod tunnel;
mod ui;

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Result;

use crate::paths::Paths;

/// App version; keep in lockstep with `[package].version` in Cargo.toml.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn version_line() -> String {
    format!("tunnel-ui {VERSION}")
}

fn usage() -> String {
    format!(
        "\
tunnel-ui {VERSION} — SSH local-forward TUI

Usage:
  tunnel-ui                 Start the terminal UI
  tunnel-ui start <name>    Start a saved tunnel
  tunnel-ui stop <name>     Stop a saved tunnel
  tunnel-ui status [name]   Show status
  tunnel-ui delete <name>   Stop and delete a saved tunnel
  tunnel-ui -v | --version  Print version
  tunnel-ui -h | --help     Show this help
  tunnel-ui shortcut        Windows-only (not available on Linux)

Requires OpenSSH (ssh on PATH). Override the binary with SSH.
"
    )
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(io::stderr(), "{e:#}");
            ExitCode::from(1)
        }
    }
}

fn real_main() -> Result<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--supervisor") {
        let name = args.get(1).cloned().unwrap_or_default();
        if name.is_empty() {
            let _ = writeln!(io::stderr(), "usage: tunnel-ui --supervisor <name>");
            return Ok(ExitCode::from(2));
        }
        let paths = Paths::from_env();
        crate::paths::mkdir_755(&paths.state_dir)?;
        crate::tunnel::run_supervisor(&paths, &name)?;
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(cmd) = args.first().map(String::as_str) {
        match cmd {
            "-h" | "--help" | "help" => {
                print!("{}", usage());
                return Ok(ExitCode::SUCCESS);
            }
            "-v" | "--version" | "version" => {
                println!("{}", version_line());
                return Ok(ExitCode::SUCCESS);
            }
            "shortcut" => {
                println!("shortcut is Windows-only");
                return Ok(ExitCode::SUCCESS);
            }
            "start" | "stop" | "status" | "delete" => {
                let paths = Paths::from_env();
                paths.ensure_dirs()?;
                crate::prune::prune_orphan_state(&paths);
                match crate::tunnel::run_cli(&paths, &args) {
                    Ok(out) => {
                        if !out.is_empty() {
                            println!("{out}");
                        }
                        return Ok(ExitCode::SUCCESS);
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if !msg.is_empty() {
                            let _ = writeln!(io::stderr(), "{msg}");
                        }
                        return Ok(ExitCode::from(1));
                    }
                }
            }
            other => {
                let _ = writeln!(io::stderr(), "unknown command {other:?}");
                return Ok(ExitCode::from(1));
            }
        }
    }

    let paths = Paths::from_env();
    paths.ensure_dirs()?;
    crate::prune::prune_orphan_state(&paths);
    ui::run(paths)?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_cargo_toml() {
        assert_eq!(super::VERSION, env!("CARGO_PKG_VERSION"));
        assert!(!super::VERSION.is_empty());
        let line = super::version_line();
        assert!(line.starts_with("tunnel-ui "));
        assert!(line.contains(super::VERSION));
    }
}
