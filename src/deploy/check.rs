use crate::deploy::service::{self, find_easytier_cli, get_core_version};
use colored::Colorize;
use std::path::Path;

pub(crate) enum ExistingAction {
    Continue,
    Handled(anyhow::Result<()>),
}

/// 检测已有安装并提示用户操作。统一用于登录前和登录后两个路径。
pub(crate) async fn check_existing_install(
    install_dir: &Path,
    version_override: Option<String>,
) -> ExistingAction {
    let cli_path = match find_easytier_cli(install_dir) {
        Ok(p) => p,
        Err(_) => return ExistingAction::Continue,
    };

    let status_output = match tokio::process::Command::new(&cli_path)
        .args(["service", "--name", service::SERVICE_NAME, "status"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(_) => return ExistingAction::Continue,
    };
    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    if status_stdout.contains("Service is not installed") {
        return ExistingAction::Continue;
    }

    let core_path = install_dir.join(super::core_binary_name());
    let version = get_core_version(&core_path).unwrap_or_else(|| "未知版本".to_string());
    crate::style::info(&format!(
        "检测到 EasyTier {} 已安装",
        version.bright_white()
    ));
    println!();

    let items = vec!["取消", "卸载", "更新软件", "重新部署"];
    let choice = match dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
        .with_prompt("检测到已有安装，请选择操作")
        .items(&items)
        .default(0)
        .interact()
    {
        Ok(c) => c,
        Err(_) => return ExistingAction::Handled(Ok(())),
    };

    match choice {
        0 => ExistingAction::Handled(Ok(())),
        1 => ExistingAction::Handled(do_uninstall(&cli_path).await),
        2 => ExistingAction::Handled(super::run_upgrade(install_dir, version_override).await),
        3 => ExistingAction::Continue,
        _ => ExistingAction::Handled(Ok(())),
    }
}

async fn do_uninstall(cli_path: &Path) -> anyhow::Result<()> {
    crate::style::info("正在卸载服务...");
    let _ = tokio::process::Command::new(cli_path)
        .args(["service", "--name", service::SERVICE_NAME, "stop"])
        .output()
        .await;
    let output = tokio::process::Command::new(cli_path)
        .args(["service", "--name", service::SERVICE_NAME, "uninstall"])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stderr.contains("No such file or directory") {
                if !stderr.is_empty() {
                    eprintln!("{}", stderr.trim());
                }
                return Err(anyhow::anyhow!(
                    "卸载服务失败，请使用 sudo 或管理员身份运行"
                ));
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("卸载服务失败: {}", e));
        }
    }

    service::systemd_daemon_reload().await;

    let verify = tokio::process::Command::new(cli_path)
        .args(["service", "--name", service::SERVICE_NAME, "status"])
        .output()
        .await;
    if let Ok(v) = verify {
        let stdout = String::from_utf8_lossy(&v.stdout);
        if stdout.contains("Service is not installed") {
            crate::style::success("EasyTier 服务已卸载");
        } else {
            crate::style::warning("卸载未生效，服务仍然存在");
            return Err(anyhow::anyhow!("卸载未生效，请手动检查 easytier 进程"));
        }
    } else {
        crate::style::success("EasyTier 服务已卸载");
    }
    Ok(())
}
