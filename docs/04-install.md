[English](04-install.md) | [中文](04-install.zh.md)

# Install, upgrade and uninstall

Corvus is one local daemon (`corvusd`), one CLI (`corvus`) and one browser extension. Every backend file lives under a single root selected by `--home` or `CORVUS_HOME`: binaries, database, logs, configuration and the unpacked extension. This page covers the Windows service install with nssm and the Linux systemd install from the release tarball. Docker, Kubernetes, backups and the full operations checklist are in [05-deployment.md](05-deployment.md). The daemon binds to `127.0.0.1` by default.

## Default layout

| Platform | Default root | How it starts |
|---|---|---|
| Windows service | `%ProgramData%\Corvus` | `install-windows.ps1` registers the `Corvus` service through nssm |
| Windows foreground | `%LOCALAPPDATA%\Corvus` | `corvusd serve` or `corvus serve` |
| Linux package | `/opt/corvus` | `install-linux.sh` writes `/etc/systemd/system/corvus.service` |
| Linux foreground | `$XDG_DATA_HOME/corvus` or `~/.local/share/corvus` | `corvusd serve` |
| Docker / Kubernetes | `/data` | container entrypoint or Kubernetes manifest |
| macOS from source | `~/Library/Application Support/Corvus` | `corvusd serve`; no packaged installer in 1.2.0 |

One root contains every backend file:

    <root>/
      bin/            corvusd, corvus, and nssm.exe on Windows
      data/corvus.db  SQLite recording store
      logs/           daily file log and service stdout/stderr files
      config/         reserved for file configuration
      extension/      unpacked extension

| Port | Purpose | Default |
|---|---|---|
| 7330 | WebSocket transport for the extension | `ws://127.0.0.1:7330` |
| 7331 | MCP streamable HTTP, `/health` and `/metrics` | `http://127.0.0.1:7331/mcp` |

The database is `<root>/data/corvus.db` unless `--db <path>` is given. A configured `--home` or `CORVUS_HOME` takes precedence over `CORVUS_DB_PATH`; without a home override, `CORVUS_DB_PATH` is still honoured. File logging defaults to `<root>/logs/corvusd.log` and rotates daily in UTC, producing `corvusd.log.YYYY-MM-DD`. `--log-file` overrides the path and `--no-log-file` disables file output.

## Requirements

### Windows

- Windows 10 or Windows 11, 64-bit.
- PowerShell 5.1 or PowerShell 7, started as Administrator for install and uninstall.
- A release ZIP built on Windows. It contains `corvusd.exe`, `corvus.exe`, the extension, the install and uninstall scripts, and the vendored `nssm.exe`.
- nssm 2.24 or later. The package vendors it in `bin\nssm.exe`; a system copy can be selected with `-NssmPath`.
- Chrome, Edge or Brave (Chromium 120 or later) for the extension.

### Linux

- Linux x86_64 with glibc and systemd.
- Root access through `sudo`; `install-linux.sh` requires systemd and writes a unit under `/etc/systemd/system`.
- `curl` is recommended for the installer health check and for verification; the install succeeds with a warning when `curl` is absent.
- The `corvus-<version>-linux-x86_64.tar.gz` archive and its `SHA256SUMS`.

Building from source needs Rust 1.90 or later. macOS is supported from source only in this release.

## Build the release packages

With a prepared, trusted package, skip this section. From the repository root:

```bash
# Windows ZIP: dist/corvus-<version>-windows-x64.zip
bash scripts/package.sh

# Linux tarball: dist/corvus-<version>-linux-x86_64.tar.gz
bash scripts/package-linux.sh
```

The Windows package builds offline by default against the local cargo cache; set `CORVUS_CARGO_OFFLINE=0` to let CI use the network. The Linux package builds with normal Cargo network access (`cargo build --release --locked`). Both scripts verify the entries and checksums of the staged package. On a fresh machine, populate the cache once with `cargo fetch` while network access is available. The Windows installer expects `.exe` names, so build the Windows package on Windows. The Linux package is x86_64 only.

## Install on Windows

The release ZIP contains `install-windows.ps1` and `uninstall-windows.ps1` at its root. Open an elevated PowerShell in the extracted archive and run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install-windows.ps1
```

From a repository checkout, build the bundle with `bash scripts/package.sh` and run `.\scripts\install-windows.ps1` instead.

Select a different root and ports with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install-windows.ps1 `
  -HomeDir "D:\Corvus" -WsPort 7330 -HttpPort 7331
```

In a repository checkout, replace `.\install-windows.ps1` with `.\scripts\install-windows.ps1` in either command.

The installer validates elevation and the root path, refuses to run when the release binaries are missing, stops an existing `Corvus` service, copies `bin/` and `extension/`, stops and removes an older service registration, and registers the nssm service. The service command is:

```text
corvusd.exe serve --home "<HomeDir>" --ws-port <WsPort> --http-port <HttpPort>
```

It is idempotent: re-running the same command upgrades an existing install in place and preserves the database. If the service does not reach `Running` within 30 seconds, the installer prints recent log lines and exits non-zero.

`-InstallDir`, `-DataDir` and `-ExtensionDir` remain accepted as deprecated aliases for upgrade compatibility, but they must resolve to subdirectories of `-HomeDir`. New installs should pass `-HomeDir` only.

### Migration from 1.0.0

A 1.0.0 install kept its files in `%USERPROFILE%\.corvus`. The installer moves that directory to `<HomeDir>\migration\userprofile-corvus`; if `<HomeDir>\data\corvus.db` does not exist yet, it copies `corvus.db`, `corvus.db-wal` and `corvus.db-shm` into the new data directory. Pass `-KeepLegacyData` to leave the old directory in place. Back up the old database before upgrading when the recording history matters.

### Load the extension

1. Open `chrome://extensions` or `edge://extensions`.
2. Turn on Developer mode.
3. Choose Load unpacked and select `<HomeDir>\extension`.
4. Open the extension popup. A green dot means the daemon connection is up; the card shows the browser id, extension version and capabilities.

The extension has no pairing step. Its optional options page sets the daemon WebSocket URL; see [Extension options page](#extension-options-page).

## Install on Linux

Download or copy the tarball, extract it into a directory, then verify and install:

```bash
mkdir -p corvus-1.2.0-linux-x86_64
tar -xzf corvus-1.2.0-linux-x86_64.tar.gz -C corvus-1.2.0-linux-x86_64
cd corvus-1.2.0-linux-x86_64
sha256sum -c SHA256SUMS
sudo ./install-linux.sh
```

The installer creates a system user named `corvus` (no home, `nologin` shell), installs `bin/` and `extension/` as root, makes `data/`, `logs/` and `config/` writable by the service user, writes the systemd unit, enables it and restarts the service. It then polls `http://127.0.0.1:<HttpPort>/health` for up to 30 seconds.

Override the root, service user and ports with environment variables:

```bash
sudo CORVUS_HOME=/var/lib/corvus CORVUS_USER=corvus \
  CORVUS_WS_PORT=7330 CORVUS_HTTP_PORT=7331 ./install-linux.sh
```

`CORVUS_HOME` must be a dedicated absolute path outside system and home directories. The service unit is `/etc/systemd/system/corvus.service`; logs go to the daily file under `<root>/logs` and to the journal.

Verify the install with `systemctl status corvus.service`, `curl -fsS http://127.0.0.1:7331/health`, and `sudo -u corvus <root>/bin/corvus doctor`.

## Extension options page

The extension connects to `ws://127.0.0.1:7330` by default. To use another daemon, or a `wss://` tunnel, open the extension options page (or the Change link in the popup), enter the full WebSocket URL, and save. The URL is stored under `corvus.daemonUrl`; Reset restores the default. After reloading or updating the extension, reload open tabs so their content scripts reconnect.

A `wss://` endpoint must terminate TLS outside Corvus, for example at a reverse proxy. The daemon itself has no authentication, so a tunnel that exposes port 7330 or 7331 beyond loopback exposes full browser control. See [05-deployment.md](05-deployment.md#7-ports-exposure-and-the-no-authentication-boundary).

### CDP endpoint (optional)

The default zero-setup path uses the extension only. To add browser-level CDP, start a Chromium browser with remote debugging on a loopback port and a dedicated profile:

```bash
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile
curl -s http://127.0.0.1:9222/json/version
```

Use the `webSocketDebuggerUrl` from that response, not the HTTP URL. Then either set the extension options page CDP endpoint to `auto` (it probes 9222, 9223 and 9333) or an explicit `http://127.0.0.1:9222`, or start the daemon with:

```bash
corvusd serve --home /opt/corvus --cdp-endpoint "ws://127.0.0.1:9222/devtools/browser/<id>"
corvusd serve --home /opt/corvus --channel extension   # ignore CDP
corvusd serve --home /opt/corvus --no-cdp              # remove the CDP path
```

`--channel` accepts `auto`, `cdp` or `extension`; `CORVUS_CHANNEL` sets the same value. The CDP endpoint is loopback-only and has no authentication. Never expose 9222, 9223 or 9333 beyond loopback. See [07-cdp.md](07-cdp.md) for the full guide.

## Verify the install

On Windows:

```powershell
Get-Service Corvus | Format-Table Name, Status, StartType
& "$env:ProgramData\Corvus\bin\corvus.exe" doctor
Invoke-RestMethod http://127.0.0.1:7331/health
Invoke-WebRequest http://127.0.0.1:7331/metrics | Select-Object -Expand Content
```

On Linux:

```bash
systemctl status corvus.service
/opt/corvus/bin/corvus doctor
curl -fsS http://127.0.0.1:7331/health
curl -fsS http://127.0.0.1:7331/metrics
```

`doctor` reports the version, whether the WebSocket and MCP HTTP ports are reachable, uptime, tool-call counters, the database path, schema version, session and step counts, and one line per connected browser. `/health` returns `status: "ok"`, the bound addresses and the recording block. A reachable daemon with no browser lines means the extension is not loaded yet or points at another port. When CDP is expected, call `browser_channel` with no arguments and `cdp_status` through your MCP client; the status lists the active channel, endpoint and browser, and any `last_error` or `browser_mismatch`.

## Connect an MCP client

### HTTP transport

Use HTTP when the service stays installed. Generate the entry with the installed binary:

```powershell
& "$env:ProgramData\Corvus\bin\corvus.exe" config --client cursor --transport http
```

Paste the emitted `mcpServers` object into the client configuration. It points at `http://127.0.0.1:7331/mcp`; keep the service running. `--client` accepts `claude`, `cursor`, `dsh`, `cline` or `generic` and adds a location hint on stderr. The JSON on stdout is the same schema for every client, so redirecting stdout produces a valid configuration file.

### stdio transport

Use stdio when the client manages the process itself. The generated entry starts `corvus serve --stdio`. A stdio client needs the same loopback ports as the service, so stop the service first or the new process fails with a port-in-use error:

```powershell
Stop-Service Corvus
& "$env:ProgramData\Corvus\bin\corvus.exe" config --client generic --transport stdio
```

To pin a custom root in the generated command, add `--home <dir>`; the stdio arguments become `serve --stdio --home <dir>`. Keep the service stopped while the stdio client is in use, and start it again with `Start-Service Corvus` when you return to HTTP. Do not run both transports on the default ports at the same time.

## Five-minute walkthrough

This is the Windows acceptance walkthrough. A prepared ZIP keeps it inside five minutes; a clean release build adds compile time.

| Time | Step | Command or action | Success signal |
|---|---|---|---|
| 0:00 | Check prerequisites | Windows 10/11, elevated PowerShell, Chrome, Edge or Brave (Chromium 120+) | Windows and browser versions are supported |
| 0:30 | Build or locate the package | `bash scripts/package.sh` (skip when the ZIP exists) | `dist/corvusd.exe` and `dist/corvus.exe` exist |
| 1:30 | Install the service | `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1` | installer ends with the service running |
| 2:30 | Load the extension | Load unpacked: `%ProgramData%\Corvus\extension` | popup dot is green |
| 3:30 | Connect a client | `corvus.exe config --client cursor --transport http` and paste the entry | client lists the Corvus server |
| 4:30 | First page | Call `browser_navigate` with `{"url":"https://example.com"}`, then `page_snapshot` | snapshot returns the page title and nodes |

## Upgrade

Back up `<root>/data/corvus.db` when the recording history matters. Then install the new package the same way:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
& "$env:ProgramData\Corvus\bin\corvus.exe" doctor
```

```bash
sudo ./install-linux.sh
systemctl status corvus.service
/opt/corvus/bin/corvus doctor
```

The installers stop the service, replace `bin/` and `extension/`, rewrite the service parameters, and start the service again. The database, logs and configuration are not deleted. Reload the extension and its open tabs when extension files changed. Docker and Kubernetes upgrades are covered in [05-deployment.md](05-deployment.md).

## Uninstall

On Windows, run from an elevated PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall-windows.ps1
```

The script stops and removes the `Corvus` service, removes the binaries and extension, and keeps the database and logs by default. Add `-PurgeData` to delete the whole root after the service is removed. Pass `-HomeDir` and `-ServiceName` when the install used custom values; `-SelfTest` checks the path guard without changing anything.

On Linux:

```bash
sudo ./uninstall-linux.sh
# also remove data, logs and the service user
sudo ./uninstall-linux.sh --purge
```

The default uninstall keeps `<root>/data`, `<root>/logs` and `<root>/config`. `--purge` removes the whole root plus the service user and group. Run `--self-test` to check the path guard only.

## Service control and troubleshooting

Windows:

```powershell
Get-Service Corvus
Restart-Service Corvus
nssm restart Corvus
```

Linux:

```bash
systemctl status corvus.service
systemctl restart corvus.service
journalctl -u corvus.service -n 100 --no-pager
```

| Symptom | Check | Fix |
|---|---|---|
| `nssm.exe not found` | `nssm version` | Use the vendored `bin\nssm.exe`, put nssm on `PATH`, or pass `-NssmPath` |
| Service does not start on Windows | `<root>\logs\corvusd.err.log` and `<root>\logs\corvusd.log.YYYY-MM-DD` | Fix the reported path, port or permission error, then rerun the installer |
| Service does not start on Linux | `systemctl status corvus.service` and `journalctl -u corvus.service` | Fix the unit, path or port error, then rerun the installer |
| Port 7330 or 7331 is in use | `netstat -ano | findstr :7330` or `ss -ltnp | grep 733` | Stop the other daemon; do not run a service and stdio mode together |
| `doctor` shows no browsers | popup dot, service status, options page URL | Load or reload the extension; confirm the WebSocket URL and port 7330 |
| Client cannot reach HTTP | `Invoke-RestMethod http://127.0.0.1:7331/health` or `curl -fsS http://127.0.0.1:7331/health` | Start the service; use `127.0.0.1`, not a LAN address |
| Package fails offline | `bash scripts/dev-setup.sh` | Populate the cargo cache once with network access, or use a prepared package |
| Script is blocked by policy on Windows | error text names ExecutionPolicy | Use `-ExecutionPolicy Bypass` as shown above |
| `missing ...corvusd.exe` | package contents | Run `bash scripts/package.sh` first |
| `page_text` or `page_snapshot` returns TIMEOUT after the extension was reloaded | `tab_list` still lists the tab; extension card is green | Reload the tab or open its URL again. Tabs keep the previous content script until reloaded; `tab_list` does not need one |
| `browser_channel` reports `cdp` but disconnected | `cdp_status.connected` and `last_error` | The browser or endpoint changed. Restart the daemon with the current `webSocketDebuggerUrl`, or let the extension rediscover it |
| CDP endpoint rejected | endpoint scheme, host and path | Use the loopback `ws://` or `wss://` browser URL from `/json/version`; HTTP discovery URLs, LAN hosts and paths are rejected |
| Extension-only tool fails under forced `cdp` | `browser_channel` mode | Switch to `auto` or `extension`; forced CDP never falls back |

## Security notes

The daemon binds to `127.0.0.1` only and has no authentication layer: any local process that can reach the ports can drive the connected browser. Do not expose ports 7330 or 7331 beyond loopback. A configured CDP endpoint has the same trust model and can read cookies, inject script and control every profile in that browser instance; keep 9222, 9223 and 9333 on loopback too. The Windows installer registers the service with the nssm default account, LocalSystem; change the service account with nssm when policy requires a dedicated identity. The root contains recorded commands, arguments and results, so treat `<root>/data/corvus.db` and the logs as sensitive data and remove them before handing the machine to someone else. The Linux installer creates a dedicated `corvus` service user and restricts `data/`, `logs/` and `config/` to it.
