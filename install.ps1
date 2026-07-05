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

function Get-InstallerNativeMethodsType {
    $Type = "EasyTierInstaller.NativeMethods" -as [type]
    if ($Type) {
        return $Type
    }

    $AssemblyName = New-Object System.Reflection.AssemblyName "EasyTierInstaller.Native"
    try {
        $AssemblyBuilder = [System.Reflection.Emit.AssemblyBuilder]::DefineDynamicAssembly(
            $AssemblyName,
            [System.Reflection.Emit.AssemblyBuilderAccess]::Run
        )
    } catch {
        $AssemblyBuilder = [AppDomain]::CurrentDomain.DefineDynamicAssembly(
            $AssemblyName,
            [System.Reflection.Emit.AssemblyBuilderAccess]::Run
        )
    }

    $ModuleBuilder = $AssemblyBuilder.DefineDynamicModule("EasyTierInstaller.Native")
    $TypeBuilder = $ModuleBuilder.DefineType(
        "EasyTierInstaller.NativeMethods",
        [System.Reflection.TypeAttributes]"Public,Sealed,Abstract"
    )

    $MethodAttributes = [System.Reflection.MethodAttributes]"Public,Static,PinvokeImpl"
    $CallingConvention = [System.Runtime.InteropServices.CallingConvention]::Winapi
    $CharSet = [System.Runtime.InteropServices.CharSet]::Auto
    $StandardCall = [System.Reflection.CallingConventions]::Standard

    $Method = $TypeBuilder.DefinePInvokeMethod(
        "GetStdHandle",
        "kernel32.dll",
        $MethodAttributes,
        $StandardCall,
        [IntPtr],
        [type[]]@([int]),
        $CallingConvention,
        $CharSet
    )
    $Method.SetImplementationFlags(
        $Method.GetMethodImplementationFlags() -bor [System.Reflection.MethodImplAttributes]::PreserveSig
    )

    $UInt32Ref = [uint32].MakeByRefType()
    $Method = $TypeBuilder.DefinePInvokeMethod(
        "GetConsoleMode",
        "kernel32.dll",
        $MethodAttributes,
        $StandardCall,
        [bool],
        [type[]]@([IntPtr], $UInt32Ref),
        $CallingConvention,
        $CharSet
    )
    $Method.SetImplementationFlags(
        $Method.GetMethodImplementationFlags() -bor [System.Reflection.MethodImplAttributes]::PreserveSig
    )

    $Method = $TypeBuilder.DefinePInvokeMethod(
        "SetConsoleMode",
        "kernel32.dll",
        $MethodAttributes,
        $StandardCall,
        [bool],
        [type[]]@([IntPtr], [uint32]),
        $CallingConvention,
        $CharSet
    )
    $Method.SetImplementationFlags(
        $Method.GetMethodImplementationFlags() -bor [System.Reflection.MethodImplAttributes]::PreserveSig
    )

    try {
        return $TypeBuilder.CreateType()
    } catch {
        return $TypeBuilder.CreateTypeInfo().AsType()
    }
}

function Enable-InstallerVirtualTerminal {
    try {
        $NativeMethods = Get-InstallerNativeMethodsType
        $GetStdHandle = $NativeMethods.GetMethod("GetStdHandle")
        $GetConsoleMode = $NativeMethods.GetMethod("GetConsoleMode")
        $SetConsoleMode = $NativeMethods.GetMethod("SetConsoleMode")
        $EnableVirtualTerminalProcessing = [uint32]4
        $InvalidHandle = [IntPtr](-1)

        foreach ($StdHandle in @(-11, -12)) {
            $Handle = $GetStdHandle.Invoke($null, @($StdHandle))
            if ($Handle -eq [IntPtr]::Zero -or $Handle -eq $InvalidHandle) {
                continue
            }

            $Arguments = @($Handle, [uint32]0)
            if (-not [bool]$GetConsoleMode.Invoke($null, $Arguments)) {
                continue
            }

            $Mode = [uint32]$Arguments[1]
            $NewMode = [uint32]($Mode -bor $EnableVirtualTerminalProcessing)
            [void]$SetConsoleMode.Invoke($null, @($Handle, $NewMode))
        }
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
        $global:LASTEXITCODE = 1
        return
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
        $global:LASTEXITCODE = 1
        return
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
        return
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
    $global:LASTEXITCODE = 1
    return
}

$Version | Out-File $VersionFile -Encoding utf8

Write-Host ""
Write-Host "下载完成: $Dest"
Write-Host "正在启动 installer..."
Write-Host ""

# 运行 installer
Start-Installer -Path $Dest -InstallerArguments $InstallerArgs
