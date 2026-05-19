use std::path::PathBuf;

pub(crate) struct Platform {
    pub(crate) os: &'static str,
    pub(crate) arch: &'static str,
}

pub(crate) fn detect_platform() -> anyhow::Result<Platform> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let et_os = match os {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "darwin",
        "freebsd" => "freebsd",
        _ => anyhow::bail!("不支持的操作系统: {}", os),
    };

    let et_arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" => {
            if os == "windows" {
                "arm64"
            } else {
                "aarch64"
            }
        }
        "arm" => "arm",
        _ => anyhow::bail!("不支持的架构: {}", arch),
    };

    Ok(Platform {
        os: et_os,
        arch: et_arch,
    })
}

pub(crate) fn default_install_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        directories::ProjectDirs::from("cn", "easytier", "agent")
            .map(|d| d.data_dir().join("easytier"))
            .unwrap_or_else(|| {
                let local_app_data = std::env::var("LOCALAPPDATA")
                    .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".to_string());
                PathBuf::from(local_app_data).join("easytier")
            })
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/usr/local/easytier")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        PathBuf::from("/opt/easytier")
    }
}

/// 检测当前是否以 root / 管理员身份运行。
pub(crate) fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        // net session 只有管理员才能成功执行
        std::process::Command::new("cmd")
            .args(["/C", "net", "session"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// 尝试以提升后的权限重新运行当前程序。
/// Unix 下用 sudo -E；Windows 下返回错误提示用户手动操作。
pub(crate) fn relaunch_elevated() -> anyhow::Result<std::process::ExitStatus> {
    #[cfg(unix)]
    {
        let exe = std::env::current_exe()?;
        let args = std::env::args().skip(1).collect::<Vec<String>>();

        let mut cmd = std::process::Command::new("sudo");
        cmd.arg("-E").arg(&exe).args(&args);
        cmd.stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        let status = cmd.status().map_err(|e| {
            anyhow::anyhow!(
                "无法调用 sudo ({})，请手动使用 sudo 重新运行本程序",
                e
            )
        })?;

        Ok(status)
    }
    #[cfg(windows)]
    {
        anyhow::bail!(
            "需要管理员权限，请右键点击 PowerShell/CMD 选择\"以管理员身份运行\"，然后重新执行本程序"
        )
    }
}

pub(crate) fn build_config_server_url(
    console_url: &str,
    override_base: Option<String>,
) -> anyhow::Result<String> {
    if let Some(base) = override_base {
        return Ok(base);
    }
    let url = console_url.parse::<reqwest::Url>()?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("无法解析 Console 地址"))?;
    Ok(format!("tcp://{}:22020", host))
}
