[English](04-install.md) | [中文](04-install.zh.md)

# Install and upgrade

Corvus is a local daemon (`corvusd`) plus a command-line client (`corvus`) and one browser extension.
This page installs the daemon as a Windows service with nssm, loads the extension, connects an MCP
client, and covers upgrade and uninstall. The service binds only to `127.0.0.1`.

## Default layout

| Item | Default |
|---|---|
| Service name | `Corvus` |
| Binaries | `%USERPROFILE%\.corvus\bin` (`corvusd.exe`, `corvus.exe`) |
| Data directory | `%USERPROFILE%\.corvus` |
| Database | `%USERPROFILE%\.corvus\corvus.db` |
| Extension | `%USERPROFILE%\.corvus\extension` |
| Logs | `%USERPROFILE%\.corvus\corvusd.log`, `%USERPROFILE%\.corvus\corvusd.err.log` |
| WebSocket | `ws://127.0.0.1:7330` |
| MCP HTTP | `http://127.0.0.1:7331/mcp` |

## Requirements

- Windows 10 or Windows 11, 64-bit.
- PowerShell 5.1 or PowerShell 7, started as Administrator for install and uninstall.
- nssm 2.24 or later. The installer looks on `PATH`, in the install directory, in the Chocolatey bin
  directory, in `C:\Program Files\nssm`, and at `-NssmPath`.
- A release bundle in `dist/` containing `corvusd.exe`, `corvus.exe` and `extension/`.
- Chrome, Edge or Brave (Chromium 120+) for the extension.
- A Rust toolchain only when building the bundle from source.

## Build the release bundle

Skip this section when you already have a prepared or trusted `dist/` bundle. From the repository root in
MSYS bash or Git Bash on Windows:

```bash
bash scripts/dev-setup.sh
bash scripts/package.sh
(cd dist && sha256sum -c SHA256SUMS)
```

`scripts/package.sh` runs an offline release build, then writes `dist/corvusd.exe`, `dist/corvus.exe`,
`dist/extension/`, `dist/VERSION` and `dist/SHA256SUMS`. The build scripts pass `--offline` and use the
local cargo cache. On a fresh machine, populate that cache once with `cargo fetch` while network access
is available; CI builds use the network normally. The Windows installer expects the `.exe` names, so
build the Windows bundle on Windows.

## Install the service

Open an elevated PowerShell in the repository root. The default install is one command:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
```

If nssm is not on `PATH`, pass its full path:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 `
  -NssmPath "C:\tools\nssm\nssm.exe"
```

To install outside the user profile, pass all three paths. Use the same values for upgrade and uninstall:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 `
  -InstallDir "D:\Corvus\bin" -DataDir "D:\Corvus\data" -ExtensionDir "D:\Corvus\extension"
```

The installer validates elevation and paths, refuses to run when the release binaries are missing, stops
an existing service, copies the binaries and the packaged extension, configures the nssm service, and
starts it. The service runs `corvusd.exe serve --db "<DataDir>\corvus.db"`. It is idempotent: re-running
the same command upgrades an existing install in place and preserves the database. If the service does
not reach `Running` within 30 seconds, the installer prints the recent log lines and exits non-zero.

## Load the extension

1. Open `chrome://extensions` or `edge://extensions`.
2. Turn on Developer mode.
3. Choose Load unpacked and select `%USERPROFILE%\.corvus\extension` (or your `-ExtensionDir`).
4. Open the extension popup. A green dot means the daemon connection is up; the card shows the browser
   id, extension version and capabilities.

After an upgrade, use Reload on the extension card when the manifest or scripts changed. The extension
has no settings page and no pairing step.

## Verify the install

```powershell
Get-Service Corvus | Format-Table Name, Status, StartType
& "$env:USERPROFILE\.corvus\bin\corvus.exe" doctor
Invoke-RestMethod http://127.0.0.1:7331/health
```

`doctor` reports the version, whether the WebSocket and MCP HTTP ports are reachable, the database path,
and one line per connected browser. `health` returns `status: "ok"` and the active database path. A
reachable daemon with no browser lines means the extension is not loaded yet or is using another port.

## Connect an MCP client

### HTTP transport

Use HTTP when the Windows service stays installed. Generate the entry with the installed binary:

```powershell
& "$env:USERPROFILE\.corvus\bin\corvus.exe" config --transport http
```

Paste the emitted `mcpServers` object into the client configuration. It points at
`http://127.0.0.1:7331/mcp`; keep the service running. `--client` takes a client name and adds a location
hint on stderr; the JSON on stdout is the same schema for every client.

### stdio transport

Use stdio only when the client cannot use HTTP. A stdio client starts its own `corvus.exe serve --stdio`
process, which needs the same loopback ports as the service. Stop the service first or the new process
fails with a port-in-use error:

```powershell
Stop-Service Corvus
& "$env:USERPROFILE\.corvus\bin\corvus.exe" config --transport stdio
```

The client spawns the command from that JSON. Keep the service stopped while the stdio client is in use,
and start it again with `Start-Service Corvus` when you return to HTTP. Do not run both transports at the
same time on the default ports.

## Five-minute walkthrough

This is the M4 acceptance walkthrough. A prepared `dist/` keeps it inside five minutes; a clean release
build adds compile time.

| Time | Step | Command or action | Success signal |
|---|---|---|---|
| 0:00 | Check prerequisites | Windows 10/11, elevated PowerShell, nssm, Chrome, Edge or Brave (Chromium 120+) | `nssm version` prints a version |
| 0:30 | Build or locate the bundle | `bash scripts/package.sh` (skip if `dist/` exists) | `dist/corvusd.exe` and `dist/corvus.exe` exist |
| 1:30 | Install the service | `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1` | installer ends with `service is running` |
| 2:30 | Load the extension | Load unpacked: `%USERPROFILE%\.corvus\extension` | popup dot is green |
| 3:30 | Connect a client | `corvus.exe config --transport http` and paste the entry | client lists the Corvus server |
| 4:30 | First page | Call `browser_navigate` with `{"url":"https://example.com"}`, then `browser_snapshot` | snapshot returns the page title and nodes |

## Upgrade

1. Build or obtain the new bundle.
2. Run the same installer command with the same paths.
3. Reload the extension if its files changed.
4. Run `doctor` and check `version`.

```bash
bash scripts/package.sh
```

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
& "$env:USERPROFILE\.corvus\bin\corvus.exe" doctor
```

The installer stops the service, replaces the binaries and extension, rewrites the service parameters,
and starts the service again. The database and logs are not touched. For a schema-sensitive upgrade, copy
`corvus.db` while the service is stopped before installing the new build.

## Uninstall

Run from an elevated PowerShell in the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall-windows.ps1
```

This stops and removes the `Corvus` service, removes `corvusd.exe` and `corvus.exe` from the install
directory, and removes the extension directory. It keeps `corvus.db` and the log files by default. To
delete the database and logs as well:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall-windows.ps1 -PurgeData
```

Pass `-InstallDir`, `-DataDir`, `-ExtensionDir` and `-ServiceName` when you installed a custom layout. The
script is idempotent and reports when the service is already absent. It leaves the empty install and data
directories in place; remove them manually after a purge if you want a clean disk.

## Service control and troubleshooting

```powershell
Get-Service Corvus
Restart-Service Corvus
nssm restart Corvus
```

| Symptom | Check | Fix |
|---|---|---|
| `nssm.exe not found` | `nssm version` | Put nssm on `PATH` or pass `-NssmPath` |
| Service does not start | `%USERPROFILE%\.corvus\corvusd.err.log` | Fix the reported path, port or permission error, then rerun the installer |
| Port 7330 or 7331 is in use | `netstat -ano | findstr :7330` | Stop the other daemon; do not run service and stdio mode together |
| `doctor` shows no browsers | popup dot, `Get-Service Corvus` | Load or reload the extension; confirm the WebSocket port is 7330 |
| Client cannot reach HTTP | `Invoke-RestMethod http://127.0.0.1:7331/health` | Start the service; use `127.0.0.1`, not a LAN address |
| Package fails offline | `bash scripts/dev-setup.sh` | Populate the cargo cache once with network access, or use a prepared `dist/` |
| Script is blocked by policy | error text names ExecutionPolicy | Use `-ExecutionPolicy Bypass` as shown above |
| `missing ...corvusd.exe` | `dist/` contents | Run `bash scripts/package.sh` first |
| page_text/page_snapshot return TIMEOUT after the extension is reloaded or loaded from a new directory | tab_list still lists the tab; extension card is green | Reload the tab or open its URL again. Tabs keep the previous extension content script until reloaded; tab_list does not need a content script |

## Security notes

The daemon binds to `127.0.0.1` only and has no authentication layer: any local process that can reach the
ports can drive the connected browser. Do not expose ports 7330 or 7331 beyond loopback. The installer
registers the service with the nssm default account, LocalSystem; change the service account with nssm if
your policy requires a dedicated identity. The database contains recorded commands, arguments and results,
so treat `%USERPROFILE%\.corvus\corvus.db` as sensitive data and purge it before handing the machine to
someone else.
