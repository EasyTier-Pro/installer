#Requires -Version 5.1

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

function Get-ExpectedChecksum {
    param(
        [string]$ChecksumPath,
        [string]$AssetName
    )

    foreach ($Line in Get-Content -Path $ChecksumPath) {
        $Trimmed = $Line.Trim()
        if (-not $Trimmed) {
            continue
        }

        $Parts = $Trimmed -split '\s+'
        $Candidate = $Parts[0]
        $CandidateAsset = $null
        if ($Parts.Length -ge 2) {
            $CandidateAsset = $Parts[1].TrimStart('*')
        }

        if ((-not $CandidateAsset) -or ($CandidateAsset -eq $AssetName)) {
            if ($Candidate -notmatch '^[0-9a-fA-F]{64}$') {
                throw "错误：$ChecksumPath 中的 SHA-256 checksum 无效"
            }
            return $Candidate
        }
    }

    throw "错误：$ChecksumPath 中未找到 $AssetName 的 SHA-256 checksum"
}

function Test-Checksum {
    param(
        [string]$FilePath,
        [string]$ChecksumPath,
        [string]$AssetName
    )

    $Expected = Get-ExpectedChecksum -ChecksumPath $ChecksumPath -AssetName $AssetName
    $Actual = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash
    if (-not [string]::Equals($Actual, $Expected, [System.StringComparison]::OrdinalIgnoreCase)) {
        Write-Error "错误：$AssetName SHA-256 checksum 不匹配`n  expected: $Expected`n  actual:   $Actual"
        return $false
    }

    Write-Host "checksum 验证通过"
    return $true
}

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
$ChecksumAsset = "$Asset.sha256"
$ChecksumUrl = "$Url.sha256"

# 安装目录
$InstallPath = Resolve-Path $InstallDir -ErrorAction SilentlyContinue
if (-not $InstallPath) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
    $InstallPath = Resolve-Path $InstallDir
}

$Dest = Join-Path $InstallPath "easytier-pro-installer.exe"
$VersionFile = "$Dest.version"
$TempDest = "$Dest.tmp.$PID"
$TempChecksum = "$TempDest.sha256"

try {
    Write-Host "正在下载 checksum: $ChecksumAsset"
    Invoke-WebRequest -Uri $ChecksumUrl -OutFile $TempChecksum -UseBasicParsing
} catch {
    Write-Error "错误：checksum 下载失败: $_"
    exit 1
}

# 检查本地缓存
if ((Test-Path $Dest) -and (Test-Path $VersionFile)) {
    $LocalVersion = Get-Content $VersionFile -Raw
    if ($LocalVersion.Trim() -eq $Version) {
        try {
            if (Test-Checksum -FilePath $Dest -ChecksumPath $TempChecksum -AssetName $Asset) {
                Write-Host "本地已是最新版本 $Version，跳过下载"
                Remove-Item $TempChecksum -Force -ErrorAction SilentlyContinue
                & $Dest @InstallerArgs
                exit 0
            }
        } catch {
            Write-Error $_
        }
        Write-Host "本地缓存校验失败，重新下载"
        Remove-Item $Dest -Force -ErrorAction SilentlyContinue
        Remove-Item $VersionFile -Force -ErrorAction SilentlyContinue
    }
}

# 下载
Write-Host "目标路径: $Dest"
Write-Host "正在下载 $Asset ($Version)..."
Write-Host "  来源: $Url"

try {
    $ProgressPreference = 'Continue'
    Invoke-WebRequest -Uri $Url -OutFile $TempDest -UseBasicParsing
} catch {
    Write-Error "错误：下载失败: $_"
    Remove-Item $TempDest -Force -ErrorAction SilentlyContinue
    Remove-Item $TempChecksum -Force -ErrorAction SilentlyContinue
    exit 1
}

try {
    if (-not (Test-Checksum -FilePath $TempDest -ChecksumPath $TempChecksum -AssetName $Asset)) {
        Remove-Item $TempDest -Force -ErrorAction SilentlyContinue
        Remove-Item $TempChecksum -Force -ErrorAction SilentlyContinue
        exit 1
    }
} catch {
    Write-Error $_
    Remove-Item $TempDest -Force -ErrorAction SilentlyContinue
    Remove-Item $TempChecksum -Force -ErrorAction SilentlyContinue
    exit 1
}

Move-Item -Path $TempDest -Destination $Dest -Force
Remove-Item $TempChecksum -Force -ErrorAction SilentlyContinue

$Version | Out-File $VersionFile -Encoding utf8

Write-Host ""
Write-Host "下载完成: $Dest"
Write-Host "正在启动 installer..."
Write-Host ""

# 运行 installer
& $Dest @InstallerArgs
