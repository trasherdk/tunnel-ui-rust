# tunnel-ui

Terminal UI (and small CLI) for **SSH local port forwards** on Linux and Windows.

Save setups, start and stop them, reconnect when the SSH session drops, and see other live `ssh` forwards (including Cursor / VS Code Remote-SSH) without touching them.

This is the Rust port of [trasherdk/tunnel-ui](https://github.com/trasherdk/tunnel-ui). Config files are the same `configs/*.conf` format as the Windows Go app, so setups can be copied between machines.

It is not an interactive SSH client and not a VPN. It does not stop SSH processes started by other tools.

## Requirements

- Linux or Windows
- A **recent stable Rust** (`rustc` + `cargo`, edition 2021) if you build from source. Distro packages can be too old (Slackware 15 ships 1.58; Ratatui needs a current toolchain). [rustup](https://rustup.rs/) is the usual fix.
- **OpenSSH** client: `ssh` on `PATH`, or set `SSH` to the binary. On Windows, the built-in OpenSSH client is enough (`ssh.exe`).

No systemd. No Go.

## Release binaries

Pushing a version tag (`v0.1.0`, `v1.2.3`, …) runs GitHub Actions and attaches:

| File | What it is |
| --- | --- |
| `tunnel-ui-linux-x86_64` | Statically linked Linux binary (musl) |
| `tunnel-ui.exe` | Windows 64-bit executable (icon + GUI subsystem, same as the Go build) |

On Windows, after you put `tunnel-ui.exe` somewhere permanent (for example `%USERPROFILE%\.tunnel-ui\tunnel-ui.exe`), run:

```text
tunnel-ui shortcut
```

That creates **SSH tunnels.lnk** on the Desktop and in the Start Menu, using the exe icon, working directory `%USERPROFILE%\.tunnel-ui`, and AppUserModelID `dk.fumlersoft.tunnel-ui` so you can pin it. Double-clicking the shortcut opens its own console titled **SSH tunnels**.

Cut a release after bumping `[package].version` in `Cargo.toml`:

```bash
git tag v0.1.0
git push origin v0.1.0
```

## Build

From this directory:

```bash
cargo test
cargo build --release
```

The binary is `target/release/tunnel-ui` (Linux) or `target/release/tunnel-ui.exe` (Windows). Keep the name `tunnel-ui` (the name is how the app recognizes its own supervisor).

Version is `[package].version` in `Cargo.toml`. Print it with `tunnel-ui -v` or `tunnel-ui --version`. Bump that field when you cut a release.

`cargo run` and `target/debug` / `target/release` use the **current working directory** for `configs/` and `.state/` (dev). An installed copy uses `~/.tunnel-ui` on Linux and `%USERPROFILE%\.tunnel-ui` on Windows. `TUNNEL_HOME` overrides either.

```bash
# run from the crate root so repo configs/ resolve
./target/release/tunnel-ui

# install; data goes to ~/.tunnel-ui
install -m 755 target/release/tunnel-ui /usr/local/bin/tunnel-ui
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

Saved files live in `configs/<name>.conf`. Runtime PIDs and logs live in `.state/` (not committed). Under an installed binary that is `~/.tunnel-ui` (Linux) or `%USERPROFILE%\.tunnel-ui` (Windows). See `configs/mysql-example.conf.example`.

## CLI

The same binary can run without the TUI:

```bash
tunnel-ui -v
tunnel-ui start <name>
tunnel-ui stop <name>
tunnel-ui status [name]
tunnel-ui delete <name>
tunnel-ui shortcut   # Windows: Start Menu + Desktop shortcuts
```

Start/stop spawn a background supervisor that runs `ssh -N -L …`. If the session drops, it reconnects. Config and auth failures (host key verification, permission denied, unusable key, DNS, local bind) **do not** loop; the supervisor exits and the TUI/CLI reports the error.

## Other tunnels

The list also shows live `ssh` processes this app did not start, including Cursor/VS Code Remote-SSH (`ssh -T -D …`). Those rows are **inspect-only** — the TUI will not stop them.

## Environment

| Variable | Meaning |
| --- | --- |
| `TUNNEL_HOME` | App root (`configs/`, `.state/`). Default: `~/.tunnel-ui` / `%USERPROFILE%\.tunnel-ui`, or cwd when running from `target/debug` or `target/release` |
| `TUNNEL_CONFIG_DIR` | Saved `*.conf` |
| `TUNNEL_STATE_DIR` | PIDs and logs |
| `SSH` | `ssh` executable |

## Host keys

OpenSSH stores host keys under the **name you typed**, not the DNS name it later resolves. `ssh user@short-name` does not automatically cover `user@short-name.example.com`. Use the same host string in the setup as in a working `ssh`, or connect once with each name you will use.

## License

Use and modify as you like.
