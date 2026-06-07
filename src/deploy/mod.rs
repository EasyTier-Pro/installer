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
use fs2::FileExt;
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

type DesktopEventEmitter<'a> =
    &'a mut dyn FnMut(&'static str, serde_json::Value) -> anyhow::Result<()>;

#[derive(Debug)]
struct DesktopLifecycleLock {
    _file: std::fs::File,
}

fn hold_desktop_lifecycle_lock(_lock: &DesktopLifecycleLock) {}

struct DesktopPreparedUpdate {
    staging_dir: PathBuf,
    version: String,
}

impl Drop for DesktopPreparedUpdate {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.staging_dir);
    }
}

#[cfg(windows)]
struct DesktopSecretFile {
    path: PathBuf,
}

#[cfg(windows)]
impl DesktopSecretFile {
    fn create(contents: &str) -> anyhow::Result<Self> {
        let dir = desktop_lifecycle_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!(
            "desktop-service-config-{}.secret",
            uuid::Uuid::new_v4()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        drop(file);
        service::restrict_sensitive_file_permissions(&path)?;
        Ok(Self { path })
    }
}

#[cfg(windows)]
impl Drop for DesktopSecretFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn desktop_lifecycle_dir() -> PathBuf {
    platform::default_cache_dir()
        .parent()
        .map(|path| path.join("locks"))
        .unwrap_or_else(|| std::env::temp_dir().join("easytier-pro-installer-locks"))
}

fn acquire_desktop_lifecycle_lock(install_dir: &Path) -> anyhow::Result<DesktopLifecycleLock> {
    let lock_dir = desktop_lifecycle_dir();
    acquire_desktop_lifecycle_lock_in(&lock_dir, install_dir)
}

fn acquire_desktop_lifecycle_lock_in_optional_dir(
    install_dir: &Path,
    lock_dir: Option<&Path>,
) -> anyhow::Result<DesktopLifecycleLock> {
    if let Some(lock_dir) = lock_dir {
        acquire_desktop_lifecycle_lock_in(lock_dir, install_dir)
    } else {
        acquire_desktop_lifecycle_lock(install_dir)
    }
}

fn acquire_desktop_lifecycle_lock_in(
    lock_dir: &Path,
    install_dir: &Path,
) -> anyhow::Result<DesktopLifecycleLock> {
    std::fs::create_dir_all(lock_dir)?;
    let lock_path = lock_dir.join("desktop-lifecycle.lock");
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;

    if let Err(err) = file.try_lock_exclusive() {
        if err.kind() == std::io::ErrorKind::WouldBlock {
            anyhow::bail!("已有桌面端安装、更新或卸载任务正在运行");
        }
        return Err(err.into());
    }

    file.set_len(0)?;
    file.seek(std::io::SeekFrom::Start(0))?;
    writeln!(file, "pid={}", std::process::id())?;
    writeln!(file, "install_dir={}", install_dir.display())?;
    let _ = file.sync_data();
    Ok(DesktopLifecycleLock { _file: file })
}

pub(crate) fn ensure_desktop_purge_safe(
    install_dir: &Path,
    active_lock_dir: Option<&Path>,
) -> anyhow::Result<()> {
    let install_dir = normalize_for_overlap_check(install_dir)?;
    let default_lock_dir;
    let active_lock_dir = match active_lock_dir {
        Some(lock_dir) => lock_dir,
        None => {
            default_lock_dir = desktop_lifecycle_dir();
            &default_lock_dir
        }
    };
    let lock_dir = normalize_for_overlap_check(active_lock_dir)?;
    if lock_dir.starts_with(&install_dir) {
        anyhow::bail!("install_dir 不能是桌面端生命周期锁目录或其上级目录");
    }
    Ok(())
}

fn normalize_for_overlap_check(path: &Path) -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::fs::canonicalize(path) {
        return Ok(path);
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

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

fn read_machine_id(install_dir: &Path) -> Option<String> {
    let id = std::fs::read_to_string(install_dir.join(".machine-id"))
        .ok()?
        .trim()
        .to_string();
    if id.is_empty() { None } else { Some(id) }
}

fn service_config_matches(binary_path: Option<&String>, expected_config_url: &str) -> Option<bool> {
    let binary_path = binary_path?;
    if binary_path.contains(expected_config_url) {
        Some(true)
    } else if binary_path.contains("--config-server") {
        Some(false)
    } else {
        None
    }
}

fn machine_id_from_service_args(binary_path: Option<&String>) -> Option<String> {
    let binary_path = binary_path?;
    let mut parts = binary_path.split_whitespace();
    while let Some(part) = parts.next() {
        if let Some(value) = part.strip_prefix("--machine-id=")
            && !value.trim().is_empty()
        {
            return Some(value.trim().to_string());
        }
        if part == "--machine-id" {
            let value = parts.next()?.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn version_matches(installed: Option<&str>, target: &str) -> bool {
    let Some(installed) = installed else {
        return false;
    };
    let installed = installed.trim().trim_start_matches('v');
    let target = target.trim().trim_start_matches('v');
    installed == target || installed.starts_with(&format!("{target}-"))
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
        println!("  {} https://console.easytier.net", "控制台地址:".bold());
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

pub(crate) async fn run_desktop_status(
    install_dir: Option<PathBuf>,
    config_server: Option<String>,
    bootstrap_token: Option<String>,
    version: Option<String>,
    emit: DesktopEventEmitter<'_>,
) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(default_install_dir);
    let core_path = install_dir.join(core_binary_name());
    let cli_path = install_dir.join(cli_binary_name());
    let binaries_present = core_path.exists() && cli_path.exists();
    let installed_version = if core_path.exists() {
        service::get_core_version(&core_path)
    } else {
        None
    };
    let existing_fingerprint = service::bootstrap_fingerprint(&install_dir);
    let service_status = service::query_service_status(
        &install_dir,
        if cli_path.exists() {
            Some(&cli_path)
        } else {
            None
        },
    )
    .await;
    let machine_id = read_machine_id(&install_dir)
        .or_else(|| machine_id_from_service_args(service_status.binary_path.as_ref()));

    let expected_fingerprint = bootstrap_token
        .as_deref()
        .map(service::bootstrap_fingerprint_for_token);
    let identity_match = expected_fingerprint
        .as_deref()
        .map(|expected| existing_fingerprint.as_deref() == Some(expected));
    let target_version = version.map(|value| download::normalize_version(&value));
    let version_match = target_version
        .as_deref()
        .map(|target| version_matches(installed_version.as_deref(), target));
    let expected_config_url = config_server.as_ref().and_then(|base| {
        bootstrap_token
            .as_deref()
            .map(|token| format!("{}/{}", base.trim_end_matches('/'), token))
    });
    let config_server_match = expected_config_url
        .as_ref()
        .and_then(|expected| service_config_matches(service_status.binary_path.as_ref(), expected));
    let ready = service_status.installed
        && service_status.running
        && binaries_present
        && identity_match.unwrap_or(true)
        && version_match.unwrap_or(true)
        && config_server_match.unwrap_or(true);

    emit(
        "finished",
        serde_json::json!({
            "install_dir": install_dir.to_string_lossy(),
            "service_name": service::SERVICE_NAME,
            "installed": service_status.installed,
            "running": service_status.running,
            "service_state": service_status.state,
            "binary_path": service_status.binary_path,
            "machine_id": machine_id,
            "core_path": core_path.to_string_lossy(),
            "cli_path": cli_path.to_string_lossy(),
            "binaries_present": binaries_present,
            "version": installed_version,
            "target_version": target_version,
            "bootstrap_fingerprint": existing_fingerprint,
            "identity_match": identity_match,
            "version_match": version_match,
            "config_server_match": config_server_match,
            "ready": ready,
        }),
    )?;

    Ok(())
}

pub(crate) async fn run_desktop_install(
    install_dir: Option<PathBuf>,
    config_server: String,
    bootstrap_token: String,
    version: String,
    emit: DesktopEventEmitter<'_>,
) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(default_install_dir);
    let lifecycle_lock = acquire_desktop_lifecycle_lock(&install_dir)?;
    hold_desktop_lifecycle_lock(&lifecycle_lock);

    emit(
        "started",
        serde_json::json!({
            "install_dir": install_dir.to_string_lossy(),
        }),
    )?;

    let mut machine_id = read_machine_id(&install_dir).unwrap_or_default();
    let full_config_url = format!(
        "{}/{}",
        config_server.trim_end_matches('/'),
        bootstrap_token
    );
    let expected_fingerprint = service::bootstrap_fingerprint_for_token(&bootstrap_token);
    let existing_fingerprint = service::bootstrap_fingerprint(&install_dir);
    let normalized_version = download::normalize_version(&version);

    let platform = platform::detect_platform()?;
    emit(
        "platform_detected",
        serde_json::json!({
            "os": platform.os,
            "arch": platform.arch,
        }),
    )?;

    let existing_core_path = install_dir.join(core_binary_name());
    let existing_cli_path = install_dir.join(cli_binary_name());
    let binaries_present = existing_core_path.exists() && existing_cli_path.exists();
    let existing_version = if binaries_present {
        service::get_core_version(&existing_core_path)
    } else {
        None
    };
    let identity_match = existing_fingerprint.as_deref() == Some(expected_fingerprint.as_str());
    let service_status = service::query_service_status(
        &install_dir,
        if existing_cli_path.exists() {
            Some(&existing_cli_path)
        } else {
            None
        },
    )
    .await;
    if machine_id.is_empty()
        && let Some(id) = machine_id_from_service_args(service_status.binary_path.as_ref())
    {
        machine_id = id;
    }
    let config_server_match =
        service_config_matches(service_status.binary_path.as_ref(), &full_config_url)
            .unwrap_or(true);

    emit(
        "identity_evaluated",
        serde_json::json!({
            "identity_match": identity_match,
            "binaries_present": binaries_present,
            "installed_version": existing_version.clone(),
            "target_version": normalized_version.clone(),
            "service_installed": service_status.installed,
            "service_running": service_status.running,
            "config_server_match": config_server_match,
        }),
    )?;

    if identity_match
        && binaries_present
        && version_matches(existing_version.as_deref(), &normalized_version)
        && service_status.installed
        && service_status.running
        && config_server_match
        && !machine_id.is_empty()
    {
        emit(
            "service_installing",
            serde_json::json!({
                "mode": "reuse_existing",
            }),
        )?;
        emit(
            "service_started",
            serde_json::json!({
                "service_name": service::SERVICE_NAME,
                "reused": true,
            }),
        )?;
        emit(
            "finished",
            serde_json::json!({
                "machine_id": machine_id,
                "install_dir": install_dir.to_string_lossy(),
                "core_path": existing_core_path.to_string_lossy(),
                "cli_path": existing_cli_path.to_string_lossy(),
                "version": normalized_version,
                "reused": true,
            }),
        )?;
        return Ok(());
    }

    check_install_dir_writable(&install_dir)?;
    std::fs::create_dir_all(&install_dir)?;
    if machine_id.is_empty() {
        machine_id = get_or_create_machine_id(&install_dir)?;
    }

    let (core_path, cli_path, installed_version) =
        download_with_desktop_events(&platform, &install_dir, &normalized_version, emit).await?;

    emit("service_installing", serde_json::json!({}))?;
    install_desktop_service(
        &install_dir,
        &core_path,
        &cli_path,
        &full_config_url,
        &machine_id,
    )
    .await?;
    emit(
        "service_started",
        serde_json::json!({
            "service_name": service::SERVICE_NAME,
            "reused": false,
        }),
    )?;

    emit(
        "finished",
        serde_json::json!({
            "machine_id": machine_id,
            "install_dir": install_dir.to_string_lossy(),
            "core_path": core_path.to_string_lossy(),
            "cli_path": cli_path.to_string_lossy(),
            "version": installed_version,
            "reused": false,
        }),
    )?;

    Ok(())
}

async fn install_desktop_service(
    install_dir: &Path,
    core_path: &Path,
    cli_path: &Path,
    full_config_url: &str,
    machine_id: &str,
) -> anyhow::Result<()> {
    #[cfg(windows)]
    if !platform::is_elevated() {
        let lock_dir = desktop_lifecycle_dir();
        let service_config = DesktopSecretFile::create(full_config_url)?;
        let core_path_str = core_path.to_string_lossy().to_string();
        let install_dir_str = install_dir.to_string_lossy().to_string();
        let service_config_path = service_config.path.to_string_lossy().to_string();
        let lock_dir_str = lock_dir.to_string_lossy().to_string();
        let mut extra_args = vec![
            "install-service",
            "--service-core-path",
            &core_path_str,
            "--service-config-url-file",
            &service_config_path,
            "--service-strict-start",
            "--desktop-lock-dir",
            &lock_dir_str,
            "--desktop-parent-lock-held",
            "--install-dir",
            &install_dir_str,
        ];
        if !machine_id.is_empty() {
            extra_args.push("--service-machine-id");
            extra_args.push(machine_id);
        }
        let status = platform::relaunch_elevated_with_replaced_args_hidden(&extra_args)?;
        if !status.success() {
            anyhow::bail!("提权后的安装服务进程执行失败，请在管理员窗口中查看详细错误");
        }
        return Ok(());
    }

    #[cfg(windows)]
    {
        service::install_service_quiet(cli_path, core_path, full_config_url, Some(machine_id)).await
    }

    #[cfg(not(windows))]
    {
        service::install_service_quiet(cli_path, core_path, full_config_url, Some(machine_id)).await
    }
}

pub(crate) async fn run_desktop_install_service(
    install_dir: PathBuf,
    core_path: PathBuf,
    config_url: &str,
    machine_id: Option<&str>,
    lock_dir: Option<PathBuf>,
    parent_lock_held: bool,
) -> anyhow::Result<()> {
    let lifecycle_lock = if parent_lock_held {
        None
    } else {
        Some(acquire_desktop_lifecycle_lock_in_optional_dir(
            &install_dir,
            lock_dir.as_deref(),
        )?)
    };
    if let Some(lifecycle_lock) = &lifecycle_lock {
        hold_desktop_lifecycle_lock(lifecycle_lock);
    }
    let cli_path = service::find_easytier_cli(&install_dir)?;
    service::install_service_quiet(&cli_path, &core_path, config_url, machine_id).await
}

pub(crate) async fn run_desktop_update(
    install_dir: Option<PathBuf>,
    target_version: &str,
    emit: DesktopEventEmitter<'_>,
) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(default_install_dir);
    let lifecycle_lock = acquire_desktop_lifecycle_lock(&install_dir)?;
    hold_desktop_lifecycle_lock(&lifecycle_lock);
    let platform = platform::detect_platform()?;
    let target_version = download::normalize_version(target_version);
    let current_cli_path = service::find_easytier_cli(&install_dir)?;

    emit(
        "started",
        serde_json::json!({
            "install_dir": install_dir.to_string_lossy(),
            "version": target_version,
        }),
    )?;
    emit(
        "platform_detected",
        serde_json::json!({
            "os": platform.os,
            "arch": platform.arch,
        }),
    )?;

    let core_path = install_dir.join(core_binary_name());
    let current_version = service::get_core_version(&core_path);
    if version_matches(current_version.as_deref(), &target_version) {
        emit(
            "finished",
            serde_json::json!({
                "install_dir": install_dir.to_string_lossy(),
                "version": target_version,
                "up_to_date": true,
            }),
        )?;
        return Ok(());
    }

    #[cfg(windows)]
    if !platform::is_elevated() {
        let lock_dir = desktop_lifecycle_dir();
        let install_dir_arg = install_dir.to_string_lossy().to_string();
        let lock_dir_arg = lock_dir.to_string_lossy().to_string();
        let status = platform::relaunch_elevated_with_replaced_args_hidden(&[
            "upgrade-service",
            "--version",
            &target_version,
            "--service-strict-start",
            "--desktop-lock-dir",
            &lock_dir_arg,
            "--desktop-parent-lock-held",
            "--install-dir",
            &install_dir_arg,
        ])?;
        if !status.success() {
            anyhow::bail!("提权后的升级进程执行失败，请在管理员窗口中查看详细错误");
        }
        emit(
            "service_started",
            serde_json::json!({
                "service_name": service::SERVICE_NAME,
            }),
        )?;
        emit(
            "finished",
            serde_json::json!({
                "install_dir": install_dir.to_string_lossy(),
                "version": target_version,
                "up_to_date": false,
            }),
        )?;
        return Ok(());
    }

    let package =
        prepare_desktop_update_package_with_events(&platform, &install_dir, &target_version, emit)
            .await?;

    stop_service_for_update(&current_cli_path).await?;
    let update_result = async {
        let (_, cli_path) = install_prepared_desktop_update(&package, &install_dir)?;
        start_service_strict(&cli_path).await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if let Err(err) = update_result {
        restart_existing_service_after_failed_update(&install_dir, &current_cli_path).await;
        return Err(err);
    }
    emit(
        "service_started",
        serde_json::json!({
            "service_name": service::SERVICE_NAME,
        }),
    )?;
    emit(
        "finished",
        serde_json::json!({
            "install_dir": install_dir.to_string_lossy(),
            "version": package.version.clone(),
            "up_to_date": false,
        }),
    )?;

    Ok(())
}

pub(crate) async fn run_desktop_update_service(
    install_dir: PathBuf,
    target_version: &str,
    lock_dir: Option<PathBuf>,
    parent_lock_held: bool,
) -> anyhow::Result<()> {
    let lifecycle_lock = if parent_lock_held {
        None
    } else {
        Some(acquire_desktop_lifecycle_lock_in_optional_dir(
            &install_dir,
            lock_dir.as_deref(),
        )?)
    };
    if let Some(lifecycle_lock) = &lifecycle_lock {
        hold_desktop_lifecycle_lock(lifecycle_lock);
    }
    let platform = platform::detect_platform()?;
    let target_version = download::normalize_version(target_version);
    let current_cli_path = service::find_easytier_cli(&install_dir)?;

    let core_path = install_dir.join(core_binary_name());
    let current_version = service::get_core_version(&core_path);
    if version_matches(current_version.as_deref(), &target_version) {
        return Ok(());
    }

    let package =
        prepare_desktop_update_package_quiet(&platform, &install_dir, &target_version).await?;

    stop_service_for_update(&current_cli_path).await?;
    let update_result = async {
        let (_, cli_path) = install_prepared_desktop_update(&package, &install_dir)?;
        start_service_strict(&cli_path).await
    }
    .await;
    if let Err(err) = update_result {
        restart_existing_service_after_failed_update(&install_dir, &current_cli_path).await;
        return Err(err);
    }
    Ok(())
}

pub(crate) async fn run_desktop_uninstall_service(
    install_dir: PathBuf,
    purge: bool,
    lock_dir: Option<PathBuf>,
    parent_lock_held: bool,
) -> anyhow::Result<()> {
    let lifecycle_lock = if parent_lock_held {
        None
    } else {
        Some(acquire_desktop_lifecycle_lock_in_optional_dir(
            &install_dir,
            lock_dir.as_deref(),
        )?)
    };
    if let Some(lifecycle_lock) = &lifecycle_lock {
        hold_desktop_lifecycle_lock(lifecycle_lock);
    }
    if let Ok(cli_path) = service::find_easytier_cli(&install_dir) {
        service::uninstall_service_quiet(&cli_path).await?;
    }
    if purge {
        purge_desktop_install(&install_dir, lock_dir.as_deref())?;
    }
    Ok(())
}

pub(crate) async fn run_desktop_uninstall(
    install_dir: Option<PathBuf>,
    purge: bool,
    emit: DesktopEventEmitter<'_>,
) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(default_install_dir);
    let lifecycle_lock = acquire_desktop_lifecycle_lock(&install_dir)?;
    hold_desktop_lifecycle_lock(&lifecycle_lock);
    emit(
        "started",
        serde_json::json!({
            "install_dir": install_dir.to_string_lossy(),
            "purge": purge,
        }),
    )?;

    #[cfg(windows)]
    if !platform::is_elevated() {
        let lock_dir = desktop_lifecycle_dir();
        let install_dir_arg = install_dir.to_string_lossy().to_string();
        let lock_dir_arg = lock_dir.to_string_lossy().to_string();
        let mut extra_args = vec!["uninstall-service"];
        if purge {
            extra_args.push("--purge");
        }
        extra_args.push("--desktop-lock-dir");
        extra_args.push(&lock_dir_arg);
        extra_args.push("--desktop-parent-lock-held");
        extra_args.push("--install-dir");
        extra_args.push(&install_dir_arg);
        let status = platform::relaunch_elevated_with_replaced_args_hidden(&extra_args)?;
        if !status.success() {
            anyhow::bail!("提权后的卸载进程执行失败，请在管理员窗口中查看详细错误");
        }
        emit("service_uninstalled", serde_json::json!({}))?;
        emit(
            "finished",
            serde_json::json!({
                "install_dir": install_dir.to_string_lossy(),
                "purged": purge,
            }),
        )?;
        return Ok(());
    }

    emit("service_uninstalling", serde_json::json!({}))?;
    if let Ok(cli_path) = service::find_easytier_cli(&install_dir) {
        service::uninstall_service_quiet(&cli_path).await?;
    }
    emit("service_uninstalled", serde_json::json!({}))?;

    if purge {
        purge_desktop_install(&install_dir, None)?;
    }

    emit(
        "finished",
        serde_json::json!({
            "install_dir": install_dir.to_string_lossy(),
            "purged": purge,
        }),
    )?;

    Ok(())
}

fn purge_desktop_install(install_dir: &Path, active_lock_dir: Option<&Path>) -> anyhow::Result<()> {
    ensure_desktop_purge_safe(install_dir, active_lock_dir)?;
    service::remove_install_dir(install_dir)?;
    service::remove_cache_dir(&platform::default_cache_dir())
}

async fn download_with_desktop_events(
    platform: &platform::Platform,
    install_dir: &Path,
    version: &str,
    emit: DesktopEventEmitter<'_>,
) -> anyhow::Result<(PathBuf, PathBuf, String)> {
    download_with_desktop_events_to_dir(platform, install_dir, install_dir, version, emit).await
}

async fn download_with_desktop_events_to_dir(
    platform: &platform::Platform,
    download_dir: &Path,
    event_install_dir: &Path,
    version: &str,
    emit: DesktopEventEmitter<'_>,
) -> anyhow::Result<(PathBuf, PathBuf, String)> {
    let version = download::normalize_version(version);
    emit(
        "download_started",
        serde_json::json!({
            "version": version,
            "install_dir": event_install_dir.to_string_lossy(),
            "cache_dir": platform::default_cache_dir().to_string_lossy(),
        }),
    )?;

    let result = {
        let mut progress = |progress: download::DownloadProgress| {
            emit(
                "download_progress",
                serde_json::json!({
                    "downloaded": progress.downloaded,
                    "total": progress.total,
                }),
            )
        };
        download::download_easytier_with_fallback_report(
            platform,
            download_dir,
            &version,
            &mut progress,
        )
        .await?
    };

    emit(
        "download_finished",
        serde_json::json!({
            "version": result.2,
            "core_path": event_install_dir.join(core_binary_name()).to_string_lossy(),
            "cli_path": event_install_dir.join(cli_binary_name()).to_string_lossy(),
        }),
    )?;

    Ok(result)
}

async fn prepare_desktop_update_package_with_events(
    platform: &platform::Platform,
    install_dir: &Path,
    version: &str,
    emit: DesktopEventEmitter<'_>,
) -> anyhow::Result<DesktopPreparedUpdate> {
    let staging_dir = desktop_update_staging_dir(install_dir);
    remove_existing_staging_dir(&staging_dir)?;
    let (_, _, installed_version) =
        download_with_desktop_events_to_dir(platform, &staging_dir, install_dir, version, emit)
            .await?;
    Ok(DesktopPreparedUpdate {
        staging_dir,
        version: installed_version,
    })
}

async fn prepare_desktop_update_package_quiet(
    platform: &platform::Platform,
    install_dir: &Path,
    version: &str,
) -> anyhow::Result<DesktopPreparedUpdate> {
    let staging_dir = desktop_update_staging_dir(install_dir);
    remove_existing_staging_dir(&staging_dir)?;
    let mut ignore_progress = |_progress: download::DownloadProgress| Ok(());
    let (_, _, installed_version) = download::download_easytier_with_fallback_report(
        platform,
        &staging_dir,
        version,
        &mut ignore_progress,
    )
    .await?;
    Ok(DesktopPreparedUpdate {
        staging_dir,
        version: installed_version,
    })
}

fn desktop_update_staging_dir(install_dir: &Path) -> PathBuf {
    install_dir.join(".desktop-update-tmp")
}

fn remove_existing_staging_dir(staging_dir: &Path) -> anyhow::Result<()> {
    if staging_dir.exists() {
        std::fs::remove_dir_all(staging_dir)?;
    }
    Ok(())
}

fn install_prepared_desktop_update(
    package: &DesktopPreparedUpdate,
    install_dir: &Path,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    download::sync_dir_contents(&package.staging_dir, install_dir)?;
    let core_path = install_dir.join(core_binary_name());
    let cli_path = install_dir.join(cli_binary_name());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&core_path, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok((core_path, cli_path))
}

async fn restart_existing_service_after_failed_update(
    install_dir: &Path,
    fallback_cli_path: &Path,
) {
    let cli_path =
        service::find_easytier_cli(install_dir).unwrap_or_else(|_| fallback_cli_path.to_path_buf());
    let _ = start_service_strict(&cli_path).await;
}

async fn stop_service_for_update(cli_path: &Path) -> anyhow::Result<()> {
    let stop = tokio::process::Command::new(cli_path)
        .args(["service", "--name", service::SERVICE_NAME, "stop"])
        .output()
        .await?;
    if stop.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&stop.stderr);
    if stderr.contains("already stopped") || stderr.contains("Service is stopped") {
        Ok(())
    } else {
        anyhow::bail!("停止服务失败，无法继续升级")
    }
}

async fn start_service_strict(cli_path: &Path) -> anyhow::Result<()> {
    let start = tokio::process::Command::new(cli_path)
        .args(["service", "--name", service::SERVICE_NAME, "start"])
        .output()
        .await?;
    if start.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&start.stderr);
    if stderr.contains("already running") {
        Ok(())
    } else {
        anyhow::bail!("启动服务失败: {}", stderr.trim())
    }
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

    if version_matches(Some(&current_version), &target_version) {
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
        if stderr.contains("already stopped") || stderr.contains("Service is stopped") {
            crate::style::success("服务已停止");
        } else {
            anyhow::bail!("停止服务失败，无法继续升级");
        }
    } else {
        crate::style::success("服务已停止");
    }

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
        if stderr.contains("already running") {
            crate::style::success("服务已重启");
        } else {
            crate::style::warning("启动失败，请尝试重新部署");
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_purge_rejects_lock_directory_ancestor() {
        let lock_parent = desktop_lifecycle_dir()
            .parent()
            .expect("lock dir has parent")
            .to_path_buf();

        let err = ensure_desktop_purge_safe(&lock_parent, None).unwrap_err();

        assert!(err.to_string().contains("生命周期锁目录"));
    }

    #[test]
    fn desktop_lifecycle_lock_excludes_concurrent_acquisition() {
        let lock_dir =
            std::env::temp_dir().join(format!("easytier-lock-test-{}", uuid::Uuid::new_v4()));
        let install_dir = std::env::temp_dir().join("easytier-lock-test");
        let first = acquire_desktop_lifecycle_lock_in(&lock_dir, &install_dir).expect("first lock");

        acquire_desktop_lifecycle_lock_in(&lock_dir, &install_dir).unwrap_err();

        drop(first);
        let second =
            acquire_desktop_lifecycle_lock_in(&lock_dir, &install_dir).expect("lock after drop");
        drop(second);
        let _ = std::fs::remove_dir_all(lock_dir);
    }

    #[test]
    fn matches_core_versions_with_release_prefixes() {
        assert!(version_matches(Some("2.6.4-8428a89d"), "v2.6.4"));
        assert!(version_matches(Some("2.6.4-8428a89d"), "v2.6.4-8428a89d"));
        assert!(!version_matches(Some("2.6.5-8428a89d"), "v2.6.4"));
    }

    #[test]
    fn service_config_match_is_unknown_when_args_are_not_visible() {
        assert_eq!(
            service_config_matches(
                Some(&"/usr/local/easytier/easytier-core".to_string()),
                "tcp://console/token",
            ),
            None
        );
        assert_eq!(
            service_config_matches(
                Some(
                    &"/usr/local/easytier/easytier-core --config-server tcp://console/token"
                        .to_string()
                ),
                "tcp://console/token",
            ),
            Some(true)
        );
        assert_eq!(
            service_config_matches(
                Some(
                    &"/usr/local/easytier/easytier-core --config-server tcp://console/other"
                        .to_string()
                ),
                "tcp://console/token",
            ),
            Some(false)
        );
    }

    #[test]
    fn reads_machine_id_from_service_args() {
        assert_eq!(
            machine_id_from_service_args(Some(
                &"easytier-core --machine-id machine-1 --secure-mode=true".to_string()
            )),
            Some("machine-1".to_string())
        );
        assert_eq!(
            machine_id_from_service_args(Some(&"easytier-core --machine-id=machine-2".to_string())),
            Some("machine-2".to_string())
        );
    }
}
