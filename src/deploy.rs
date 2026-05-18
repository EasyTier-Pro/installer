use crate::api::client::{
    ConsoleClient, CreateDeviceEnrollmentKeyRequest, DeviceEnrollmentKey,
};
use crate::config::Config;
use colored::Colorize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn get_core_version(core_path: &Path) -> Option<String> {
    let output = std::process::Command::new(core_path)
        .arg("--version")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().split_whitespace().nth(1).map(|s| s.to_string())
}

pub async fn run_deploy(
    config: &Config,
    client: &ConsoleClient,
    install_dir: Option<PathBuf>,
    config_server_base: Option<String>,
    version_override: Option<String>,
) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(default_install_dir);
    std::fs::create_dir_all(&install_dir)?;

    // 1. 获取用户信息
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
        let idx = read_choice(&tenant_names, "请选择要部署到的工作空间")? - 1;
        let t = me.tenants.into_iter().nth(idx).unwrap();
        crate::style::ok_kv("工作空间:", &t.name);
        t
    };

    // 尝试从 Console 获取推荐的版本和 config server URL
    let mut console_version = None;
    let mut console_config_server = None;
    if let Ok(gs) = client.get_started(&tenant.id).await {
        if !gs.config_server_url.is_empty() {
            console_config_server = Some(gs.config_server_url);
        }
        if !gs.release_channels.stable.version.is_empty() {
            console_version = Some(gs.release_channels.stable.version.clone());
        }
    }

    // 检测已有安装
    let mut need_reinstall = false;
    if let Ok(cli_path) = find_easytier_cli(&install_dir) {
        let is_installed = tokio::process::Command::new(&cli_path)
            .args(["service", "status"])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if is_installed {
            let core_name = if cfg!(windows) {
                "easytier-core.exe"
            } else {
                "easytier-core"
            };
            let core_path = install_dir.join(core_name);
            let version = get_core_version(&core_path)
                .unwrap_or_else(|| "未知版本".to_string());
            crate::style::info(&format!(
                "检测到 EasyTier {} 已安装",
                version.bright_white()
            ));
            println!();

            let items = vec!["取消", "更新软件", "重新部署"];
            let choice = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
                .with_prompt("检测到已有安装，请选择操作")
                .items(&items)
                .default(0)
                .interact()?;

            match choice {
                0 => return Ok(()),
                1 => {
                    return run_upgrade(
                        client,
                        &tenant.id,
                        &install_dir,
                        version_override,
                        console_version.clone(),
                    )
                    .await
                }
                2 => {
                    need_reinstall = true;
                }
                _ => unreachable!(),
            }
        }
    }

    // 2. 获取 enrollment key
    let keys = client.list_device_enrollment_keys(&tenant.id).await?;
    let active_keys: Vec<_> = keys
        .into_iter()
        .filter(|k| !k.revoked && k.lifecycle_state != "expired")
        .collect();

    let bootstrap_token = if active_keys.is_empty() {
        crate::style::info("当前没有可用的注册密钥");
        if !confirm_yes("当前没有可用密钥，是否创建一个用于本次部署")? {
            anyhow::bail!("没有可用的注册密钥，部署已取消。您可以前往 Console 手动创建。");
        }
        let (key, token) = create_new_key(client, &tenant.id).await?;
        let label = key_type_label(key.reusable);
        crate::style::ok_kv("注册密钥:", &format!("{} [{}]", key_name(&key), label));
        token
    } else if active_keys.len() == 1 {
        let key = active_keys.into_iter().next().unwrap();
        let name = key_name(&key).to_string();
        let label = key_type_label(key.reusable);
        crate::style::info(&format!("发现{}密钥: {}", label, name));
        if confirm_yes(&format!("是否使用{}密钥 {} 进行部署", label, name))? {
            let token = get_key_token(client, &tenant.id, &key).await?;
            crate::style::ok_kv("注册密钥:", &format!("{} [{}]", name, label));
            token
        } else {
            let (key, token) = create_new_key(client, &tenant.id).await?;
            let new_label = key_type_label(key.reusable);
            crate::style::ok_kv("注册密钥:", &format!("{} [{}]", key_name(&key), new_label));
            token
        }
    } else {
        let multi_keys: Vec<_> = active_keys.iter().filter(|k| k.reusable).cloned().collect();
        let single_keys: Vec<_> = active_keys.iter().filter(|k| !k.reusable).cloned().collect();
        let (token, key) = select_key(client, &tenant.id, &multi_keys, &single_keys).await?;
        let label = key_type_label(key.reusable);
        crate::style::ok_kv("注册密钥:", &format!("{} [{}]", key_name(&key), label));
        token
    };

    // 3. 构造 config server URL
    let config_server = build_config_server_url(
        &config.console_base_url,
        config_server_base.or(console_config_server),
    )?;
    let full_config_url = format!(
        "{}/{}",
        config_server.trim_end_matches('/'),
        bootstrap_token
    );
    crate::style::kv("配置服务器:", &full_config_url);
    println!();

    // 4. 检测平台并下载 easytier
    let platform = detect_platform()?;

    let download_version = version_override.or(console_version);
    let version_label = download_version.clone().unwrap_or_else(|| "latest".to_string());
    crate::style::info(&format!(
        "正在下载 easytier {} ({}-{})...",
        version_label.bright_white(),
        platform.os,
        platform.arch
    ));

    let (core_path, cli_path, _installed_version) =
        download_easytier(&platform, &install_dir, download_version).await?;
    crate::style::success("下载完成");

    if need_reinstall {
        if let Ok(cli) = find_easytier_cli(&install_dir) {
            crate::style::info("正在卸载旧服务...");
            let _ = tokio::process::Command::new(&cli)
                .args(["service", "uninstall"])
                .output()
                .await;
        }
    }

    println!();
    crate::style::info("正在安装并启动服务...");
    install_service(&cli_path, &core_path, &full_config_url).await?;

    println!();
    crate::style::success(&format!("{} 部署完成，正在运行。", "EasyTier".bright_white()));
    Ok(())
}

pub async fn run_status(install_dir: Option<PathBuf>) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(default_install_dir);
    let cli_path = find_easytier_cli(&install_dir)?;

    let status = tokio::process::Command::new(&cli_path)
        .arg("service")
        .arg("status")
        .output()
        .await?;

    if status.status.success() {
        println!("{}", String::from_utf8_lossy(&status.stdout).trim());
    } else {
        eprintln!("{}", String::from_utf8_lossy(&status.stderr));
        anyhow::bail!("获取服务状态失败");
    }
    Ok(())
}

pub async fn run_uninstall(install_dir: Option<PathBuf>) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(default_install_dir);
    let cli_path = find_easytier_cli(&install_dir)?;

    let output = tokio::process::Command::new(&cli_path)
        .arg("service")
        .arg("uninstall")
        .output()
        .await?;

    if output.status.success() {
        crate::style::success("EasyTier 服务已卸载");
    } else {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        anyhow::bail!("卸载服务失败");
    }
    Ok(())
}

// ─── Key selection helpers ───

fn key_name(key: &DeviceEnrollmentKey) -> &str {
    key.display_name.as_deref().unwrap_or(&key.key_code)
}

fn key_type_label(reusable: bool) -> &'static str {
    if reusable {
        "多设备"
    } else {
        "单设备"
    }
}

fn confirm_yes(prompt: &str) -> anyhow::Result<bool> {
    dialoguer::Confirm::with_theme(&crate::style::dialoguer_theme())
        .with_prompt(prompt)
        .default(true)
        .interact()
        .map_err(|e| e.into())
}

async fn get_key_token(
    client: &ConsoleClient,
    tenant_id: &str,
    key: &DeviceEnrollmentKey,
) -> anyhow::Result<String> {
    Ok(client
        .get_device_enrollment_key_secret(tenant_id, &key.id)
        .await?
        .bootstrap_token)
}

async fn select_key(
    client: &ConsoleClient,
    tenant_id: &str,
    multi_keys: &[DeviceEnrollmentKey],
    single_keys: &[DeviceEnrollmentKey],
) -> anyhow::Result<(String, DeviceEnrollmentKey)> {
    let mut options: Vec<String> = Vec::new();
    let mut key_refs: Vec<&DeviceEnrollmentKey> = Vec::new();

    for key in multi_keys {
        options.push(format!("{} [多设备]", key_name(key)));
        key_refs.push(key);
    }
    for key in single_keys {
        options.push(format!("{} [单设备]", key_name(key)));
        key_refs.push(key);
    }

    options.push("[创建新密钥]".to_string());

    let choice = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
        .with_prompt("请选择要使用的注册密钥")
        .items(&options)
        .default(0)
        .interact()?;

    if choice == key_refs.len() {
        let (key, token) = create_new_key(client, tenant_id).await?;
        let label = key_type_label(key.reusable);
        crate::style::success(&format!("已创建{}密钥: {}", label, key_name(&key)));
        Ok((token, key))
    } else {
        let key = key_refs[choice].clone();
        let token = get_key_token(client, tenant_id, &key).await?;
        Ok((token, key))
    }
}

async fn create_new_key(
    client: &ConsoleClient,
    tenant_id: &str,
) -> anyhow::Result<(DeviceEnrollmentKey, String)> {
    let default_name = format!(
        "agent-{}",
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "unknown".to_string())
    );

    let name = dialoguer::Input::with_theme(&crate::style::dialoguer_theme())
        .with_prompt("请输入新密钥的名称")
        .default(default_name)
        .interact()?;

    let type_items = vec!["单设备（仅本设备可用）", "多设备（可被多台设备共用）"];
    let is_multi = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
        .with_prompt("该密钥是否可被多台设备共用")
        .items(&type_items)
        .default(0)
        .interact()? == 1;

    let req = CreateDeviceEnrollmentKeyRequest {
        display_name: Some(name),
        tags: None,
        reusable: is_multi,
        pre_approved: true,
    };

    let resp = client.create_device_enrollment_key(tenant_id, &req).await?;
    Ok((resp.enrollment_key, resp.bootstrap_token))
}

fn read_choice(items: &[String], prompt: &str) -> anyhow::Result<usize> {
    let selection = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact()?;
    Ok(selection + 1)
}

fn build_config_server_url(
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

struct Platform {
    os: &'static str,
    arch: &'static str,
}

fn detect_platform() -> anyhow::Result<Platform> {
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

fn default_install_dir() -> PathBuf {
    directories::ProjectDirs::from("cn", "easytier", "agent")
        .map(|d| d.data_dir().join("easytier"))
        .unwrap_or_else(|| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".easytier")
        })
}

fn find_easytier_cli(install_dir: &Path) -> anyhow::Result<PathBuf> {
    let name = if cfg!(windows) {
        "easytier-cli.exe"
    } else {
        "easytier-cli"
    };
    let path = install_dir.join(name);
    if path.exists() {
        Ok(path)
    } else {
        anyhow::bail!("未找到 easytier-cli，请先执行部署命令进行安装")
    }
}

async fn run_upgrade(
    client: &ConsoleClient,
    tenant_id: &str,
    install_dir: &Path,
    version_override: Option<String>,
    console_version: Option<String>,
) -> anyhow::Result<()> {
    let platform = detect_platform()?;
    let target_version = if let Some(v) = version_override {
        if !v.starts_with('v') {
            format!("v{}", v)
        } else {
            v
        }
    } else if let Some(v) = console_version {
        v
    } else if let Ok(gs) = client.get_started(tenant_id).await {
        if !gs.release_channels.stable.version.is_empty() {
            gs.release_channels.stable.version
        } else {
            fetch_latest_version().await?
        }
    } else {
        fetch_latest_version().await?
    };

    let core_name = if cfg!(windows) {
        "easytier-core.exe"
    } else {
        "easytier-core"
    };
    let core_path = install_dir.join(core_name);
    let current_version = get_core_version(&core_path)
        .unwrap_or_else(|| "未知".to_string());

    if current_version == target_version {
        crate::style::info(&format!("当前已是最新版本 {}", target_version.bright_white()));
        return Ok(());
    }

    crate::style::info(&format!(
        "正在从 {} 升级至 {}...",
        current_version.bright_white(),
        target_version.bright_white()
    ));

    let (_, cli_path, _) =
        download_easytier(&platform, install_dir, Some(target_version.clone())).await?;
    crate::style::success(&format!("已下载 {}", target_version));

    println!();
    crate::style::info("正在重启服务...");
    let _ = tokio::process::Command::new(&cli_path)
        .args(["service", "stop"])
        .output()
        .await;

    let start = tokio::process::Command::new(&cli_path)
        .args(["service", "start"])
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

async fn download_easytier(
    platform: &Platform,
    install_dir: &Path,
    version_override: Option<String>,
) -> anyhow::Result<(PathBuf, PathBuf, String)> {
    let is_specific_version = version_override.is_some();
    let version = if let Some(v) = version_override {
        if !v.starts_with('v') {
            format!("v{}", v)
        } else {
            v
        }
    } else {
        fetch_latest_version().await?
    };

    let asset_name = format!(
        "easytier-{}-{}-{}.zip",
        platform.os, platform.arch, version
    );
    let download_url = if is_specific_version {
        format!(
            "https://github.com/EasyTier/EasyTier/releases/download/{}/{}",
            version, asset_name
        )
    } else {
        format!(
            "https://github.com/EasyTier/EasyTier/releases/latest/download/{}",
            asset_name
        )
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let resp = client.get(&download_url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "下载失败 ({}): 请检查网络连接或手动下载 {} 到 {}",
            resp.status(),
            asset_name,
            install_dir.display()
        );
    }

    let total_size = resp.content_length().unwrap_or(0);
    let pb = indicatif::ProgressBar::new(total_size);
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
            .progress_chars("#>-"),
    );

    let mut zip_data = Vec::new();
    let mut stream = resp.bytes_stream();
    use tokio_stream::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        zip_data.extend_from_slice(&chunk);
        pb.inc(chunk.len() as u64);
    }
    pb.finish_and_clear();

    extract_zip(&zip_data, install_dir)?;

    let core_name = if cfg!(windows) {
        "easytier-core.exe"
    } else {
        "easytier-core"
    };
    let cli_name = if cfg!(windows) {
        "easytier-cli.exe"
    } else {
        "easytier-cli"
    };

    let core_path = install_dir.join(core_name);
    let cli_path = install_dir.join(cli_name);

    if !core_path.exists() {
        let mut found = false;
        for entry in std::fs::read_dir(install_dir)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                let sub_core = p.join(core_name);
                let sub_cli = p.join(cli_name);
                if sub_core.exists() && sub_cli.exists() {
                    std::fs::copy(&sub_core, &core_path)?;
                    std::fs::copy(&sub_cli, &cli_path)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&core_path, std::fs::Permissions::from_mode(0o755))?;
                        std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o755))?;
                    }
                    found = true;
                    break;
                }
            }
        }
        if !found {
            anyhow::bail!("解压后未找到 easytier-core 和 easytier-cli");
        }
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&core_path, std::fs::Permissions::from_mode(0o755))?;
            std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    Ok((core_path, cli_path, version))
}

async fn fetch_latest_version() -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .get("https://api.github.com/repos/EasyTier/EasyTier/releases/latest")
        .header("User-Agent", "easytier-agent/0.1.0")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "无法获取最新版本 ({}): 请使用 --version 指定版本号",
            resp.status()
        );
    }

    let json: serde_json::Value = resp.json().await?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("GitHub API 返回格式异常"))?;
    Ok(tag.to_string())
}

fn extract_zip(data: &[u8], dest: &Path) -> anyhow::Result<()> {
    let reader = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => dest.join(path),
            None => continue,
        };

        if file.is_dir() {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(p)?;
                }
            }
            let mut outfile = std::fs::File::create(&outpath)?;
            let mut buf = [0u8; 8192];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                outfile.write_all(&buf[..n])?;
            }
        }
    }
    Ok(())
}

async fn install_service(
    cli_path: &Path,
    core_path: &Path,
    config_url: &str,
) -> anyhow::Result<()> {
    let args = vec![
        "service".to_string(),
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
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.trim().is_empty() {
            println!("  {}", stdout.trim());
        }
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
        .args(["service", "start"])
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
