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

pub(crate) fn default_cache_dir() -> PathBuf {
    if let Some(dirs) = directories::ProjectDirs::from("cn", "easytier", "agent") {
        return dirs.cache_dir().join("downloads");
    }

    #[cfg(target_os = "windows")]
    {
        let local_app_data = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".to_string());
        PathBuf::from(local_app_data)
            .join("easytier")
            .join("downloads")
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/tmp/easytier/downloads")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        PathBuf::from("/tmp/easytier/downloads")
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
/// Unix 下用 sudo -E；Windows 下通过 UAC (ShellExecuteExW + runas) 自动提权。
pub(crate) fn relaunch_elevated() -> anyhow::Result<std::process::ExitStatus> {
    relaunch_elevated_with_args(&[])
}

pub(crate) fn relaunch_elevated_with_args(extra_args: &[&str]) -> anyhow::Result<std::process::ExitStatus> {
    #[cfg(unix)]
    {
        let exe = std::env::current_exe()?;
        let mut args = std::env::args().skip(1).collect::<Vec<String>>();
        args.extend(extra_args.iter().map(|s| s.to_string()));

        let mut cmd = std::process::Command::new("sudo");
        cmd.arg("-E").arg(&exe).args(&args);
        cmd.stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        let status = cmd.status().map_err(|e| {
            anyhow::anyhow!("无法调用 sudo ({})，请手动使用 sudo 重新运行本程序", e)
        })?;

        Ok(status)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::process::ExitStatusExt;

        let exe = std::env::current_exe()?;
        crate::style::debug(&format!("Windows 提权开始: exe={}", exe.display()));
        if let Ok(cwd) = std::env::current_dir() {
            crate::style::debug(&format!("Windows 提权开始: cwd={}", cwd.display()));
        }
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            crate::style::debug(&format!("Windows 提权开始: LOCALAPPDATA={}", local_app_data));
        }
        let mut args: Vec<String> = std::env::args().skip(1).collect();
        crate::style::debug(&format!("Windows 提权开始: 原始参数={:?}", args));
        let has_install_dir_arg = args
            .iter()
            .any(|arg| arg == "--install-dir" || arg == "-i");
        if !has_install_dir_arg {
            let install_dir = default_install_dir();
            crate::style::debug(&format!(
                "Windows 提权开始: 自动追加 install_dir={}",
                install_dir.display()
            ));
            args.push("--install-dir".to_string());
            args.push(install_dir.to_string_lossy().to_string());
        }
        let has_debug_arg = args.iter().any(|arg| arg == "--debug");
        if crate::style::debug_enabled() && !has_debug_arg {
            crate::style::debug("Windows 提权开始: 自动追加 --debug");
            args.push("--debug".to_string());
        }
        args.extend(extra_args.iter().map(|s| s.to_string()));
        crate::style::debug(&format!("Windows 提权开始: 最终参数={:?}", args));

        // 构建参数字符串（简单转义：含空格则加引号）
        let mut params = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                params.push(' ');
            }
            if arg.contains(' ') || arg.contains('\t') {
                params.push('"');
                params.push_str(arg);
                params.push('"');
            } else {
                params.push_str(arg);
            }
        }
        crate::style::debug(&format!("Windows 提权开始: 参数字符串={}", params));

        let exe_wide: Vec<u16> =
            std::ffi::OsStr::new(&exe).encode_wide().chain(Some(0)).collect();
        let param_wide: Vec<u16> = params.encode_utf16().chain(Some(0)).collect();
        let verb_wide: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
        let cwd = std::env::current_dir()?;
        let cwd_wide: Vec<u16> = cwd.as_os_str().encode_wide().chain(Some(0)).collect();

        let mut sei = unsafe { std::mem::zeroed::<windows_sys::Win32::UI::Shell::SHELLEXECUTEINFOW>() };
        sei.cbSize = std::mem::size_of::<windows_sys::Win32::UI::Shell::SHELLEXECUTEINFOW>() as u32;
        sei.fMask = windows_sys::Win32::UI::Shell::SEE_MASK_NOCLOSEPROCESS;
        sei.hwnd = std::ptr::null_mut();
        sei.lpVerb = verb_wide.as_ptr();
        sei.lpFile = exe_wide.as_ptr();
        sei.lpParameters = if params.is_empty() {
            std::ptr::null()
        } else {
            param_wide.as_ptr()
        };
        sei.lpDirectory = cwd_wide.as_ptr();
        sei.nShow = windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let ok = unsafe { windows_sys::Win32::UI::Shell::ShellExecuteExW(&mut sei) };
        if ok == 0 {
            let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            anyhow::bail!("请求管理员权限失败 (ShellExecuteExW 错误码: {})", err);
        }
        crate::style::debug("Windows 提权开始: ShellExecuteExW 调用成功，等待管理员子进程退出");

        if sei.hProcess.is_null() {
            anyhow::bail!("无法获取提升后进程的句柄");
        }

        unsafe {
            windows_sys::Win32::System::Threading::WaitForSingleObject(
                sei.hProcess,
                windows_sys::Win32::System::Threading::INFINITE,
            );

            let mut exit_code: u32 = 0;
            if windows_sys::Win32::System::Threading::GetExitCodeProcess(sei.hProcess, &mut exit_code) == 0
            {
                windows_sys::Win32::Foundation::CloseHandle(sei.hProcess);
                anyhow::bail!("无法获取提升后进程的退出码");
            }
            crate::style::debug(&format!(
                "Windows 提权结束: 管理员子进程退出码={}",
                exit_code
            ));

            windows_sys::Win32::Foundation::CloseHandle(sei.hProcess);

            // Windows 退出码是 u32；std::process::ExitStatus 的 from_raw 在 Windows 上接受 u32
            Ok(std::process::ExitStatus::from_raw(exit_code))
        }
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
