use crate::deploy::service::{self, find_easytier_cli, get_core_version};
use colored::Colorize;
use std::path::Path;

pub(crate) enum ExistingAction {
    Continue,
    UpdateRequested,
    Handled(anyhow::Result<()>),
}

/// 检测已有安装并提示用户操作。统一用于登录前和登录后两个路径。
pub(crate) async fn check_existing_install(install_dir: &Path) -> ExistingAction {
    let cli_path = match find_easytier_cli(install_dir) {
        Ok(p) => p,
        Err(_) => return ExistingAction::Continue,
    };

    if !service::service_is_installed(&cli_path).await {
        crate::style::debug("已有安装检测: 服务不存在，继续正常部署流程");
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
        1 => ExistingAction::Handled(do_uninstall(install_dir).await),
        2 => ExistingAction::UpdateRequested,
        3 => ExistingAction::Continue,
        _ => ExistingAction::Handled(Ok(())),
    }
}

async fn do_uninstall(install_dir: &Path) -> anyhow::Result<()> {
    crate::style::debug(&format!(
        "交互卸载开始: install_dir={}, elevated={}",
        install_dir.display(),
        crate::deploy::platform::is_elevated()
    ));
    service::run_uninstall(Some(install_dir.to_path_buf()), false).await
}
