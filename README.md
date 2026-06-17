# EasyTier Agent

跨平台一键部署工具。运行后自动引导完成登录、获取注册密钥、下载 EasyTier、安装系统服务的全部流程。

> **下载源说明**：install 脚本优先从 **Gitee** 下载 installer 二进制，若失败则自动回退到 GitHub。
> **校验说明**：install 脚本会下载同 release 下的 `${asset}.sha256` checksum 文件，并用 SHA-256 验证 installer 二进制后才执行；installer 下载 EasyTier ZIP 时会校验 GitHub release metadata 中的 `digest` checksum。

## 快速安装

### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/EasyTier-Pro/installer/main/install.sh | bash
```

国内网络可改用 gitee：
```bash
curl -fsSL https://gitee.com/easytier/easytier-pro-installer/raw/main/install.sh | bash
```

默认下载到 `~/.local/share/easytier-pro-installer/`，可通过环境变量指定：
```bash
curl -fsSL https://raw.githubusercontent.com/EasyTier-Pro/installer/main/install.sh | INSTALL_DIR=/usr/local/bin bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/EasyTier-Pro/installer/main/install.ps1 | iex
```

国内网络可改用 gitee：
```powershell
irm https://gitee.com/easytier/easytier-pro-installer/raw/main/install.ps1 | iex
```

默认下载到 `%LOCALAPPDATA%\easytier-pro-installer\`，可通过参数指定：
```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EasyTier-Pro/installer/main/install.ps1))) -InstallDir C:\Tools
```

## 用法

通过 install 脚本运行后会自动启动 installer，无需手动找到文件：

```bash
# Linux / macOS（每次运行都会检查更新并自动启动）
curl -fsSL https://gitee.com/easytier/easytier-pro-installer/raw/main/install.sh | bash

# Windows
irm https://gitee.com/easytier/easytier-pro-installer/raw/main/install.ps1 | iex
```

如需手动运行已下载的 installer：

```bash
# Linux / macOS
~/.local/share/easytier-pro-installer/easytier-pro-installer

# Windows
%LOCALAPPDATA%\easytier-pro-installer\easytier-pro-installer.exe
```

首次运行后，直接执行上述路径即可，不再需要通过 install 脚本。

运行示例：
```bash
~/.local/share/easytier-pro-installer/easytier-pro-installer
# 等价于
~/.local/share/easytier-pro-installer/easytier-pro-installer install
```

典型流程：

```
$ ~/.local/share/easytier-pro-installer/easytier-pro-installer
您尚未登录 Console，正在引导登录...

正在获取登录验证码...

请在浏览器中完成登录:
  访问链接: https://casdoor.easytier.cn/login/oauth/device/xxxxx
  验证码:   abcdef

✅ 登录成功！

工作空间: 个人空间

注册密钥:
  1. my-key
  2. [创建新密钥]
选择 [1-2]: 1
使用注册密钥: my-key

配置服务器: tcp://console.easytier.cn:22020/xxxxx

平台: linux-x86_64

安装目录: /home/user/.local/share/easytier-pro-installer/easytier

正在查询最新版本...
最新版本: v2.6.4

正在下载: https://github.com/.../easytier-linux-x86_64-v2.6.4.zip
[####################] 100% 下载完成

正在解压...
EasyTier 已就绪: ...
  core: .../easytier-core
  cli:  .../easytier-cli

正在安装系统服务...
正在启动服务...

✅ 部署完成！EasyTier 服务已安装并启动。
```

## 其他操作

```bash
# 更新已安装的 EasyTier
~/.local/share/easytier-pro-installer/easytier-pro-installer update

# 更新到指定版本
~/.local/share/easytier-pro-installer/easytier-pro-installer update --version v2.6.4

# 查看服务状态
~/.local/share/easytier-pro-installer/easytier-pro-installer status

# 卸载服务，保留已下载文件和缓存
~/.local/share/easytier-pro-installer/easytier-pro-installer uninstall

# 彻底卸载，删除安装目录和缓存压缩包
~/.local/share/easytier-pro-installer/easytier-pro-installer uninstall --purge
```

## 可选参数

| 参数 | 环境变量 | 说明 |
|------|----------|------|
| `-s, --server` | `EASYTIER_CONSOLE_URL` | Console 地址，默认 `https://api.console.easytier.net` |
| `--config-server` | `EASYTIER_CONFIG_SERVER` | 覆盖 config server 地址 |
| `-i, --install-dir` | `EASYTIER_INSTALL_DIR` | 安装目录 |
| `--debug` | - | 开启调试日志，默认写入当前目录下的 `easytier-pro-installer.debug.log` |

子命令参数：

| 子命令 | 参数 | 环境变量 | 说明 |
|--------|------|----------|------|
| `install` | `-v, --version` | `EASYTIER_VERSION` | 指定安装的 EasyTier 版本号 |
| `update` | `-v, --version` | `EASYTIER_VERSION` | 指定更新到的 EasyTier 版本号 |
| `uninstall` | `--purge` | - | 彻底删除安装目录和缓存压缩包 |

## 桌面端集成

桌面应用可以随安装包内置 `easytier-pro-installer` 二进制，然后用子进程调用 `desktop` 子命令。桌面端负责登录、选择注册密钥和 Console UI，installer 只负责本机 EasyTier 服务生命周期，并向 `stdout` 输出 JSON Lines 事件。

命令形式固定为：

```text
easytier-pro-installer desktop <install|status|update|uninstall> --json
```

调用约定：

- 请求体是写入 `stdin` 的单个 JSON object。
- 响应是写入 `stdout` 的 JSON Lines，每行格式为 `{"event":"...","data":{...}}`。
- `--json` 必须显式传入；缺失时会返回 `error` 事件。
- 请求字段启用严格校验，未知字段会被拒绝。
- 错误统一输出为 `error` 事件，`data.code` 为 `invalid_request`、`permission_denied` 或 `internal_error`，`data.message` 为错误说明。

```bash
printf '%s' '{}' | easytier-pro-installer desktop status --json
```

常用命令：

```bash
easytier-pro-installer desktop install --json
easytier-pro-installer desktop status --json
easytier-pro-installer desktop update --json
easytier-pro-installer desktop uninstall --json
```

请求 schema：

| 命令 | 字段 | 必填 | 说明 |
|------|------|------|------|
| `install` | `bootstrap_token` | 是 | 桌面端从 Console 获取的注册密钥；示例使用占位符，日志和输出不应暴露原值 |
| `install` | `version` | 是 | 目标 EasyTier 版本，例如 `v2.6.4` |
| `install` | `install_dir` | 否 | 安装目录；缺省使用全局 `--install-dir` 或默认安装目录 |
| `install` | `config_server` | 否 | config server 基础地址；缺省使用全局 `--config-server` 或默认 Console 配置 |
| `status` | `bootstrap_token` | 否 | 用于和本机已安装服务的 bootstrap fingerprint 比对，不输出原值 |
| `status` | `version` | 否 | 用于计算 `target_version` 和 `version_match` |
| `status` | `install_dir` | 否 | 安装目录；缺省规则同 `install` |
| `status` | `config_server` | 否 | 用于计算 `config_server_match`；不传则不校验该项 |
| `update` | `version` | 是 | 目标 EasyTier 版本 |
| `update` | `install_dir` | 否 | 安装目录；缺省规则同 `install` |
| `uninstall` | `install_dir` | 否 | 安装目录；缺省规则同 `install` |
| `uninstall` | `purge` | 否 | 布尔值；`true` 时删除安装目录和缓存，缺省为 `false` |

`install` 请求示例：

```json
{
  "bootstrap_token": "BOOTSTRAP_TOKEN",
  "install_dir": "/opt/easytier",
  "config_server": "tcp://console.easytier.net:22020",
  "version": "v2.6.4"
}
```

`status` 请求示例：

```json
{
  "bootstrap_token": "BOOTSTRAP_TOKEN",
  "install_dir": "/opt/easytier",
  "config_server": "tcp://console.easytier.net:22020",
  "version": "v2.6.4"
}
```

`update` 请求示例：

```json
{
  "install_dir": "/opt/easytier",
  "version": "v2.6.4"
}
```

`uninstall` 请求示例：

```json
{
  "install_dir": "/opt/easytier",
  "purge": true
}
```

`install` 使用桌面端传入的 `bootstrap_token` 创建服务；`update` 必须传入目标 `version` 并会更新二进制后重启服务；`uninstall` 可传 `purge: true` 删除安装目录和缓存。`status` 只返回本机 core/cli、版本、machine id 和服务状态，不访问 Console。

事件说明：

| 事件 | 命令 | 重要字段 |
|------|------|----------|
| `started` | `install` | `install_dir` |
| `started` | `update` | `install_dir`, `version` |
| `started` | `uninstall` | `install_dir`, `purge` |
| `platform_detected` | `install`, `update` | `os`, `arch` |
| `identity_evaluated` | `install` | `identity_match`, `binaries_present`, `installed_version`, `target_version`, `service_installed`, `service_running`, `config_server_match` |
| `download_started` | `install`, `update` | `version`, `install_dir`, `cache_dir` |
| `download_progress` | `install`, `update` | `downloaded`, `total` |
| `download_finished` | `install`, `update` | `version`, `core_path`, `cli_path` |
| `service_installing` | `install` | `mode` 可能为 `reuse_existing`；新安装路径可能为空 object |
| `service_started` | `install` | `service_name`, `reused` |
| `service_started` | `update` | `service_name` |
| `service_uninstalling` | `uninstall` | 空 object |
| `service_uninstalled` | `uninstall` | 空 object |
| `finished` | `install` | `machine_id`, `install_dir`, `core_path`, `cli_path`, `version`, `reused` |
| `finished` | `status` | `install_dir`, `service_name`, `installed`, `running`, `service_state`, `binary_path`, `machine_id`, `core_path`, `cli_path`, `binaries_present`, `version`, `target_version`, `bootstrap_fingerprint`, `identity_match`, `version_match`, `config_server_match`, `ready` |
| `finished` | `update` | `install_dir`, `version`, `up_to_date` |
| `finished` | `uninstall` | `install_dir`, `purged` |
| `error` | 所有命令 | `code`, `message` |

敏感字段约定：`bootstrap_token` 不会作为事件字段输出；状态事件使用 `bootstrap_fingerprint` 表示本机服务身份。可能包含 config server URL 的文本字段会经过脱敏处理，客户端不应依赖其中的密钥片段。

兼容性约定：当前没有单独的桌面协议版本号。桌面端应忽略未知事件字段；遇到未知事件名时应保守处理，不要阻塞已知终态事件。随桌面安装包分发时建议固定 installer 版本，并在升级 installer 时同步验证事件处理逻辑。

## 构建

项目固定使用 `rust-toolchain.toml` 中的 Rust 版本；安装 `rustup` 后，`cargo` 会自动读取该文件并安装所需的 `rustfmt`、`clippy` 组件。
Release 中的 MIPS 目标因 `-Z build-std` 使用单独固定的 `nightly-2026-04-21` toolchain。

```bash
cargo build --release
```

## 支持平台

- Linux (x86_64, aarch64, riscv64, loongarch64, armv7hf, armv7, armhf, arm, mips, mipsel)
- Windows (x86_64, i686, arm64)
- macOS (x86_64, aarch64)
