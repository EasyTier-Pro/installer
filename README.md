# EasyTier Agent

跨平台一键部署工具。运行后自动引导完成登录、获取注册密钥、下载 EasyTier、安装系统服务的全部流程。

## 用法

```bash
# 直接运行，按提示操作即可
./easytier-agent
```

典型流程：

```
$ ./easytier-agent
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

安装目录: /home/user/.local/share/easytier-agent/easytier

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
# 查看服务状态
./easytier-agent --status

# 卸载服务
./easytier-agent --uninstall
```

## 可选参数

| 参数 | 环境变量 | 说明 |
|------|----------|------|
| `-s, --server` | `EASYTIER_CONSOLE_URL` | Console 地址，默认 `https://console.easytier.cn` |
| `--config-server` | `EASYTIER_CONFIG_SERVER` | 覆盖 config server 地址 |
| `-i, --install-dir` | `EASYTIER_INSTALL_DIR` | 安装目录 |
| `-v, --version` | `EASYTIER_VERSION` | 指定 EasyTier 版本号 |

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
