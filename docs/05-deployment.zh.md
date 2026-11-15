[English](05-deployment.md) | [中文](05-deployment.zh.md)

# 部署 Corvus

Corvus 由一个本地守护进程、一个命令行和一个浏览器扩展组成。本指南覆盖受支持的部署形态：Windows 服务、Linux systemd 服务、Docker 容器与 Kubernetes 清单。后端的全部文件都位于同一个根目录下，由 `--home` 或环境变量 `CORVUS_HOME` 指定。

## 1. 部署模型与单根布局

守护进程拥有一个根目录。默认根目录取决于启动方式：

| 场景 | 根目录 |
|---|---|
| Windows 服务（安装器默认） | `%ProgramData%\Corvus` |
| Windows 前台或 CLI | `%LOCALAPPDATA%\Corvus`，回退到 `%USERPROFILE%\.corvus` |
| Linux 软件包（systemd） | `/opt/corvus` |
| Linux 前台 | `$XDG_DATA_HOME/corvus`，回退到 `$HOME/.local/share/corvus` |
| macOS 前台 | `$HOME/Library/Application Support/Corvus` |
| Docker 与 Kubernetes | `/data` |

根目录下的布局固定：

| 路径 | 内容 |
|---|---|
| `bin/` | `corvusd` 与 `corvus`（Windows 为 `.exe`） |
| `data/corvus.db` | SQLite 录制库；打开期间还有 `-wal` 与 `-shm` |
| `logs/` | 文件日志；滚动文件名按 UTC 日期为 `corvusd.log.YYYY-MM-DD` |
| `config/` | 预留配置目录；本版本不加载配置文件 |
| `extension/` | 已解包的 Manifest V3 扩展 |

在 `corvusd serve`、`corvusd doctor` 与 `corvus` 上使用 `--home <DIR>` 固定根目录；未给出该参数时，`CORVUS_HOME` 指定同一个根。传入 `--home` 后，守护进程启动时创建 `data/`、`logs/` 与 `config/`，不创建 `bin/` 与 `extension/`，它们由打包与安装流程负责。

数据库路径优先级：

1. `--db <PATH>`。
2. 当 `--home` 或 `CORVUS_HOME` 选定根目录时，使用 `<root>/data/corvus.db`。
3. `CORVUS_DB_PATH`。
4. 平台默认根目录。

相对根目录与相对 `--log-file` 分别按进程工作目录与根目录解析；服务定义中请使用绝对路径。

文件日志默认写入 `logs/`。`--log-file <PATH>` 覆盖路径，`--no-log-file` 关闭文件日志，内存日志环与 stderr 输出仍然保留。滚动文件始终带 UTC 日期后缀，例如 `corvusd.log.2026-11-15`，而不是裸的 `corvusd.log`。事件结构与级别规则见 [08-logging.zh.md](08-logging.zh.md)。

## 2. Linux systemd 安装

Linux 发布包仅面向 x86_64。先解压到一个目录，再安装：

```bash
mkdir -p corvus-1.3.0-linux-x86_64
tar -xzf corvus-1.3.0-linux-x86_64.tar.gz -C corvus-1.3.0-linux-x86_64
cd corvus-1.3.0-linux-x86_64
sha256sum -c SHA256SUMS
sudo ./install-linux.sh
```

安装器需要 root、`systemctl` 与正在运行的 systemd。它创建系统用户与用户组 `corvus`，把二进制安装到 `/opt/corvus/bin`，把扩展复制到 `/opt/corvus/extension`，为服务用户创建 `data`、`logs` 与 `config`，写入 `/etc/systemd/system/corvus.service`，启用并重启单元，然后等待 `http://127.0.0.1:7331/health`。

环境变量覆盖：

```bash
sudo CORVUS_HOME=/var/lib/corvus CORVUS_USER=corvus \
  CORVUS_WS_PORT=7330 CORVUS_HTTP_PORT=7331 ./install-linux.sh
```

安装器强制执行根目录安全约束：根必须为绝对路径且是专用目录，拒绝 `/`、`/usr`、`/etc`、`/home` 等常见系统目录，拒绝包含 `..` 或非法字符的路径，也拒绝位于符号链接祖先之下的路径。可在不改动主机的前提下验证：

```bash
./install-linux.sh --self-test
```

单元以无特权服务用户运行，系统视图只读，设置 `ProtectHome=true`，丢弃全部 capability，使用 SIGTERM 停止，并把 `--log-file` 指向根目录下的日志。`journalctl -u corvus` 可查看服务输出。

## 3. Linux 升级与卸载

升级时解压新包并运行同一个安装器。它会停止服务、替换二进制与扩展、重写单元并重启；`data/corvus.db` 与 `logs/` 保留。数据库 schema 为版本 1，1.0.0 的数据库可原地打开。

卸载默认保留数据与日志：

```bash
sudo ./uninstall-linux.sh
```

使用 `--purge` 删除整个根目录与服务用户。脚本在做任何改动前拒绝不安全的根，并支持 `--self-test`。

## 4. 包内容与校验

Linux 归档包含 `VERSION`、`SHA256SUMS`、`bin/corvusd`、`bin/corvus`、完整的 `extension/` 目录树、`install-linux.sh`、`uninstall-linux.sh` 与 `systemd/corvus.service`。`SHA256SUMS` 覆盖 `VERSION`、两个二进制、两个脚本、unit 与每个扩展文件。

安装前必须校验：

```bash
sha256sum -c SHA256SUMS
```

Windows 发布包是 ZIP，包含 `corvusd.exe`、`corvus.exe`、`nssm.exe`、扩展目录树、`VERSION`、`SHA256SUMS`、安装与卸载脚本以及 nssm 说明。Windows 流程见 [04-install.zh.md](04-install.zh.md)。

## 5. Docker

在仓库根目录构建镜像：

```bash
docker build -t corvus:1.3.0 .
```

镜像以 UID/GID 10001 运行 `corvusd serve --bind 0.0.0.0 --home /data`，运行数据存放在 `/data` 卷，暴露 7330（WebSocket）与 7331（MCP HTTP）。未打包扩展位于 `/opt/corvus/extension`，不在 `/data`；请用 `docker cp` 复制出来，或在宿主机上使用发布 ZIP 中的扩展。只发布到宿主机回环：

```bash
docker volume create corvus-data
docker run -d --name corvus \
  -p 127.0.0.1:7330:7330 -p 127.0.0.1:7331:7331 \
  -v corvus-data:/data corvus:1.3.0
curl -fsS http://127.0.0.1:7331/health
```

Compose 使用同一镜像、命名卷、只读根文件系统、`/tmp` tmpfs、丢弃全部 capability，并启用 `no-new-privileges`：

```bash
docker compose up -d --build
docker compose logs -f corvus
docker compose down
```

`docker compose down -v` 还会删除数据库卷。镜像在容器内绑定 0.0.0.0；该地址只能通过发布的端口映射访问，因此不要把宿主侧映射改成 `0.0.0.0` 或公网接口。

## 6. Kubernetes

`deploy/k8s/` 中的清单创建 `corvus` 命名空间、1 GiB ReadWriteOnce PVC、一个守护进程副本、ClusterIP Service 与默认拒绝的 NetworkPolicy。部署使用只读根文件系统、无 capability、RuntimeDefault seccomp，并通过 init 容器把镜像内的扩展播种到卷。

```bash
docker build -t corvus:1.3.0 .
kind load docker-image corvus:1.3.0   # 或推送到你的镜像仓库
kubectl apply -k deploy/k8s
kubectl -n corvus rollout status deployment/corvus
kubectl -n corvus port-forward svc/corvus 7330:7330 7331:7331
curl -fsS http://127.0.0.1:7331/health
```

Service 有意保持 ClusterIP。浏览器扩展无法直接访问 ClusterIP；桌面客户端应使用 port-forward，或由运维提供带认证的 ingress 或代理。同命名空间客户端不需要额外标签；其他命名空间客户端需要同时具备 `corvus.io/allow-client: "true"` 命名空间标签与 `corvus.io/client: "true"` Pod 标签。策略、存储与升级细节见 [deploy/k8s/README.md](../deploy/k8s/README.md)。

## 7. 端口、暴露面与无认证边界

| 端口 | 协议 | 用途 |
|---|---|---|
| 7330 | WebSocket | 扩展传输 |
| 7331 | HTTP | `/mcp` 的 MCP streamable HTTP、`/health`、`/metrics` |

Corvus **没有认证**。安全边界是操作系统账号加回环绑定。绝不能把 7330 或 7331 暴露到公网接口、公网 LoadBalancer 或公网 Ingress。若远程浏览器必须访问守护进程，应在其前面部署可信隧道或带认证的代理，并在扩展选项页使用 `wss://` URL；不要把套接字发往不可信网络。`/health`、`/metrics` 与 MCP 端点处于同一信任边界。

### 容器与 Kubernetes 中的 CDP

CDP 仅限宿主机回环。守护进程只接受主机为 `127.0.0.1`、`localhost` 或 `[::1]` 的浏览器级端点；容器或 Pod 有自己的回环，默认无法访问宿主机上运行的浏览器。发布镜像不含浏览器，也不会主动连接 CDP。把浏览器作为 sidecar 运行不是开箱即用的受支持路径：它需要共享网络命名空间、匹配的浏览器家族与可写 profile，且不在本文测试范围内。在 Docker 或 Kubernetes 内请把通道保持为 `extension`（或以 `--no-cdp` 启动），通过扩展驱动浏览器。

## 8. 备份、恢复与保留期

录制库是 WAL 模式的普通 SQLite。要获得一致副本，请停止守护进程或使用 SQLite 在线备份，然后复制 `data/corvus.db`（停止时连同 `-wal`/`-shm`）。日志不是恢复所需。

守护进程启动时把上一进程遗留的活动会话标记为 `abandoned`，并清理早于 30 天的会话。数据库包含工具参数与结果（包括页面文本），应视为敏感数据。Kubernetes 用户可对 PVC 做快照；Docker 用户可备份命名卷。

恢复时停止守护进程，用备份替换 `data/corvus.db`，再启动。高于版本 1 的 schema 会被拒绝，而不是降级。

## 9. 运维与故障排查

| 现象 | 检查 | 处理 |
|---|---|---|
| 服务立即退出 | `journalctl -u corvus` 或 Windows 服务日志 | 修复报告中的路径、端口或权限错误 |
| `/health` 不可达 | `ss -ltnp` 或 `netstat -ano` | 确认 7331 正在监听，防火墙允许回环 |
| `corvus doctor` 没有浏览器 | 扩展弹窗与选项页 | 加载扩展并确认守护进程 URL |
| 端口被占用 | 监听表 | 停止其他守护进程；不要在同一端口同时运行服务与 stdio 模式 |
| 日志缺失 | 根目录 `logs/` | 检查 `--log-file`/`--no-log-file`；文件名为 `corvusd.log.YYYY-MM-DD` |
| 磁盘增长 | `data/` 与 `logs/` | 保留期为 30 天；按需归档或轮转日志 |
| Docker 健康检查失败 | `docker logs corvus` | 确认 `/data` 卷对 UID 10001 可写 |
| Kubernetes Pod 未就绪 | `kubectl -n corvus describe pod` | 检查 PVC 绑定、init 容器与 NetworkPolicy |

`corvus doctor` 报告版本、端口、数据库、运行时长、工具计数与已连接浏览器。服务未运行时可用 `corvusd doctor --home <DIR>`。
