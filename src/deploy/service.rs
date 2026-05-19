use std::path::{Path, PathBuf};

pub(crate) const SERVICE_NAME: &str = "easytier-pro-installer";

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
        let stdout = String::from_utf8_lossy(&status.stdout);
        if !stdout.contains("Service is not installed") {
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

pub async fn run_uninstall(install_dir: Option<PathBuf>) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(super::platform::default_install_dir);
    let cli_path = find_easytier_cli(&install_dir)?;

    let output = tokio::process::Command::new(&cli_path)
        .arg("service")
        .arg("--name")
        .arg(SERVICE_NAME)
        .arg("uninstall")
        .output()
        .await?;

    if output.status.success() {
        systemd_daemon_reload().await;
        crate::style::success("EasyTier 服务已卸载");
    } else {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        anyhow::bail!("卸载服务失败");
    }
    Ok(())
}
