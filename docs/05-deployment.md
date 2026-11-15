[English](05-deployment.md) | [中文](05-deployment.zh.md)

# Deploy Corvus

Corvus is one local daemon, one CLI and one browser extension. This guide covers the supported production layouts: a Windows service, a Linux systemd service, a Docker container and Kubernetes manifests. Every backend file lives under one root directory, selected by `--home` or the `CORVUS_HOME` environment variable.

## 1. Deployment model and one-root layout

The daemon owns one root. The default root depends on how it was started:

| Context | Root |
|---|---|
| Windows service (installer default) | `%ProgramData%\Corvus` |
| Windows foreground or CLI | `%LOCALAPPDATA%\Corvus`, falling back to `%USERPROFILE%\.corvus` |
| Linux package (systemd) | `/opt/corvus` |
| Linux foreground | `$XDG_DATA_HOME/corvus`, falling back to `$HOME/.local/share/corvus` |
| macOS foreground | `$HOME/Library/Application Support/Corvus` |
| Docker and Kubernetes | `/data` |

The layout below the root is fixed:

| Path | Contents |
|---|---|
| `bin/` | `corvusd` and `corvus` (Windows: `.exe`) |
| `data/corvus.db` | SQLite recording database, plus `-wal` and `-shm` while open |
| `logs/` | file logs; the rolling file name is `corvusd.log.YYYY-MM-DD` in UTC |
| `config/` | reserved for configuration; no file is loaded in this release |
| `extension/` | unpacked Manifest V3 extension |

Use `--home <DIR>` on `corvusd serve`, `corvusd doctor` and `corvus` to pin the root. `CORVUS_HOME` sets the same root when the flag is absent. Start with `--home` and the daemon creates `data/`, `logs/` and `config/` on startup; it does not create `bin/` or `extension/`, which packaging and installation own.

Database path precedence is:

1. `--db <PATH>`.
2. `<root>/data/corvus.db` when `--home` or `CORVUS_HOME` selected the root.
3. `CORVUS_DB_PATH`.
4. The platform default root.

Relative roots and relative `--log-file` values are resolved against the process working directory and the root respectively; use absolute paths in service definitions.

File logging is on by default under `logs/`. `--log-file <PATH>` overrides it, `--no-log-file` disables it, and the in-memory log ring plus stderr output stay enabled. A rolling file is always suffixed with its UTC date, for example `corvusd.log.2026-11-15`; it is not a bare `corvusd.log`.

## 2. Linux systemd installation

The Linux release archive is built for x86_64 only. Extract it into a directory, then install:

```bash
mkdir -p corvus-1.1.0-linux-x86_64
tar -xzf corvus-1.1.0-linux-x86_64.tar.gz -C corvus-1.1.0-linux-x86_64
cd corvus-1.1.0-linux-x86_64
sha256sum -c SHA256SUMS
sudo ./install-linux.sh
```

The installer needs root, `systemctl` and a running systemd. It creates a system user and group named `corvus`, installs the binaries under `/opt/corvus/bin`, copies the extension to `/opt/corvus/extension`, creates `data`, `logs` and `config` for the service user, writes `/etc/systemd/system/corvus.service`, enables and restarts the unit, and waits for `http://127.0.0.1:7331/health`.

Environment overrides:

```bash
sudo CORVUS_HOME=/var/lib/corvus CORVUS_USER=corvus \
  CORVUS_WS_PORT=7330 CORVUS_HTTP_PORT=7331 ./install-linux.sh
```

Home safety is enforced. The root must be absolute and dedicated: the installer refuses `/`, common system directories such as `/usr`, `/etc` and `/home`, paths containing `..` or unexpected characters, and paths below a symlinked ancestor. Verify the guard without changing the host:

```bash
./install-linux.sh --self-test
```

The unit runs as the unprivileged service user with a read-only system view, `ProtectHome=true`, no capabilities, SIGTERM shutdown, and `--log-file` under the root. `journalctl -u corvus` shows service output.

## 3. Linux upgrade and uninstall

Upgrade by extracting a newer archive and running the same installer. It stops the service, replaces the binaries and extension, rewrites the unit and restarts it; `data/corvus.db` and `logs/` are preserved. The database schema is version 1, so a 1.0.0 database opens in place.

Uninstall keeps data and logs by default:

```bash
sudo ./uninstall-linux.sh
```

Use `--purge` to remove the whole root and the service user. The script refuses unsafe roots before making any change and supports `--self-test`.

## 4. Package contents and verification

The Linux archive contains `VERSION`, `SHA256SUMS`, `bin/corvusd`, `bin/corvus`, the complete `extension/` tree, `install-linux.sh`, `uninstall-linux.sh` and `systemd/corvus.service`. `SHA256SUMS` covers `VERSION`, both binaries, both scripts, the unit and every extension file.

Always verify before installing:

```bash
sha256sum -c SHA256SUMS
```

The Windows bundle is a ZIP containing `corvusd.exe`, `corvus.exe`, `nssm.exe`, the extension tree, `VERSION`, `SHA256SUMS`, the install and uninstall scripts, and the nssm notice. See [04-install.md](04-install.md) for the Windows procedure.

## 5. Docker

Build the image from the repository root:

```bash
docker build -t corvus:1.1.0 .
```

The image runs `corvusd serve --bind 0.0.0.0 --home /data` as UID/GID 10001, stores runtime data under the `/data` volume, and exposes 7330 (WebSocket) and 7331 (MCP HTTP). The unpacked extension is carried at `/opt/corvus/extension`, not `/data`; copy it out with `docker cp` or load the extension from the release ZIP on the host. Publish only to host loopback:

```bash
docker volume create corvus-data
docker run -d --name corvus \
  -p 127.0.0.1:7330:7330 -p 127.0.0.1:7331:7331 \
  -v corvus-data:/data corvus:1.1.0
curl -fsS http://127.0.0.1:7331/health
```

Compose uses the same image, a named volume, a read-only root filesystem, a `/tmp` tmpfs, all capabilities dropped and `no-new-privileges`:

```bash
docker compose up -d --build
docker compose logs -f corvus
docker compose down
```

`docker compose down -v` also deletes the database volume. The image binds 0.0.0.0 inside the container; that is reachable only through the published port mapping, so do not change the host side of the mapping to `0.0.0.0` or a public interface.

## 6. Kubernetes

The manifests in `deploy/k8s/` create a `corvus` namespace, a 1 GiB ReadWriteOnce PVC, one daemon replica, a ClusterIP Service, and a default-deny NetworkPolicy. The deployment runs with a read-only root filesystem, no capabilities, a RuntimeDefault seccomp profile and an init container that seeds the extension from the image into the volume.

```bash
docker build -t corvus:1.1.0 .
kind load docker-image corvus:1.1.0   # or push to your registry
kubectl apply -k deploy/k8s
kubectl -n corvus rollout status deployment/corvus
kubectl -n corvus port-forward svc/corvus 7330:7330 7331:7331
curl -fsS http://127.0.0.1:7331/health
```

The Service is ClusterIP on purpose. A browser extension cannot reach a ClusterIP directly; desktop clients should use the port-forward, or an authenticated ingress or proxy that the operator provides. Same-namespace clients need no extra label; clients in other namespaces need both the `corvus.io/allow-client: "true"` namespace label and the `corvus.io/client: "true"` pod label. See [deploy/k8s/README.md](../deploy/k8s/README.md) for the exact policy, storage and upgrade notes.

## 7. Ports, exposure and the no-authentication boundary

| Port | Protocol | Purpose |
|---|---|---|
| 7330 | WebSocket | Extension transport |
| 7331 | HTTP | MCP streamable HTTP at `/mcp`, `/health`, `/metrics` |

Corvus has **no authentication**. The security boundary is the operating-system account plus loopback binding. Never expose 7330 or 7331 on a public interface, public LoadBalancer or public Ingress. If a remote browser must reach the daemon, put a trusted tunnel or an authenticating proxy in front of it and use a `wss://` URL in the extension options page; do not send the socket over an untrusted network. `/health`, `/metrics` and the MCP endpoint share the same trust boundary.

## 8. Backup, restore and retention

The recording database is ordinary SQLite in WAL mode. For a consistent copy, stop the daemon or use SQLite's online backup, then copy `data/corvus.db` (and any `-wal`/`-shm` files when stopped). Logs are not required for restore.

The daemon marks previously active sessions `abandoned` at startup and prunes sessions older than 30 days. Treat the database as sensitive: it contains tool arguments and results, including page text. Kubernetes users can snapshot the PVC; Docker users can back up the named volume.

Restore by stopping the daemon, replacing `data/corvus.db` with the backup, and starting it again. A schema newer than version 1 is refused rather than downgraded.

## 9. Operations and troubleshooting

| Symptom | Check | Action |
|---|---|---|
| Service exits immediately | `journalctl -u corvus` or the Windows service log | Fix the reported path, port or permission error |
| `/health` unreachable | `ss -ltnp` or `netstat -ano` | Confirm 7331 is listening and the firewall allows loopback |
| No browsers in `corvus doctor` | Extension popup and options page | Load the extension and confirm the daemon URL |
| Port already in use | Listener table | Stop the other daemon; do not run service and stdio mode on the same ports |
| Logs missing | Root `logs/` directory | Check `--log-file`/`--no-log-file`; the file is `corvusd.log.YYYY-MM-DD` |
| Disk grows | `data/` and `logs/` | Retention is 30 days; archive or rotate logs as needed |
| Docker health fails | `docker logs corvus` | Confirm the `/data` volume is writable by UID 10001 |
| Kubernetes pod not ready | `kubectl -n corvus describe pod` | Check the PVC binding, init container and NetworkPolicy |

Run `corvus doctor` for version, ports, database, uptime, tool counts and connected browsers. Run `corvusd doctor --home <DIR>` when the service is not running.
