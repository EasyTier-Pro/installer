#Requires -Version 3.0

param(
    [string]$InstallDir = "$env:LOCALAPPDATA\easytier-pro-installer",
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$InstallerArgs
)

$GithubRepo = "EasyTier-Pro/installer"
$GiteeRepo = "easytier/easytier-pro-installer"

# 优先从 gitee 下载，失败回退到 github
$ReleaseApiBase = "https://gitee.com/api/v5/repos"
$ReleaseDownloadBase = "https://gitee.com"
$Repo = $GiteeRepo

function Invoke-InstallerRestMethod {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Uri
    )

    $Params = @{ Uri = $Uri }
    if ((Get-Command Invoke-RestMethod).Parameters.ContainsKey("UseBasicParsing")) {
        $Params.UseBasicParsing = $true
    }

    Invoke-RestMethod @Params
}

function Invoke-InstallerWebRequest {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Uri,
        [Parameter(Mandatory = $true)]
        [string]$OutFile
    )

    $Params = @{
        Uri = $Uri
        OutFile = $OutFile
    }
    if ((Get-Command Invoke-WebRequest).Parameters.ContainsKey("UseBasicParsing")) {
        $Params.UseBasicParsing = $true
    }

    Invoke-WebRequest @Params
}

function Enable-InstallerVirtualTerminal {
    try {
        if (-not ("EasyTierInstallerConsoleMode" -as [type])) {
            Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class EasyTierInstallerConsoleMode
{
    private const int STD_OUTPUT_HANDLE = -11;
    private const int STD_ERROR_HANDLE = -12;
    private const uint ENABLE_VIRTUAL_TERMINAL_PROCESSING = 0x0004;

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetStdHandle(int nStdHandle);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetConsoleMode(IntPtr hConsoleHandle, out uint lpMode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetConsoleMode(IntPtr hConsoleHandle, uint dwMode);

    public static void Enable()
    {
        EnableOn(STD_OUTPUT_HANDLE);
        EnableOn(STD_ERROR_HANDLE);
    }

    private static void EnableOn(int stdHandle)
    {
        IntPtr handle = GetStdHandle(stdHandle);
        if (handle == IntPtr.Zero || handle == new IntPtr(-1)) {
            return;
        }

        uint mode;
        if (!GetConsoleMode(handle, out mode)) {
            return;
        }

        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
    }
}
"@
        }

        [EasyTierInstallerConsoleMode]::Enable()
    } catch {
        # Best-effort only. The installer still works without ANSI colors.
    }
}

function Start-Installer {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [string[]]$InstallerArguments
    )

    Enable-InstallerVirtualTerminal
    & $Path @InstallerArguments
}

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
    $Release = Invoke-InstallerRestMethod -Uri "$ReleaseApiBase/$Repo/releases/latest"
    $Version = $Release.tag_name
} catch {
    Write-Host "gitee 获取失败，尝试 github..."
    $ReleaseApiBase = "https://api.github.com/repos"
    $ReleaseDownloadBase = "https://github.com"
    $Repo = $GithubRepo
    try {
        $Release = Invoke-InstallerRestMethod -Uri "$ReleaseApiBase/$Repo/releases/latest"
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
$VersionFile = "$Dest.version"

# 检查本地缓存
if ((Test-Path $Dest) -and (Test-Path $VersionFile)) {
    $LocalVersion = Get-Content $VersionFile -Raw
    if ($LocalVersion.Trim() -eq $Version) {
        Write-Host "本地已是最新版本 $Version，跳过下载"
        Start-Installer -Path $Dest -InstallerArguments $InstallerArgs
        exit 0
    }
}

# 下载
Write-Host "目标路径: $Dest"
Write-Host "正在下载 $Asset ($Version)..."
Write-Host "  来源: $Url"

try {
    $ProgressPreference = 'Continue'
    Invoke-InstallerWebRequest -Uri $Url -OutFile $Dest
} catch {
    Write-Error "错误：下载失败: $_"
    exit 1
}

$Version | Out-File $VersionFile -Encoding utf8

Write-Host ""
Write-Host "下载完成: $Dest"
Write-Host "正在启动 installer..."
Write-Host ""

# 运行 installer
Start-Installer -Path $Dest -InstallerArguments $InstallerArgs
