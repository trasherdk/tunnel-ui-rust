# tunnel-ui

Terminal UI (and small CLI) for **SSH local port forwards** on Linux.

Save setups, start and stop them, reconnect when the SSH session drops, and see other live `ssh` forwards (including Cursor / VS Code Remote-SSH) without touching them.

This is the Linux Rust port of [trasherdk/tunnel-ui](https://github.com/trasherdk/tunnel-ui). Config files are the same `configs/*.conf` format as the Windows Go app, so setups can be copied between machines.

It is not an interactive SSH client and not a VPN. It does not stop SSH processes started by other tools.

## Requirements

- Linux (developed on headless Slackware; other distros are fine)
- A **recent stable Rust** (`rustc` + `cargo`, edition 2021). Distro packages can be too old (Slackware 15 ships 1.58; Ratatui needs a current toolchain). [rustup](https://rustup.rs/) is the usual fix.
- **OpenSSH** client: `ssh` on `PATH`, or set `SSH` to the binary

No systemd. No Go.

## Build

From this directory:

```bash
cargo test
cargo build --release
```

The binary is `target/release/tunnel-ui`. Keep it named `tunnel-ui` (the name is how the app recognizes its own supervisor).

`cargo run` and `target/debug` / `target/release` use the **current working directory** for `configs/` and `.state/`. An installed copy (not under `target/debug` or `target/release`) uses the directory the binary lives in, unless you set `TUNNEL_HOME`.

```bash
# run from the crate root so configs/ next to cwd resolve
./target/release/tunnel-ui

# or install somewhere and point data at a home dir
install -m 755 target/release/tunnel-ui /usr/local/bin/tunnel-ui
TUNNEL_HOME=/var/lib/tunnel-ui /usr/local/bin/tunnel-ui
```

## Keys

| Key | Action |
| --- | --- |
| Enter | Actions for a saved setup, or inspect a detected process |
| n | New setup |
| r | Refresh on/off status and other SSH processes |
| / | Filter the list |
| q | Quit |
| Esc | Cancel a form |

Saved setups: **Start**, **Stop**, **Status**, **Edit**, **Delete**.

Start, Stop, and Delete return to the list on success. Errors and Status show a result screen.

## Setups

Two kinds, chosen on the New/Edit form:

1. **SSH config alias** — `Host` name from `~/.ssh/config`
2. **User, host, and key** — remote user, hostname, and private key path

Shared fields: comment (what the tunnel is for), local port, remote host/port (as seen from the SSH host), auto-reconnect.

**Edit** (and **New**) shows every setting. Pick one to change, then **Save** to write the file and return to the list. Esc cancels without saving. Rename is not supported; delete and create a new setup.

List status:

- `[on]` — this app’s SSH forward is listening
- `[off]` — not running
- `[err]` — supervisor or last start failed (host key, auth, bind, and similar). The status bar shows the SSH message.

Saved files live in `configs/<name>.conf`. Runtime PIDs and logs live in `.state/` (not committed). See `configs/mysql-example.conf.example`.

## CLI

The same binary can run without the TUI:

```bash
tunnel-ui start <name>
tunnel-ui stop <name>
tunnel-ui status [name]
tunnel-ui delete <name>
```

Start/stop spawn a background supervisor (`setsid`) that runs `ssh -N -L …`. If the session drops, it reconnects. Config and auth failures (host key verification, permission denied, unusable key, DNS, local bind) **do not** loop; the supervisor exits and the TUI/CLI reports the error.

## Other tunnels

The list also shows live `ssh` processes this app did not start, including Cursor/VS Code Remote-SSH (`ssh -T -D …`). Those rows are **inspect-only** — the TUI will not stop them.

## Environment

| Variable | Meaning |
| --- | --- |
| `TUNNEL_HOME` | App root (`configs/`, `.state/`). Default: directory of the executable, or cwd when running from `target/debug` or `target/release` |
| `TUNNEL_CONFIG_DIR` | Saved `*.conf` |
| `TUNNEL_STATE_DIR` | PIDs and logs |
| `SSH` | `ssh` executable |

## Host keys

OpenSSH stores host keys under the **name you typed**, not the DNS name it later resolves. `ssh user@short-name` does not automatically cover `user@short-name.example.com`. Use the same host string in the setup as in a working `ssh`, or connect once with each name you will use.

## License

Use and modify as you like.
