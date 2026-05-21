#Requires -Version 5.1

param(
    [string]$InstallDir = ".",
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$InstallerArgs
)

$GithubRepo = "EasyTier-Pro/installer"
$GiteeRepo = "easytier/easytier-pro-installer"

# 优先从 gitee 下载，失败回退到 github
$ReleaseApiBase = "https://gitee.com/api/v5/repos"
$ReleaseDownloadBase = "https://gitee.com"
$Repo = $GiteeRepo

# 检测架构
switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { $Arch = "x86_64" }
    "ARM64" { $Arch = "arm64" }
    "x86"   { $Arch = "i686" }
    default {
        Write-Error "错误：不支持的架构: $($env:PROCESSOR_ARCHITECTURE)"
        exit 1
    }
}

$OS = "windows"

# 获取最新版本
Write-Host "正在查询最新版本..."
try {
    $Release = Invoke-RestMethod -Uri "$ReleaseApiBase/$Repo/releases/latest" -UseBasicParsing
    $Version = $Release.tag_name
} catch {
    Write-Host "gitee 获取失败，尝试 github..."
    $ReleaseApiBase = "https://api.github.com/repos"
    $ReleaseDownloadBase = "https://github.com"
    $Repo = $GithubRepo
    try {
        $Release = Invoke-RestMethod -Uri "$ReleaseApiBase/$Repo/releases/latest" -UseBasicParsing
        $Version = $Release.tag_name
    } catch {
        Write-Error "错误：无法获取最新版本信息: $_"
        exit 1
    }
}

$Asset = "easytier-pro-installer-${OS}-${Arch}.exe"
$Url = "$ReleaseDownloadBase/$Repo/releases/download/$Version/$Asset"

# 安装目录
$InstallPath = Resolve-Path $InstallDir -ErrorAction SilentlyContinue
if (-not $InstallPath) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
    $InstallPath = Resolve-Path $InstallDir
}

$Dest = Join-Path $InstallPath "easytier-pro-installer.exe"

# 下载
Write-Host "正在下载 $Asset ($Version)..."
Write-Host "  来源: $Url"

try {
    $ProgressPreference = 'Continue'
    Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing
} catch {
    Write-Error "错误：下载失败: $_"
    exit 1
}

Write-Host ""
Write-Host "下载完成: $Dest"
Write-Host "正在启动 installer..."
Write-Host ""

# 运行 installer
& $Dest @InstallerArgs
