# EasyTier Agent

跨平台一键部署工具。运行后自动引导完成登录、获取注册密钥、下载 EasyTier、安装系统服务的全部流程。

## 快速安装

### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/EasyTier-Pro/installer/main/install.sh | bash
```

指定安装目录（将 installer 下载到该目录后执行）：
```bash
curl -fsSL https://raw.githubusercontent.com/EasyTier-Pro/installer/main/install.sh | INSTALL_DIR=/usr/local/bin bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/EasyTier-Pro/installer/main/install.ps1 | iex
```

## 用法

```bash
# 直接运行，按提示操作即可
./easytier-pro-installer

# 等价于
./easytier-pro-installer install
```

典型流程：

```
$ ./easytier-pro-installer
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
./easytier-pro-installer update

# 更新到指定版本
./easytier-pro-installer update --version v2.6.4

# 查看服务状态
./easytier-pro-installer status

# 卸载服务，保留已下载文件和缓存
./easytier-pro-installer uninstall

# 彻底卸载，删除安装目录和缓存压缩包
./easytier-pro-installer uninstall --purge
```

## 可选参数

| 参数 | 环境变量 | 说明 |
|------|----------|------|
| `-s, --server` | `EASYTIER_CONSOLE_URL` | Console 地址，默认 `https://console.easytier.cn` |
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
