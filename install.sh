#!/bin/bash
set -e

GITHUB_REPO="EasyTier-Pro/installer"
GITEE_REPO="easytier/easytier-pro-installer"

# 优先从 gitee 下载，失败回退到 github
RELEASE_API_BASE="https://gitee.com/api/v5/repos"
RELEASE_DOWNLOAD_BASE="https://gitee.com"
REPO="$GITEE_REPO"

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

verify_checksum() {
    local file="$1"
    local checksum_file="$2"
    local asset="$3"
    local expected=""
    local candidate=""
    local candidate_asset=""
    local actual=""
    local tool=""

    while IFS= read -r line || [ -n "$line" ]; do
        set -- $line
        if [ "$#" -eq 0 ]; then
            continue
        fi
        candidate="$1"
        candidate_asset="${2#\*}"
        if [ -z "$candidate_asset" ] || [ "$candidate_asset" = "$asset" ]; then
            expected="$candidate"
            break
        fi
    done < "$checksum_file"

    if [ "${#expected}" -ne 64 ]; then
        echo "错误：$checksum_file 中未找到 $asset 的 SHA-256 checksum"
        return 1
    fi
    case "$expected" in
        *[!0123456789abcdefABCDEF]*)
            echo "错误：$checksum_file 中的 SHA-256 checksum 无效"
            return 1
            ;;
    esac

    if command -v sha256sum >/dev/null 2>&1; then
        tool="sha256sum"
        actual=$(sha256sum "$file")
    elif command -v shasum >/dev/null 2>&1; then
        tool="shasum"
        actual=$(shasum -a 256 "$file")
    else
        echo "错误：需要 sha256sum 或 shasum 来验证 checksum"
        return 1
    fi
    actual="${actual%% *}"

    if [ "$actual" != "$expected" ]; then
        echo "错误：$asset SHA-256 checksum 不匹配"
        echo "  expected: $expected"
        echo "  actual:   $actual"
        return 1
    fi

    echo "checksum 验证通过 ($tool)"
}

# 获取最新版本
echo "正在查询最新版本..."
LATEST=$(download_cmd "$RELEASE_API_BASE/$REPO/releases/latest" /dev/stdout | grep -o '"tag_name":"[^"]*"' | head -n 1 | cut -d'"' -f4)
if [ -z "$LATEST" ]; then
    echo "gitee 获取失败，尝试 github..."
    RELEASE_API_BASE="https://api.github.com/repos"
    RELEASE_DOWNLOAD_BASE="https://github.com"
    REPO="$GITHUB_REPO"
    LATEST=$(download_cmd "$RELEASE_API_BASE/$REPO/releases/latest" /dev/stdout | grep -o '"tag_name":"[^"]*"' | head -n 1 | cut -d'"' -f4)
    if [ -z "$LATEST" ]; then
        echo "错误：无法获取最新版本信息"
        exit 1
    fi
fi

# 确定文件名
if [ "$OS" = "windows" ]; then
    ASSET="easytier-pro-installer-${OS}-${ARCH}.exe"
    BIN_NAME="easytier-pro-installer.exe"
else
    ASSET="easytier-pro-installer-${OS}-${ARCH}"
    BIN_NAME="easytier-pro-installer"
fi

URL="$RELEASE_DOWNLOAD_BASE/$REPO/releases/download/$LATEST/$ASSET"
CHECKSUM_ASSET="$ASSET.sha256"
CHECKSUM_URL="$URL.sha256"

# 安装目录
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/share/easytier-pro-installer}"
mkdir -p "$INSTALL_DIR"
DEST="$INSTALL_DIR/$BIN_NAME"
VERSION_FILE="$DEST.version"
TMP_DEST="$DEST.tmp.$$"
TMP_CHECKSUM="$TMP_DEST.sha256"

cleanup_tmp() {
    rm -f "$TMP_DEST" "$TMP_CHECKSUM"
}
trap cleanup_tmp EXIT

echo "正在下载 checksum: $CHECKSUM_ASSET"
download_cmd "$CHECKSUM_URL" "$TMP_CHECKSUM"

# 检查本地缓存
if [ -f "$DEST" ] && [ -f "$VERSION_FILE" ]; then
    LOCAL_VERSION=$(cat "$VERSION_FILE" 2>/dev/null || true)
    if [ "$LOCAL_VERSION" = "$LATEST" ]; then
        if verify_checksum "$DEST" "$TMP_CHECKSUM" "$ASSET"; then
            echo "本地已是最新版本 $LATEST，跳过下载"
            exec "$DEST" "$@"
        fi
        echo "本地缓存校验失败，重新下载"
        rm -f "$DEST" "$VERSION_FILE"
    fi
fi

# 下载
echo "目标路径: $DEST"
echo "正在下载 $ASSET ($LATEST)..."
echo "  来源: $URL"

if command -v curl >/dev/null 2>&1; then
    curl -fSL --progress-bar "$URL" -o "$TMP_DEST"
else
    wget -q --show-progress "$URL" -O "$TMP_DEST"
fi

verify_checksum "$TMP_DEST" "$TMP_CHECKSUM" "$ASSET"
mv "$TMP_DEST" "$DEST"
chmod +x "$DEST"
echo "$LATEST" > "$VERSION_FILE"

echo ""
echo "下载完成: $DEST"
echo "正在启动 installer..."
echo ""

exec "$DEST" "$@"
