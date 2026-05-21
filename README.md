# EasyTier Agent

跨平台一键部署工具。运行后自动引导完成登录、获取注册密钥、下载 EasyTier、安装系统服务的全部流程。

> **下载源说明**：install 脚本优先从 **Gitee** 下载 installer 二进制，若失败则自动回退到 GitHub。

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

## 构建

```bash
cd agent
cargo build --release
```

## 支持平台

- Linux (x86_64, aarch64, arm)
- Windows (x86_64, arm64)
- macOS (x86_64, aarch64)
- FreeBSD (x86_64)
