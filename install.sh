#!/bin/bash
set -e

REPO="EasyTier-Pro/installer"

# 检测 OS
detect_os() {
    case "$(uname -s)" in
        Linux*)     echo "linux";;
        Darwin*)    echo "macos";;
        FreeBSD*)   echo "freebsd";;
        CYGWIN*|MINGW*|MSYS*) echo "windows";;
        *)          echo "unknown";;
    esac
}

# 检测架构
detect_arch() {
    local arch
    arch=$(uname -m)
    case "$arch" in
        x86_64|amd64)       echo "x86_64";;
        aarch64|arm64)      echo "aarch64";;
        armv7l)             echo "armv7hf";;
        armv6l)             echo "armhf";;
        armv5tel|armel|arm) echo "arm";;
        i686|i386)          echo "i686";;
        riscv64)            echo "riscv64";;
        loongarch64)        echo "loongarch64";;
        mips)               echo "mips";;
        mipsel)             echo "mipsel";;
        *)                  echo "unknown";;
    esac
}

OS=$(detect_os)
ARCH=$(detect_arch)

if [ "$OS" = "unknown" ]; then
    echo "错误：不支持的操作系统: $(uname -s)"
    exit 1
fi

if [ "$ARCH" = "unknown" ]; then
    echo "错误：不支持的架构: $(uname -m)"
    exit 1
fi

if [ "$OS" = "freebsd" ]; then
    echo "错误：FreeBSD 暂无预编译二进制文件，请从源码编译安装。"
    exit 1
fi

# 下载工具检测
download_cmd() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2"
    else
        echo "错误：需要 curl 或 wget 来下载文件"
        exit 1
    fi
}

# 获取最新版本
echo "正在查询最新版本..."
LATEST=$(download_cmd "https://api.github.com/repos/$REPO/releases/latest" /dev/stdout | grep '"tag_name":' | head -n 1 | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$LATEST" ]; then
    echo "错误：无法获取最新版本信息"
    exit 1
fi

# 确定文件名
if [ "$OS" = "windows" ]; then
    ASSET="easytier-pro-installer-${OS}-${ARCH}.exe"
    BIN_NAME="easytier-pro-installer.exe"
else
    ASSET="easytier-pro-installer-${OS}-${ARCH}"
    BIN_NAME="easytier-pro-installer"
fi

URL="https://github.com/$REPO/releases/download/$LATEST/$ASSET"

# 安装目录
INSTALL_DIR="${INSTALL_DIR:-.}"
mkdir -p "$INSTALL_DIR"
DEST="$INSTALL_DIR/$BIN_NAME"

# 下载
echo "正在下载 $ASSET ($LATEST)..."
echo "  来源: $URL"

TMP_DEST="$DEST.tmp.$$"
if command -v curl >/dev/null 2>&1; then
    curl -fSL --progress-bar "$URL" -o "$TMP_DEST"
else
    wget -q --show-progress "$URL" -O "$TMP_DEST"
fi

mv "$TMP_DEST" "$DEST"
chmod +x "$DEST"

echo ""
echo "下载完成: $DEST"
echo "正在启动 installer..."
echo ""

exec "$DEST" "$@"
