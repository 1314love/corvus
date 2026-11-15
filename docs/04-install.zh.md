[English](04-install.md) | [中文](04-install.zh.md)

# 安装、升级与卸载

Corvus 由一个本地守护进程（`corvusd`）、一个命令行客户端（`corvus`）和一个浏览器扩展组成。所有后端文件都位于同一个根目录下，由 `--home` 或 `CORVUS_HOME` 选择：二进制、数据库、日志、配置与已解包的扩展。本页覆盖 Windows 上用 nssm 安装服务，以及 Linux 上用 systemd 从发布包安装；Docker、Kubernetes、备份与完整运维清单见 [05-deployment.zh.md](05-deployment.zh.md)。守护进程默认只绑定 `127.0.0.1`。

## 默认布局

| 平台 | 默认根目录 | 启动方式 |
|---|---|---|
| Windows 服务 | `%ProgramData%\Corvus` | `install-windows.ps1` 通过 nssm 注册 `Corvus` 服务 |
| Windows 前台 | `%LOCALAPPDATA%\Corvus` | `corvusd serve` 或 `corvus serve` |
| Linux 安装包 | `/opt/corvus` | `install-linux.sh` 写入 `/etc/systemd/system/corvus.service` |
| Linux 前台 | `$XDG_DATA_HOME/corvus` 或 `~/.local/share/corvus` | `corvusd serve` |
| Docker / Kubernetes | `/data` | 容器入口或 Kubernetes 清单 |
| macOS 源码运行 | `~/Library/Application Support/Corvus` | `corvusd serve`；1.6.0 不提供安装包 |

一个根目录容纳全部后端文件：

    <root>/
      bin/            corvusd、corvus，Windows 上还有 nssm.exe
      data/corvus.db  SQLite 录制库
      logs/           每日文件日志与服务 stdout/stderr 文件
      config/         预留给文件配置
      extension/      已解包的扩展

| 端口 | 用途 | 默认值 |
|---|---|---|
| 7330 | 扩展使用的 WebSocket 传输 | `ws://127.0.0.1:7330` |
| 7331 | MCP streamable HTTP、`/health` 与 `/metrics` | `http://127.0.0.1:7331/mcp` |

数据库默认为 `<root>/data/corvus.db`，除非给出 `--db <path>`。已配置的 `--home` 或 `CORVUS_HOME` 优先于 `CORVUS_DB_PATH`；没有根目录覆盖时仍会读取 `CORVUS_DB_PATH`。文件日志默认写入 `<root>/logs/corvusd.log`，按 UTC 每日轮转为 `corvusd.log.YYYY-MM-DD`；`--log-file` 覆盖路径，`--no-log-file` 关闭文件输出。

## 前置条件

### Windows

- Windows 10 或 Windows 11，64 位。
- PowerShell 5.1 或 PowerShell 7；安装与卸载需要以管理员身份运行。
- 在 Windows 上构建的发布 ZIP，包含 `corvusd.exe`、`corvus.exe`、扩展、安装与卸载脚本，以及随包附带的 `nssm.exe`。
- nssm 2.24 或更高版本。发布包把它放在 `bin\nssm.exe`；也可以用 `-NssmPath` 选择系统副本。
- 用于加载扩展的 Chrome、Edge 或 Brave（Chromium 120 或更高）。

### Linux

- Linux x86_64，带 glibc 与 systemd。
- 通过 `sudo` 取得 root 权限；`install-linux.sh` 需要 systemd，并在 `/etc/systemd/system` 下写入 unit。
- 建议安装 `curl` 供安装脚本做健康检查与后续验证；缺少 `curl` 时安装仍会继续并给出警告。
- `corvus-<version>-linux-x86_64.tar.gz` 发布包及其 `SHA256SUMS`。

从源码构建需要 Rust 1.90 或更高版本。本版本的 macOS 仅支持从源码运行。

## 构建发布包

如果已有准备好的、可信的发布包，可以跳过本节。在仓库根目录执行：

```bash
# Windows ZIP：dist/corvus-<version>-windows-x64.zip
bash scripts/package.sh

# Linux tar 包：dist/corvus-<version>-linux-x86_64.tar.gz
bash scripts/package-linux.sh
```

Windows 发布包默认使用 `--offline` 与本机 cargo 缓存构建；设置 `CORVUS_CARGO_OFFLINE=0` 可让 CI 使用网络。Linux 发布包使用正常的 Cargo 网络访问构建（`cargo build --release --locked`）。两个脚本都会校验暂存包的条目与校验和。新机器需要先在有网络时运行一次 `cargo fetch` 填充缓存。Windows 安装脚本要求 `.exe` 文件名，因此请在 Windows 上构建 Windows 发布包。Linux 发布包仅支持 x86_64。

## 在 Windows 上安装

发布 ZIP 的根目录包含 `install-windows.ps1` 与 `uninstall-windows.ps1`。在解压目录中打开管理员 PowerShell 并执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install-windows.ps1
```

在仓库检出中，请先用 `bash scripts/package.sh` 构建发布包，再执行 `.\scripts\install-windows.ps1`。

使用其他根目录与端口时：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install-windows.ps1 `
  -HomeDir "D:\Corvus" -WsPort 7330 -HttpPort 7331
```

在仓库检出中，请把两条命令里的 `.\install-windows.ps1` 换成 `.\scripts\install-windows.ps1`。

安装脚本会检查管理员权限与根目录，在发布二进制缺失时拒绝执行，停止已有的 `Corvus` 服务，复制 `bin/` 与 `extension/`，停止并删除旧服务注册，然后注册 nssm 服务。服务命令为：

```text
corvusd.exe serve --home "<HomeDir>" --ws-port <WsPort> --http-port <HttpPort>
```

脚本是幂等的：重复执行同一条命令会原地升级并保留数据库。如果服务在 30 秒内没有进入 `Running`，脚本会打印最近的日志并以非零状态退出。

`-InstallDir`、`-DataDir` 与 `-ExtensionDir` 仍作为已弃用的兼容别名接受，但必须解析到 `-HomeDir` 的子目录。新安装只需传 `-HomeDir`。

### 从 1.0.0 迁移

1.0.0 把文件放在 `%USERPROFILE%\.corvus`。安装脚本会把该目录移动到 `<HomeDir>\migration\userprofile-corvus`；如果 `<HomeDir>\data\corvus.db` 尚不存在，则把 `corvus.db`、`corvus.db-wal` 与 `corvus.db-shm` 复制到新的数据目录。传 `-KeepLegacyData` 可保留旧目录。需要保留录制历史时，升级前先备份旧数据库。

### 加载扩展

1. 打开 `chrome://extensions` 或 `edge://extensions`。
2. 打开开发者模式。
3. 选择“加载已解压的扩展程序”，目录选 `<HomeDir>\extension`。
4. 打开扩展弹窗。绿点表示已连接守护进程；弹窗显示浏览器 ID、扩展版本与能力。

扩展没有配对步骤。可选设置页用于修改守护进程 WebSocket URL 与可选 CDP 端点，见[扩展设置页](#扩展设置页)。发布包含有预构建的 `extension/` 目录。源码检出中请运行 `pnpm --dir extension install && pnpm --dir extension build`，然后加载 `extension/build/extension`；构建会执行三次 Vite 打包并校验 MV3 manifest。

## 在 Linux 上安装

下载或复制 tar 包，解压到一个目录，然后校验并安装：

```bash
mkdir -p corvus-1.6.0-linux-x86_64
tar -xzf corvus-1.6.0-linux-x86_64.tar.gz -C corvus-1.6.0-linux-x86_64
cd corvus-1.6.0-linux-x86_64
sha256sum -c SHA256SUMS
sudo ./install-linux.sh
```

安装脚本创建名为 `corvus` 的系统用户（无 home、`nologin` shell），以 root 身份安装 `bin/` 与 `extension/`，把 `data/`、`logs/`、`config/` 交给服务用户写入，写入 systemd unit，启用并重启服务，然后在最多 30 秒内轮询 `http://127.0.0.1:<HttpPort>/health`。

用环境变量覆盖根目录、服务用户与端口：

```bash
sudo CORVUS_HOME=/var/lib/corvus CORVUS_USER=corvus \
  CORVUS_WS_PORT=7330 CORVUS_HTTP_PORT=7331 ./install-linux.sh
```

`CORVUS_HOME` 必须是系统目录和用户目录之外的专用绝对路径。服务 unit 位于 `/etc/systemd/system/corvus.service`；日志同时写入 `<root>/logs` 下的每日文件与 journal。

用 `systemctl status corvus.service`、`curl -fsS http://127.0.0.1:7331/health` 和 `sudo -u corvus <root>/bin/corvus doctor` 验证安装。

## 扩展设置页

扩展默认连接 `ws://127.0.0.1:7330`。使用其他守护进程或 `wss://` 隧道时，打开扩展设置页（或弹窗中的 Change 链接），填写完整 WebSocket URL 并保存。URL 保存在 `corvus.daemonUrl`；Reset 恢复默认值。扩展重新加载或升级后，请刷新已打开的标签页，让内容脚本重新连接。

`wss://` 端点需要在 Corvus 之外终止 TLS，例如反向代理。MCP 与扩展 WebSocket 没有认证，可选的 REST 认证也只覆盖 `/api/v1`；把 7330 或 7331 暴露到回环之外的隧道仍等同于暴露完整的浏览器控制能力。见 [05-deployment.zh.md](05-deployment.zh.md#7-端口暴露面与认证边界)。

### CDP 端点（可选）

默认零配置路径只使用扩展。要增加浏览器级 CDP，请用远程调试端口与独立 profile 启动 Chromium 浏览器：

```bash
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile
curl -s http://127.0.0.1:9222/json/version
```

使用响应中的 `webSocketDebuggerUrl`，不要使用 HTTP 地址。然后可把扩展设置页的 CDP 端点设为 `auto`（探测 9222、9223 与 9333）或显式的 `http://127.0.0.1:9222`，也可用以下参数启动守护进程：

```bash
corvusd serve --home /opt/corvus --cdp-endpoint "ws://127.0.0.1:9222/devtools/browser/<id>"
corvusd serve --home /opt/corvus --channel extension   # 忽略 CDP
corvusd serve --home /opt/corvus --no-cdp              # 移除 CDP 路径
```

`--channel` 接受 `auto`、`cdp` 或 `extension`；`CORVUS_CHANNEL` 设置同一值。CDP 端点仅限回环且没有认证。绝不要把 9222、9223 或 9333 暴露到回环之外。完整指南见 [07-cdp.zh.md](07-cdp.zh.md)。

## 验证安装

Windows：

```powershell
Get-Service Corvus | Format-Table Name, Status, StartType
& "$env:ProgramData\Corvus\bin\corvus.exe" doctor
Invoke-RestMethod http://127.0.0.1:7331/health
Invoke-WebRequest http://127.0.0.1:7331/metrics | Select-Object -Expand Content
Invoke-RestMethod http://127.0.0.1:7331/api/v1/health
Invoke-WebRequest http://127.0.0.1:7331/api-docs/openapi.json | Select-Object -Expand Content
```

Linux：

```bash
systemctl status corvus.service
/opt/corvus/bin/corvus doctor
curl -fsS http://127.0.0.1:7331/health
curl -fsS http://127.0.0.1:7331/metrics
curl -fsS http://127.0.0.1:7331/api/v1/health
curl -fsS http://127.0.0.1:7331/api-docs/openapi.json
```

`doctor` 会报告版本、WebSocket 与 MCP HTTP 端口是否可达、运行时长、工具调用计数、数据库路径、schema 版本、会话与步骤数量，以及每个已连接浏览器一行。`/health` 返回 `status: "ok"`、绑定地址与录制信息。守护进程可达但没有浏览器行，说明扩展尚未加载，或指向了另一个端口。预期使用 CDP 时，请通过 MCP 客户端不带参数调用 `browser_channel` 与 `cdp_status`；状态会列出当前通道、端点与浏览器，以及 `last_error` 或 `browser_mismatch`。

## 连接 MCP 客户端

### HTTP 传输

Windows 服务保持安装时使用 HTTP。用已安装的二进制生成配置：

```powershell
& "$env:ProgramData\Corvus\bin\corvus.exe" config --client cursor --transport http
```

把输出的 `mcpServers` 对象粘贴到客户端配置。它指向 `http://127.0.0.1:7331/mcp`；保持服务运行。`--client` 接受 `claude`、`cursor`、`dsh`、`cline` 或 `generic`，并在 stderr 追加位置提示。stdout 的 JSON 对所有客户端使用同一套 schema，因此重定向 stdout 得到的就是有效配置文件。

### stdio 传输

客户端自行管理进程时使用 stdio。生成的条目会启动 `corvus serve --stdio`。stdio 客户端需要与服务相同的回环端口，因此必须先停止服务，否则新进程会因端口被占用而失败：

```powershell
Stop-Service Corvus
& "$env:ProgramData\Corvus\bin\corvus.exe" config --client generic --transport stdio
```

要在生成的命令中固定自定义根目录，加 `--home <dir>`；stdio 参数变为 `serve --stdio --home <dir>`。stdio 客户端使用期间保持服务停止；切回 HTTP 时运行 `Start-Service Corvus`。

### REST API 与可选认证

REST API 与 MCP 共用 7331 端口，在 `/api/v1` 提供 health、browsers、sessions、tools 与 `POST /api/v1/tools/call`，并在 `/api/v1/auth` 下提供登录、刷新、登出与账户路由。OpenAPI 文档位于 `/api-docs/openapi.json`，Swagger UI 位于 `/docs`。

认证默认关闭，只适合绑定回环地址。绑定其他地址前必须启用本地模式：

```bash
export CORVUS_AUTH_SECRET="$(openssl rand -hex 32)"   # 至少 32 字节
export CORVUS_AUTH_PASSWORD='replace-with-a-strong-password'
corvusd serve --home /opt/corvus --bind 0.0.0.0 --auth local \
  --auth-username admin --auth-password "$CORVUS_AUTH_PASSWORD"
```

同样的值也可来自 `CORVUS_AUTH`、`CORVUS_AUTH_SECRET`、`CORVUS_AUTH_USERNAME` 与 `CORVUS_AUTH_PASSWORD`。绑定非回环地址而认证关闭时，启动会以“authentication must be enabled when binding to a non-loopback address”失败；本地模式下密钥短于 32 字节同样失败。系统没有默认凭据：首次启用本地认证时必须提供引导用户名与密码，且二者都不能为空。

登录后把访问令牌作为 bearer 令牌发送：

```bash
curl -fsS http://127.0.0.1:7331/api/v1/auth/login \
  -H 'content-type: application/json' \
  -d '{"username":"admin","password":"<引导密码>"}'

curl -fsS http://127.0.0.1:7331/api/v1/tools \
  -H "authorization: Bearer <access_token>"
```

访问令牌有效期 900 秒；刷新令牌有效期 30 天，以哈希保存，每次使用都会轮换，登出即撤销。账号与刷新令牌只保存在内存中：重启守护进程会重置会话与所有经 API 做的修改，不过引导账号会按启动参数重新创建。1.6.0 没有用户管理界面，也没有持久化账号存储。

**认证范围限制：**bearer 守卫只覆盖 `/api/v1`。在其中，`/api/v1/health`、`/api/v1/auth/login` 与 `/api/v1/auth/refresh` 豁免。`/mcp`、旧版 `/health` 与 `/metrics`、`/api-docs/openapi.json` 与 `/docs` 都在守卫之外，即使开启本地认证也保持无认证；扩展 WebSocket 同样如此。启用 REST 认证并不能让非回环的 MCP 端点变安全；必须使用保护所有端口的可信代理。

## 五分钟上手流程

这是 Windows 验收流程。已有准备好的 ZIP 时可在五分钟内完成；从源码做一次全新发布构建会额外增加编译时间。

| 时间 | 步骤 | 命令或操作 | 成功信号 |
|---|---|---|---|
| 0:00 | 检查前置条件 | Windows 10/11、管理员 PowerShell、Chrome、Edge 或 Brave（Chromium 120+） | Windows 与浏览器版本受支持 |
| 0:30 | 构建或定位发布包 | `bash scripts/package.sh`（已有 ZIP 则跳过） | `dist/corvusd.exe` 与 `dist/corvus.exe` 存在 |
| 1:30 | 安装服务 | `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1` | 安装脚本以服务运行结束 |
| 2:30 | 加载扩展 | 加载已解压的扩展：`%ProgramData%\Corvus\extension` | 弹窗绿点亮起 |
| 3:30 | 连接客户端 | `corvus.exe config --client cursor --transport http` 并粘贴配置 | 客户端列出 Corvus 服务 |
| 4:30 | 打开第一页 | 调用 `browser_navigate`，参数 `{"url":"https://example.com"}`，再调用 `page_snapshot` | 快照返回页面标题与节点 |

## 升级

需要保留录制历史时，先备份 `<root>/data/corvus.db`。然后用同样的方式安装新发布包：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
& "$env:ProgramData\Corvus\bin\corvus.exe" doctor
```

```bash
sudo ./install-linux.sh
systemctl status corvus.service
/opt/corvus/bin/corvus doctor
```

安装脚本会停止服务、替换 `bin/` 与 `extension/`、重写服务参数，然后重新启动服务。数据库、日志与配置不会被删除。自 1.4.0 起，在既有数据库上启动时，录制模块会先用 `VACUUM INTO` 在数据库旁写一份备份，再应用 schema 版本 1 基线；备份失败会中止启动，因此仍建议自行保留副本。扩展文件有变化时，重新加载扩展与已打开的标签页。Docker 与 Kubernetes 升级见 [05-deployment.zh.md](05-deployment.zh.md)。

## 卸载

Windows 上在管理员 PowerShell 中执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall-windows.ps1
```

脚本会停止并删除 `Corvus` 服务，移除二进制与扩展，默认保留数据库与日志。服务删除后加 `-PurgeData` 可删除整个根目录。安装时使用了自定义值时传 `-HomeDir` 与 `-ServiceName`；`-SelfTest` 只检查路径守卫，不做任何改动。

Linux：

```bash
sudo ./uninstall-linux.sh
# 同时删除数据、日志与服务用户
sudo ./uninstall-linux.sh --purge
```

默认卸载保留 `<root>/data`、`<root>/logs` 与 `<root>/config`。`--purge` 删除整个根目录以及服务用户和组。`--self-test` 只检查路径守卫。

## 服务控制与故障排查

Windows：

```powershell
Get-Service Corvus
Restart-Service Corvus
nssm restart Corvus
```

Linux：

```bash
systemctl status corvus.service
systemctl restart corvus.service
journalctl -u corvus.service -n 100 --no-pager
```

| 现象 | 检查 | 处理 |
|---|---|---|
| 提示 `nssm.exe not found` | `nssm version` | 使用随包的 `bin\nssm.exe`、把 nssm 放入 `PATH`，或传 `-NssmPath` |
| Windows 服务无法启动 | `<root>\logs\corvusd.err.log` 与 `<root>\logs\corvusd.log.YYYY-MM-DD` | 修正日志中的路径、端口或权限问题后重跑安装脚本 |
| Linux 服务无法启动 | `systemctl status corvus.service` 与 `journalctl -u corvus.service` | 修正 unit、路径或端口问题后重跑安装脚本 |
| 7330 或 7331 被占用 | `netstat -ano | findstr :7330` 或 `ss -ltnp | grep 733` | 停止另一个守护进程；不要同时运行服务与 stdio 模式 |
| `doctor` 没有浏览器 | 弹窗状态、服务状态、设置页 URL | 加载或重新加载扩展；确认 WebSocket URL 与 7330 端口 |
| 客户端无法连接 HTTP | `Invoke-RestMethod http://127.0.0.1:7331/health` 或 `curl -fsS http://127.0.0.1:7331/health` | 启动服务；使用 `127.0.0.1`，不要用局域网地址 |
| 离线打包失败 | `bash scripts/dev-setup.sh` | 先在有网络时填充 cargo 缓存，或使用准备好的发布包 |
| Windows 脚本被执行策略拦截 | 错误文本会指出 ExecutionPolicy | 按上文使用 `-ExecutionPolicy Bypass` |
| 提示 `missing ...corvusd.exe` | 发布包内容 | 先运行 `bash scripts/package.sh` |
| 扩展重新加载后，`page_text` 或 `page_snapshot` 返回 TIMEOUT | `tab_list` 仍能列出标签页；扩展卡片为绿点 | 重新加载标签页或重新打开其 URL。标签页会继续运行旧内容脚本，而 `tab_list` 不需要内容脚本 |
| `browser_channel` 显示 `cdp` 但未连接 | `cdp_status.connected` 与 `last_error` | 浏览器或端点已变化。用当前 `webSocketDebuggerUrl` 重启守护进程，或让扩展重新发现端点 |
| CDP 端点被拒绝 | 端点 scheme、主机与路径 | 使用 `/json/version` 返回的回环 `ws://` 或 `wss://` 浏览器地址；HTTP 发现地址、局域网主机与路径都会被拒绝 |
| `/api/v1` 返回 401 `MISSING_TOKEN` | `--auth` 模式与请求头 | 登录后发送 `authorization: Bearer <access_token>`；登录、刷新与 `/api/v1/health` 保持豁免 |
| 守护进程拒绝启动并提示必须启用认证 | `--bind` 地址与 `--auth`/`CORVUS_AUTH` | 绑定回环，或启用 `local` 认证并提供至少 32 字节的密钥 |

## 安全说明

守护进程默认绑定 `127.0.0.1`。MCP、扩展 WebSocket 与 CDP 都没有认证：任何能访问这些端口的本地进程都可以驱动已连接的浏览器。REST `/api/v1` 可启用本地认证，但守卫不包含 `/api/v1/health`、`/api/v1/auth/login`、`/api/v1/auth/refresh`，也不覆盖 `/mcp`、`/health`、`/metrics`、`/api-docs/openapi.json`、`/docs` 与 WebSocket。仅靠 REST 认证不能让非回环守护进程变安全；必须在所有端口前放置可信代理，或把它们留在回环。配置的 CDP 端点具有同样的信任模型，可读取 cookie、注入脚本并控制该浏览器实例中的所有 profile；9222、9223 与 9333 也必须留在回环。Windows 安装脚本按 nssm 默认账户 LocalSystem 注册服务；安全策略要求专用身份时，请用 nssm 修改服务账户。根目录包含录制的命令、参数与结果，应把 `<root>/data/corvus.db` 与日志视为敏感数据，把机器交给他人前先删除。Linux 安装脚本创建专用 `corvus` 服务用户，并把 `data/`、`logs/`、`config/` 限制给该用户。
