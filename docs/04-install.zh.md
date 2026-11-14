[English](04-install.md) | [中文](04-install.zh.md)

# 安装与升级

Corvus 由一个本地守护进程（`corvusd`）、一个命令行客户端（`corvus`）和一个浏览器扩展组成。本页说明如何用
nssm 把守护进程安装为 Windows 服务、加载扩展、连接 MCP 客户端，并覆盖升级与卸载。服务只绑定
`127.0.0.1`。

## 默认布局

| 项目 | 默认值 |
|---|---|
| 服务名 | `Corvus` |
| 二进制 | `%USERPROFILE%\.corvus\bin`（`corvusd.exe`、`corvus.exe`） |
| 数据目录 | `%USERPROFILE%\.corvus` |
| 数据库 | `%USERPROFILE%\.corvus\corvus.db` |
| 扩展 | `%USERPROFILE%\.corvus\extension` |
| 日志 | `%USERPROFILE%\.corvus\corvusd.log`、`%USERPROFILE%\.corvus\corvusd.err.log` |
| WebSocket | `ws://127.0.0.1:7330` |
| MCP HTTP | `http://127.0.0.1:7331/mcp` |

## 前置条件

- Windows 10 或 Windows 11，64 位。
- PowerShell 5.1 或 PowerShell 7；安装与卸载需要以管理员身份运行。
- nssm 2.24 或更高版本。安装脚本会依次查找 `PATH`、安装目录、Chocolatey bin 目录、
  `C:\Program Files\nssm`，也接受 `-NssmPath`。
- `dist/` 中已有发布包，包含 `corvusd.exe`、`corvus.exe` 和 `extension/`。
- 用于加载扩展的 Chrome 或 Edge。
- 只有从源码构建发布包时才需要 Rust 工具链。

## 构建发布包

如果已有准备好的、可信的 `dist/` 发布包，可以跳过本节。在 Windows 的 MSYS bash 或 Git Bash 中，从仓库根目录执行：

```bash
bash scripts/dev-setup.sh
bash scripts/package.sh
(cd dist && sha256sum -c SHA256SUMS)
```

`scripts/package.sh` 会执行离线发布构建，然后写入 `dist/corvusd.exe`、`dist/corvus.exe`、
`dist/extension/`、`dist/VERSION` 和 `dist/SHA256SUMS`。构建脚本使用 `--offline`，依赖本机 cargo 缓存。
新机器需要先在有网络时运行一次 `cargo fetch` 填充缓存；CI 构建正常使用网络。Windows 安装脚本要求
`.exe` 文件名，因此请在 Windows 上构建 Windows 发布包。

## 安装服务

在仓库根目录打开管理员 PowerShell。默认安装只需一条命令：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
```

如果 nssm 不在 `PATH` 中，传入完整路径：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 `
  -NssmPath "C:\tools\nssm\nssm.exe"
```

如果要安装到用户目录之外，传入三个路径；升级与卸载时使用同一组值：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 `
  -InstallDir "D:\Corvus\bin" -DataDir "D:\Corvus\data" -ExtensionDir "D:\Corvus\extension"
```

安装脚本会检查管理员权限与路径，在发布二进制缺失时拒绝执行，停止已有服务，复制二进制与打包的扩展，
配置 nssm 服务并启动。服务执行的命令是 `corvusd.exe serve --db "<DataDir>\corvus.db"`。脚本是幂等的：
重复执行同一条命令会原地升级并保留数据库。如果服务在 30 秒内没有进入 `Running`，脚本会打印最近的日志
并以非零状态退出。

## 加载扩展

1. 打开 `chrome://extensions` 或 `edge://extensions`。
2. 打开开发者模式。
3. 选择“加载已解压的扩展程序”，目录选 `%USERPROFILE%\.corvus\extension`（或自定义的 `-ExtensionDir`）。
4. 打开扩展弹窗。绿点表示已连接守护进程；弹窗显示浏览器 ID、扩展版本与能力。

升级后如果 manifest 或脚本有变化，在扩展卡片上点“重新加载”。扩展没有设置页，也没有配对步骤。

## 验证安装

```powershell
Get-Service Corvus | Format-Table Name, Status, StartType
& "$env:USERPROFILE\.corvus\bin\corvus.exe" doctor
Invoke-RestMethod http://127.0.0.1:7331/health
```

`doctor` 会报告版本、WebSocket 与 MCP HTTP 端口是否可达、数据库路径，以及每个已连接浏览器一行。
`health` 返回 `status: "ok"` 和当前数据库路径。守护进程可达但没有浏览器行，说明扩展尚未加载，或扩展
连接的是另一个端口。

## 连接 MCP 客户端

### HTTP 传输

Windows 服务保持安装时使用 HTTP。用已安装的二进制生成配置：

```powershell
& "$env:USERPROFILE\.corvus\bin\corvus.exe" config --transport http
```

把输出的 `mcpServers` 对象粘贴到客户端配置。它指向 `http://127.0.0.1:7331/mcp`；保持服务运行。
`--client` 接受客户端名称，并会在 stderr 追加位置提示；stdout 的 JSON 对所有客户端使用同一套 schema。

### stdio 传输

只有客户端不支持 HTTP 时才使用 stdio。stdio 客户端会自行启动 `corvus.exe serve --stdio`，它需要与
服务相同的回环端口。必须先停止服务，否则新进程会因端口被占用而失败：

```powershell
Stop-Service Corvus
& "$env:USERPROFILE\.corvus\bin\corvus.exe" config --transport stdio
```

客户端会启动该 JSON 中的命令。stdio 客户端使用期间保持服务停止；切回 HTTP 时运行
`Start-Service Corvus`。不要在默认端口上同时运行两种传输。

## 五分钟上手流程

这是 M4 验收流程。已有准备好的 `dist/` 时可在五分钟内完成；从源码做一次全新发布构建会额外增加编译时间。

| 时间 | 步骤 | 命令或操作 | 成功信号 |
|---|---|---|---|
| 0:00 | 检查前置条件 | Windows 10/11、管理员 PowerShell、nssm、Chrome 或 Edge | `nssm version` 输出版本 |
| 0:30 | 构建或定位发布包 | `bash scripts/package.sh`（已有 `dist/` 则跳过） | `dist/corvusd.exe` 与 `dist/corvus.exe` 存在 |
| 1:30 | 安装服务 | `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1` | 安装脚本以 `service is running` 结束 |
| 2:30 | 加载扩展 | 加载已解压的扩展：`%USERPROFILE%\.corvus\extension` | 弹窗绿点亮起 |
| 3:30 | 连接客户端 | `corvus.exe config --transport http` 并粘贴配置 | 客户端列出 Corvus 服务 |
| 4:30 | 打开第一页 | 调用 `browser_navigate`，参数 `{"url":"https://example.com"}`，再调用 `browser_snapshot` | 快照返回页面标题与节点 |

## 升级

1. 构建或获取新的发布包。
2. 用相同路径执行同一条安装命令。
3. 扩展文件有变化时重新加载扩展。
4. 运行 `doctor` 检查 `version`。

```bash
bash scripts/package.sh
```

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
& "$env:USERPROFILE\.corvus\bin\corvus.exe" doctor
```

安装脚本会停止服务、替换二进制与扩展、重写服务参数，然后重新启动服务。数据库与日志不会被改动。
如果升级涉及数据库结构，先在服务停止时备份 `corvus.db` 再安装新版本。

## 卸载

在仓库根目录打开管理员 PowerShell：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall-windows.ps1
```

脚本会停止并删除 `Corvus` 服务，从安装目录移除 `corvusd.exe` 与 `corvus.exe`，并删除扩展目录。默认保留
`corvus.db` 与日志文件。如果连数据库和日志一起删除：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall-windows.ps1 -PurgeData
```

如果安装时使用了自定义布局，传入对应的 `-InstallDir`、`-DataDir`、`-ExtensionDir` 和 `-ServiceName`。
脚本是幂等的，服务已不存在时会明确提示。空的安装目录与数据目录会保留；彻底清理磁盘时可在 purge 后手动
删除。

## 服务控制与故障排查

```powershell
Get-Service Corvus
Restart-Service Corvus
nssm restart Corvus
```

| 现象 | 检查 | 处理 |
|---|---|---|
| 提示 `nssm.exe not found` | `nssm version` | 把 nssm 放入 `PATH`，或传 `-NssmPath` |
| 服务无法启动 | `%USERPROFILE%\.corvus\corvusd.err.log` | 修正日志中的路径、端口或权限问题后重跑安装脚本 |
| 7330 或 7331 被占用 | `netstat -ano | findstr :7330` | 停止另一个守护进程；不要同时运行服务与 stdio 模式 |
| `doctor` 没有浏览器 | 弹窗状态、`Get-Service Corvus` | 加载或重新加载扩展；确认 WebSocket 端口为 7330 |
| 客户端无法连接 HTTP | `Invoke-RestMethod http://127.0.0.1:7331/health` | 启动服务；使用 `127.0.0.1`，不要用局域网地址 |
| 离线打包失败 | `bash scripts/dev-setup.sh` | 先在有网络时填充 cargo 缓存，或使用准备好的 `dist/` |
| 脚本被执行策略拦截 | 错误文本会指出 ExecutionPolicy | 按上文使用 `-ExecutionPolicy Bypass` |
| 提示 `missing ...corvusd.exe` | `dist/` 内容 | 先运行 `bash scripts/package.sh` |

## 安全说明

守护进程只绑定 `127.0.0.1`，没有认证层：任何能访问这些端口的本地进程都可以驱动已连接的浏览器。不要把
7330 或 7331 暴露到回环地址之外。安装脚本按 nssm 默认账户 LocalSystem 注册服务；如果安全策略要求专用
身份，请用 nssm 修改服务账户。数据库保存了命令、参数和结果，应把 `%USERPROFILE%\.corvus\corvus.db`
视为敏感数据；把机器交给他人前先 purge。
