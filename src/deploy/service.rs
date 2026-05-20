use std::path::{Path, PathBuf};

pub(crate) const SERVICE_NAME: &str = "easytier-pro-installer";

pub(crate) fn service_not_installed(output: &std::process::Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.contains("Service is not installed") || stderr.contains("Service is not installed")
}

#[cfg(windows)]
fn service_missing_in_sc(output: &std::process::Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.contains("1060")
        || stderr.contains("1060")
        || stdout.contains("does not exist as an installed service")
        || stderr.contains("does not exist as an installed service")
        || stdout.contains("指定的服务未安装")
        || stderr.contains("指定的服务未安装")
}

pub(crate) async fn service_is_installed(cli_path: &Path) -> bool {
    #[cfg(windows)]
    {
        if let Ok(output) = tokio::process::Command::new("sc")
            .args(["query", SERVICE_NAME])
            .output()
            .await
        {
            crate::style::debug(&format!("service_is_installed(sc): 退出码={:?}", output.status.code()));
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() {
                crate::style::debug(&format!("service_is_installed(sc): stdout={}", stdout.trim()));
            }
            if !stderr.trim().is_empty() {
                crate::style::debug(&format!("service_is_installed(sc): stderr={}", stderr.trim()));
            }
            if service_missing_in_sc(&output) {
                return false;
            }
            if stdout.contains("SERVICE_NAME:") || output.status.success() {
                return true;
            }
        }

        if let Ok(output) = tokio::process::Command::new(cli_path)
            .args(["service", "--name", SERVICE_NAME, "status"])
            .output()
            .await
        {
            crate::style::debug(&format!(
                "service_is_installed(cli): 退出码={:?}",
                output.status.code()
            ));
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() {
                crate::style::debug(&format!("service_is_installed(cli): stdout={}", stdout.trim()));
            }
            if !stderr.trim().is_empty() {
                crate::style::debug(&format!("service_is_installed(cli): stderr={}", stderr.trim()));
            }
            return !service_not_installed(&output);
        }

        false
    }
    #[cfg(not(windows))]
    {
        if let Ok(output) = tokio::process::Command::new(cli_path)
            .args(["service", "--name", SERVICE_NAME, "status"])
            .output()
            .await
        {
            return !service_not_installed(&output);
        }
        false
    }
}

pub(crate) async fn systemd_daemon_reload() {
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("systemctl")
            .arg("daemon-reload")
            .output()
            .await;
    }
}

pub(crate) async fn install_service(
    cli_path: &Path,
    core_path: &Path,
    config_url: &str,
) -> anyhow::Result<()> {
    if let Ok(status) = tokio::process::Command::new(cli_path)
        .args(["service", "--name", SERVICE_NAME, "status"])
        .output()
        .await
    {
        if !service_not_installed(&status) {
            let _ = tokio::process::Command::new(cli_path)
                .args(["service", "--name", SERVICE_NAME, "uninstall"])
                .output()
                .await;
            systemd_daemon_reload().await;
        }
    }

    let args = vec![
        "service".to_string(),
        "--name".to_string(),
        SERVICE_NAME.to_string(),
        "install".to_string(),
        "--core-path".to_string(),
        core_path.to_string_lossy().to_string(),
        "--".to_string(),
        "--config-server".to_string(),
        config_url.to_string(),
        "--secure-mode=true".to_string(),
    ];

    let output = tokio::process::Command::new(cli_path)
        .args(&args)
        .output()
        .await?;

    if output.status.success() {
        crate::style::kv("服务名:", SERVICE_NAME);
        crate::style::kv("程序路径:", &core_path.to_string_lossy());
        crate::style::kv("配置服务器:", config_url);
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        if stderr.contains("permission")
            || stderr.contains("Permission")
            || stderr.contains("Access")
        {
            anyhow::bail!("安装服务需要管理员权限，请使用 sudo 或管理员身份运行本程序");
        }
        anyhow::bail!("安装服务失败");
    }

    let start = tokio::process::Command::new(cli_path)
        .args(["service", "--name", SERVICE_NAME, "start"])
        .output()
        .await?;

    if start.status.success() {
        let stdout = String::from_utf8_lossy(&start.stdout);
        if !stdout.trim().is_empty() {
            println!("  {}", stdout.trim());
        }
        crate::style::success("服务已安装并启动");
    } else {
        let stderr = String::from_utf8_lossy(&start.stderr);
        println!("  {}", stderr.trim());
        crate::style::warning("服务已安装但启动失败，您可以稍后手动启动");
    }

    Ok(())
}

pub(crate) fn find_easytier_cli(install_dir: &Path) -> anyhow::Result<PathBuf> {
    let name = super::cli_binary_name();
    let path = install_dir.join(name);
    if path.exists() {
        Ok(path)
    } else {
        crate::style::debug(&format!(
            "未找到 easytier-cli: install_dir={}, expected_path={}",
            install_dir.display(),
            path.display()
        ));
        anyhow::bail!("未找到 easytier-cli，请先执行部署命令进行安装")
    }
}

pub(crate) fn get_core_version(core_path: &Path) -> Option<String> {
    let output = std::process::Command::new(core_path)
        .arg("--version")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.split_whitespace().nth(1).map(|s| s.to_string())
}

pub async fn run_status(install_dir: Option<PathBuf>) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(super::platform::default_install_dir);
    let cli_path = find_easytier_cli(&install_dir)?;

    let status = tokio::process::Command::new(&cli_path)
        .arg("service")
        .arg("--name")
        .arg(SERVICE_NAME)
        .arg("status")
        .output()
        .await?;

    println!("{}", String::from_utf8_lossy(&status.stdout).trim());
    let stderr = String::from_utf8_lossy(&status.stderr);
    if !stderr.trim().is_empty() {
        eprintln!("{}", stderr.trim());
    }
    if !status.status.success() {
        anyhow::bail!("获取服务状态失败");
    }
    Ok(())
}

pub async fn run_uninstall(install_dir: Option<PathBuf>, purge: bool) -> anyhow::Result<()> {
    #[cfg(windows)]
    if !super::platform::is_elevated() {
        crate::style::warning("卸载服务需要管理员权限，正在请求 UAC 提权...");
        let mut extra_args = Vec::new();
        let current_args: Vec<String> = std::env::args().skip(1).collect();
        if !current_args.iter().any(|arg| arg == "--uninstall") {
            extra_args.push("--uninstall");
        }
        if purge && !current_args.iter().any(|arg| arg == "--purge") {
            extra_args.push("--purge");
        }
        let status = super::platform::relaunch_elevated_with_args(&extra_args)?;
        if status.success() {
            if purge {
                crate::style::success("EasyTier 已彻底卸载并删除本地文件与缓存");
            } else {
                crate::style::success("EasyTier 服务已卸载，已保留本地文件和缓存");
            }
            std::process::exit(0);
        }
        anyhow::bail!("提权后的卸载进程执行失败，请在管理员窗口中查看详细错误");
    }

    let install_dir = install_dir.unwrap_or_else(super::platform::default_install_dir);
    crate::style::debug(&format!(
        "命令行卸载开始: install_dir={}, purge={}, elevated={}",
        install_dir.display(),
        purge,
        super::platform::is_elevated()
    ));
    match find_easytier_cli(&install_dir) {
        Ok(cli_path) => {
            crate::style::debug(&format!("命令行卸载开始: cli_path={}", cli_path.display()));
            uninstall_service(&cli_path).await?;
        }
        Err(_) => {
            crate::style::debug("命令行卸载开始: 未找到 easytier-cli，跳过服务卸载");
        }
    }

    if purge {
        remove_install_dir(&install_dir)?;
        remove_cache_dir(&super::platform::default_cache_dir())?;
        crate::style::success("EasyTier 已彻底卸载并删除本地文件与缓存");
    } else {
        crate::style::success("EasyTier 服务已卸载，已保留本地文件和缓存");
    }
    Ok(())
}

pub(crate) async fn uninstall_service(cli_path: &Path) -> anyhow::Result<()> {
    crate::style::info("正在卸载服务...");

    let stop_output = tokio::process::Command::new(cli_path)
        .args(["service", "--name", SERVICE_NAME, "stop"])
        .output()
        .await;
    match stop_output {
        Ok(o) => {
            crate::style::debug(&format!("stop 退出码={:?}", o.status.code()));
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stdout.trim().is_empty() {
                crate::style::debug(&format!("stop stdout={}", stdout.trim()));
            }
            if !stderr.trim().is_empty() {
                crate::style::debug(&format!("stop stderr={}", stderr.trim()));
            }
        }
        Err(e) => {
            crate::style::debug(&format!("stop 执行失败: {}", e));
        }
    }

    let output = tokio::process::Command::new(cli_path)
        .arg("service")
        .arg("--name")
        .arg(SERVICE_NAME)
        .arg("uninstall")
        .output()
        .await?;

    if output.status.success() {
        crate::style::debug(&format!("uninstall 退出码={:?}", output.status.code()));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.trim().is_empty() {
            crate::style::debug(&format!("uninstall stdout={}", stdout.trim()));
        }
        if !stderr.trim().is_empty() {
            crate::style::debug(&format!("uninstall stderr={}", stderr.trim()));
        }
        systemd_daemon_reload().await;
    } else {
        crate::style::debug(&format!("uninstall 退出码={:?}", output.status.code()));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.trim().is_empty() {
            crate::style::debug(&format!("uninstall stdout={}", stdout.trim()));
        }
        if service_not_installed(&output) {
            crate::style::debug("uninstall 返回 Service is not installed，按卸载成功处理");
            return Ok(());
        }
        if !stderr.trim().is_empty() {
            crate::style::debug(&format!("uninstall stderr={}", stderr.trim()));
            eprintln!("{}", stderr.trim());
        }
        anyhow::bail!("卸载服务失败");
    }

    let verify = tokio::process::Command::new(cli_path)
        .args(["service", "--name", SERVICE_NAME, "status"])
        .output()
        .await;
    if let Ok(v) = verify {
        crate::style::debug(&format!("verify 退出码={:?}", v.status.code()));
        let stdout = String::from_utf8_lossy(&v.stdout);
        let stderr = String::from_utf8_lossy(&v.stderr);
        if !stdout.trim().is_empty() {
            crate::style::debug(&format!("verify stdout={}", stdout.trim()));
        }
        if !stderr.trim().is_empty() {
            crate::style::debug(&format!("verify stderr={}", stderr.trim()));
        }
        if service_not_installed(&v) {
            crate::style::debug("verify 返回 Service is not installed，服务已卸载");
        } else {
            crate::style::warning("卸载未生效，服务仍然存在");
            anyhow::bail!("卸载未生效，请手动检查 easytier 进程");
        }
    } else {
        crate::style::debug("verify 执行失败，按卸载成功处理");
    }

    Ok(())
}

pub(crate) fn remove_install_dir(install_dir: &Path) -> anyhow::Result<()> {
    crate::style::debug(&format!("开始删除安装目录: {}", install_dir.display()));
    if !install_dir.exists() {
        crate::style::debug("安装目录不存在，跳过删除");
        return Ok(());
    }

    std::fs::remove_dir_all(install_dir).map_err(|e| {
        anyhow::anyhow!(
            "已卸载服务，但删除安装目录失败 ({}): {}",
            install_dir.display(),
            e
        )
    })?;

    crate::style::debug(&format!("安装目录删除完成: {}", install_dir.display()));
    Ok(())
}

pub(crate) fn remove_cache_dir(cache_dir: &Path) -> anyhow::Result<()> {
    crate::style::debug(&format!("开始删除缓存目录: {}", cache_dir.display()));
    if !cache_dir.exists() {
        crate::style::debug("缓存目录不存在，跳过删除");
        return Ok(());
    }

    std::fs::remove_dir_all(cache_dir).map_err(|e| {
        anyhow::anyhow!(
            "已卸载服务，但删除缓存目录失败 ({}): {}",
            cache_dir.display(),
            e
        )
    })?;

    crate::style::debug(&format!("缓存目录删除完成: {}", cache_dir.display()));
    Ok(())
}
