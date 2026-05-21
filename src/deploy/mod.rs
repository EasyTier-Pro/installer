mod check;
mod download;
mod key_select;
pub(crate) mod platform;
pub(crate) mod service;

pub(crate) use check::{ExistingAction, check_existing_install};
pub(crate) use platform::default_install_dir;
pub(crate) use service::{run_status, run_uninstall};

use crate::api::client::{
    ConsoleClient, CreateNetworkRequest, CreateNodeRequest, DeviceSummary, LatestReleaseResponse,
    NetworkSummary, TenantSummary,
};
use crate::config::Config;
use colored::Colorize;
use std::path::{Path, PathBuf};

pub(crate) fn core_binary_name() -> &'static str {
    if cfg!(windows) {
        "easytier-core.exe"
    } else {
        "easytier-core"
    }
}

pub(crate) fn cli_binary_name() -> &'static str {
    if cfg!(windows) {
        "easytier-cli.exe"
    } else {
        "easytier-cli"
    }
}

/// 检测 install_dir 是否可写。不可写时立即返回友好的错误，避免用户走完登录流程才发现。
pub(crate) fn check_install_dir_writable(install_dir: &Path) -> anyhow::Result<()> {
    if let Err(e) = std::fs::create_dir_all(install_dir) {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            anyhow::bail!(
                "无法创建安装目录 {}：权限不足。请使用 sudo 或管理员身份运行本程序",
                install_dir.display()
            );
        }
        return Err(e.into());
    }
    let test_file = install_dir.join(".write_test");
    if let Err(e) = std::fs::File::create(&test_file) {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            anyhow::bail!(
                "无法写入安装目录 {}：权限不足。请使用 sudo 或管理员身份运行本程序",
                install_dir.display()
            );
        }
        return Err(e.into());
    }
    let _ = std::fs::remove_file(&test_file);
    Ok(())
}

fn get_or_create_machine_id(install_dir: &Path) -> anyhow::Result<String> {
    let path = install_dir.join(".machine-id");
    if path.exists() {
        let id = std::fs::read_to_string(&path)?.trim().to_string();
        if !id.is_empty() {
            return Ok(id);
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    std::fs::write(&path, &id)?;
    Ok(id)
}

#[derive(Debug, Clone)]
enum NetworkAction {
    Keep,
    Join(String),
    Create,
}

async fn onboard_device(
    client: &ConsoleClient,
    tenant: &TenantSummary,
    machine_id: &str,
    recommended_region: &str,
) -> anyhow::Result<()> {
    println!();
    crate::style::info("正在等待设备注册到控制台...");

    let mut device_info: Option<DeviceSummary> = None;
    let mut joined_networks: Vec<NetworkSummary> = Vec::new();
    for attempt in 1..=6 {
        if attempt > 1 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        match client.get_machine_status(&tenant.id, machine_id).await {
            Ok(status) => {
                if status.device.approval_state == "pending" {
                    println!();
                    crate::style::warning("设备正在等待管理员审批，审批通过后会自动加入网络");
                    return Ok(());
                }
                device_info = Some(status.device);
                joined_networks = status.networks;
                println!();
                break;
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("404")
                    || msg.contains("device_not_found")
                    || msg.contains("Not Found")
                {
                    print!(".");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    crate::style::debug(&format!("第 {} 次轮询: 设备尚未注册", attempt));
                } else {
                    print!("x");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    crate::style::debug(&format!("第 {} 次轮询失败: {}", attempt, msg));
                }
            }
        }
    }

    let Some(device) = device_info else {
        crate::style::warning("设备注册超时，您可以稍后通过控制台手动将设备加入网络");
        return Ok(());
    };
    print_device_name(&device, machine_id);

    let all_networks = match client.list_networks(&tenant.id).await {
        Ok(n) => n,
        Err(e) => {
            crate::style::warning(&format!("获取网络列表失败: {}", e));
            return Ok(());
        }
    };

    let joined_ids: std::collections::HashSet<String> =
        joined_networks.iter().map(|n| n.id.clone()).collect();

    let mut options: Vec<String> = Vec::new();
    let mut actions: Vec<NetworkAction> = Vec::new();

    if !joined_networks.is_empty() {
        let names = joined_networks
            .iter()
            .map(|n| {
                if let Some(ip) = &n.node_ipv4 {
                    format!("{} ({}, 设备IP: {})", n.name, n.ipv4_cidr, ip)
                } else {
                    format!("{} ({})", n.name, n.ipv4_cidr)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        options.push(format!("[保持当前配置]（已加入：{}）", names));
        actions.push(NetworkAction::Keep);
    }

    for net in &all_networks {
        if !joined_ids.contains(&net.id) {
            options.push(format!("{} ({})", net.name, net.ipv4_cidr));
            actions.push(NetworkAction::Join(net.id.clone()));
        }
    }

    options.push("[创建新网络]".to_string());
    actions.push(NetworkAction::Create);

    let choice = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
        .with_prompt("请选择操作")
        .items(&options)
        .default(0)
        .interact()?;

    let (network_id, network_name) = match &actions[choice] {
        NetworkAction::Keep => {
            crate::style::success("配置保持不变");
            if let Ok(status) = client.get_machine_status(&tenant.id, machine_id).await
                && let Some(net) = status.networks.first()
            {
                (net.id.clone(), net.name.clone())
            } else {
                (String::new(), String::from("网络"))
            }
        }
        NetworkAction::Join(network_id) => {
            let node_req = CreateNodeRequest {
                device_id: device.id,
            };
            if let Err(e) = client.create_node(&tenant.id, network_id, &node_req).await {
                crate::style::warning(&format!("将设备加入网络失败: {}", e));
                return Ok(());
            }
            let net_name = all_networks
                .iter()
                .find(|n| n.id == *network_id)
                .map(|n| n.name.clone())
                .unwrap_or_else(|| "网络".to_string());
            crate::style::success(&format!("设备已加入网络 {}", net_name));
            (network_id.clone(), net_name)
        }
        NetworkAction::Create => {
            let name = dialoguer::Input::with_theme(&crate::style::dialoguer_theme())
                .with_prompt("请输入新网络名称")
                .interact()?;

            let regions = if recommended_region.is_empty() {
                if let Some(first_net) = all_networks.first() {
                    first_net.regions.clone()
                } else {
                    crate::style::warning("平台未就绪，无法创建网络");
                    return Ok(());
                }
            } else {
                vec![recommended_region.to_string()]
            };

            let req = CreateNetworkRequest {
                name,
                regions,
                ipv4_cidr: None,
                relay_traffic_quota_bytes: None,
            };
            let net = match client.create_network(&tenant.id, &req).await {
                Ok(n) => n,
                Err(e) => {
                    crate::style::warning(&format!("创建网络失败: {}", e));
                    return Ok(());
                }
            };
            let node_req = CreateNodeRequest {
                device_id: device.id,
            };
            if let Err(e) = client.create_node(&tenant.id, &net.id, &node_req).await {
                crate::style::warning(&format!("将设备加入网络失败: {}", e));
                return Ok(());
            }
            crate::style::success(&format!("已创建网络 {} 并将设备加入其中", net.name));
            (net.id.clone(), net.name.clone())
        }
    };

    // 等待状态同步后输出汇总信息
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    if let Ok(status) = client.get_machine_status(&tenant.id, machine_id).await
        && let Some(net) = status.networks.iter().find(|n| n.id == network_id)
    {
        let nodes = client
            .get_network_nodes(&tenant.id, &network_id)
            .await
            .unwrap_or_default();
        println!();
        crate::style::info("设备已成功配置：");
        print_device_name(&status.device, machine_id);
        println!("  {} {}", "网络名称:".bold(), network_name);
        if let Some(ip) = &net.node_ipv4 {
            println!("  {} {}", "虚拟 IP:".bold(), ip);
        }
        println!("  {} {}", "网络网段:".bold(), &net.ipv4_cidr);
        println!("  {} {}台", "网络节点:".bold(), nodes.len());
        println!(
            "  {} {}",
            "控制台地址:".bold(),
            "https://console.easytier.net"
        );
    }

    Ok(())
}

pub(crate) async fn run_deploy(
    config: &Config,
    client: &ConsoleClient,
    tenant: &TenantSummary,
    latest_release: &LatestReleaseResponse,
    install_dir: Option<PathBuf>,
    config_server_base: Option<String>,
    version_override: Option<String>,
) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(default_install_dir);
    std::fs::create_dir_all(&install_dir)?;

    let machine_id = get_or_create_machine_id(&install_dir)?;
    crate::style::debug(&format!("machine_id: {}", machine_id));

    // 2. 获取 enrollment key
    let keys = client.list_device_enrollment_keys(&tenant.id).await?;
    let active_keys: Vec<_> = keys
        .into_iter()
        .filter(|k| !k.revoked && k.lifecycle_state != "expired")
        .collect();

    let bootstrap_token = if active_keys.is_empty() {
        if !key_select::confirm_yes("当前没有可用密钥，是否创建一个用于本次部署")?
        {
            anyhow::bail!("没有可用的注册密钥，部署已取消。您可以前往 Console 手动创建。");
        }
        let (key, token) = key_select::create_new_key(client, &tenant.id).await?;
        let label = key_select::key_type_label(key.reusable);
        crate::style::ok_kv(
            "注册密钥:",
            &format!("{} [{}]", key_select::key_name(&key), label),
        );
        token
    } else if active_keys.len() == 1 {
        let key = active_keys.into_iter().next().unwrap();
        let name = key_select::key_name(&key).to_string();
        let label = key_select::key_type_label(key.reusable);
        if key_select::confirm_yes(&format!("是否使用{}密钥 {} 进行部署", label, name))? {
            match key_select::get_key_token(client, &tenant.id, &key).await {
                Ok(token) => {
                    crate::style::ok_kv("注册密钥:", &format!("{} [{}]", name, label));
                    token
                }
                Err(err) if key_select::is_key_secret_unavailable(&err) => {
                    crate::style::warning(&format!(
                        "密钥 {} 当前无法用于部署，请创建新密钥。",
                        name
                    ));
                    let (key, token) = key_select::create_new_key(client, &tenant.id).await?;
                    let new_label = key_select::key_type_label(key.reusable);
                    crate::style::ok_kv(
                        "注册密钥:",
                        &format!("{} [{}]", key_select::key_name(&key), new_label),
                    );
                    token
                }
                Err(err) => return Err(err),
            }
        } else {
            let (key, token) = key_select::create_new_key(client, &tenant.id).await?;
            let new_label = key_select::key_type_label(key.reusable);
            crate::style::ok_kv(
                "注册密钥:",
                &format!("{} [{}]", key_select::key_name(&key), new_label),
            );
            token
        }
    } else {
        let multi_keys: Vec<_> = active_keys.iter().filter(|k| k.reusable).cloned().collect();
        let single_keys: Vec<_> = active_keys
            .iter()
            .filter(|k| !k.reusable)
            .cloned()
            .collect();
        let (token, key) =
            key_select::select_key(client, &tenant.id, &multi_keys, &single_keys).await?;
        let label = key_select::key_type_label(key.reusable);
        crate::style::ok_kv(
            "注册密钥:",
            &format!("{} [{}]", key_select::key_name(&key), label),
        );
        token
    };

    // 3. 构造 config server URL
    let config_server = platform::build_config_server_url(
        &config.console_base_url,
        config_server_base,
        &latest_release.web_config_server_url,
    )?;
    let full_config_url = format!(
        "{}/{}",
        config_server.trim_end_matches('/'),
        bootstrap_token
    );
    crate::style::kv("配置服务器:", &full_config_url);
    println!();

    // 4. 检测平台并下载 easytier
    let platform = platform::detect_platform()?;
    let stable_version = &latest_release.stable.version;
    let download_version = resolve_version(stable_version, version_override)?;
    crate::style::info(&format!(
        "正在下载 easytier {} ({}-{})...",
        download_version.bright_white(),
        platform.os,
        platform.arch
    ));
    crate::style::kv("安装目录:", &install_dir.to_string_lossy());
    crate::style::kv(
        "缓存目录:",
        &platform::default_cache_dir().to_string_lossy(),
    );

    let (core_path, cli_path, _installed_version) =
        download::download_easytier_with_fallback(&platform, &install_dir, &download_version)
            .await?;
    crate::style::success("下载完成");

    println!();
    crate::style::info("正在安装并启动服务...");

    #[cfg(windows)]
    if !platform::is_elevated() {
        crate::style::warning("安装服务需要管理员权限，正在请求 UAC 提权...");
        let core_path_str = core_path.to_string_lossy().to_string();
        let install_dir_str = install_dir.to_string_lossy().to_string();
        let mut extra_args = vec![
            "install-service",
            "--service-core-path",
            &core_path_str,
            "--service-config-url",
            &full_config_url,
            "--install-dir",
            &install_dir_str,
        ];
        if !machine_id.is_empty() {
            extra_args.push("--service-machine-id");
            extra_args.push(&machine_id);
        }
        let status = platform::relaunch_elevated_with_replaced_args(&extra_args)?;
        if !status.success() {
            anyhow::bail!("提权后的安装服务进程执行失败，请在管理员窗口中查看详细错误");
        }
    } else {
        service::install_service(&cli_path, &core_path, &full_config_url, Some(&machine_id))
            .await?;
    }

    #[cfg(not(windows))]
    {
        service::install_service(&cli_path, &core_path, &full_config_url, Some(&machine_id))
            .await?;
    }

    println!();
    crate::style::success(&format!(
        "{} 部署完成，正在运行。",
        "EasyTier".bright_white()
    ));

    onboard_device(client, tenant, &machine_id, "").await?;

    Ok(())
}

pub(crate) async fn run_upgrade_from_console(
    install_dir: &Path,
    release: &LatestReleaseResponse,
    version_override: Option<String>,
) -> anyhow::Result<()> {
    let stable_version = &release.stable.version;
    let target_version = resolve_version(stable_version, version_override)?;
    crate::style::debug(&format!("准备升级: target_version={}", target_version));
    run_upgrade(install_dir, &target_version).await
}

pub(crate) async fn run_upgrade(install_dir: &Path, target_version: &str) -> anyhow::Result<()> {
    let platform = platform::detect_platform()?;
    let target_version = download::normalize_version(target_version);

    let core_path = install_dir.join(core_binary_name());
    let current_version =
        service::get_core_version(&core_path).unwrap_or_else(|| "未知".to_string());

    if current_version == target_version {
        crate::style::info(&format!(
            "当前已是最新版本 {}",
            target_version.bright_white()
        ));
        return Ok(());
    }

    crate::style::info(&format!(
        "正在从 {} 升级至 {}...",
        current_version.bright_white(),
        target_version.bright_white()
    ));

    #[cfg(windows)]
    if !platform::is_elevated() {
        crate::style::warning("升级服务需要管理员权限，正在请求 UAC 提权...");
        let version_arg = target_version.clone();
        let install_dir_arg = install_dir.to_string_lossy().to_string();
        let status = platform::relaunch_elevated_with_replaced_args(&[
            "upgrade-service",
            "--version",
            &version_arg,
            "--install-dir",
            &install_dir_arg,
        ])?;
        if status.success() {
            crate::style::success("服务已重启");
            println!();
            crate::style::success(&format!(
                "{} 已升级至 {}，正在运行。",
                "EasyTier".bright_white(),
                target_version.bright_white()
            ));
            return Ok(());
        }
        anyhow::bail!("提权后的升级进程执行失败，请在管理员窗口中查看详细错误");
    }

    let current_cli_path = service::find_easytier_cli(install_dir)?;
    println!();
    crate::style::info("正在停止服务...");
    let stop = tokio::process::Command::new(&current_cli_path)
        .args(["service", "--name", service::SERVICE_NAME, "stop"])
        .output()
        .await?;
    if !stop.status.success() {
        let stderr = String::from_utf8_lossy(&stop.stderr);
        if !stderr.is_empty() {
            println!("  {}", stderr.trim());
        }
        anyhow::bail!("停止服务失败，无法继续升级");
    }
    crate::style::success("服务已停止");

    let (_, cli_path, _) =
        download::download_easytier_with_fallback(&platform, install_dir, &target_version).await?;
    crate::style::success(&format!("已下载 {}", target_version));

    println!();
    crate::style::info("正在启动服务...");
    let start = tokio::process::Command::new(&cli_path)
        .args(["service", "--name", service::SERVICE_NAME, "start"])
        .output()
        .await?;

    if start.status.success() {
        crate::style::success("服务已重启");
    } else {
        let stderr = String::from_utf8_lossy(&start.stderr);
        if !stderr.is_empty() {
            println!("  {}", stderr.trim());
        }
        crate::style::warning("启动失败，请尝试重新部署");
    }

    println!();
    crate::style::success(&format!(
        "{} 已升级至 {}，正在运行。",
        "EasyTier".bright_white(),
        target_version.bright_white()
    ));
    Ok(())
}

pub(crate) async fn load_console_bootstrap(
    client: &ConsoleClient,
) -> anyhow::Result<(TenantSummary, LatestReleaseResponse)> {
    let me = client.get_me().await?;
    if me.tenants.is_empty() {
        anyhow::bail!("您不属于任何工作空间，无法部署设备");
    }

    let tenant = if me.tenants.len() == 1 {
        let t = me.tenants.into_iter().next().unwrap();
        crate::style::ok_kv("工作空间:", &t.name);
        t
    } else {
        let tenant_names: Vec<String> = me.tenants.iter().map(|t| t.name.clone()).collect();
        let idx = key_select::read_choice(&tenant_names, "请选择要部署到的工作空间")? - 1;
        let t = me.tenants.into_iter().nth(idx).unwrap();
        crate::style::ok_kv("工作空间:", &t.name);
        t
    };

    let latest_release = client.get_latest_release().await?;
    Ok((tenant, latest_release))
}

fn resolve_version(
    stable_version: &str,
    version_override: Option<String>,
) -> anyhow::Result<String> {
    if let Some(version) = version_override {
        Ok(download::normalize_version(&version))
    } else if !stable_version.is_empty() {
        Ok(download::normalize_version(stable_version))
    } else {
        anyhow::bail!("Console 未返回可用版本，无法继续下载")
    }
}

fn print_device_name(device: &DeviceSummary, machine_id: &str) {
    let name = device.hostname.trim();
    if !name.is_empty() {
        crate::style::ok_kv("设备名称:", name);
    } else {
        crate::style::ok_kv("设备名称:", machine_id);
    }
}
