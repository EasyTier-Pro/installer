use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(crate) const SERVICE_NAME: &str = "easytier-pro";
const BOOTSTRAP_FINGERPRINT_FILE: &str = ".bootstrap-fingerprint";
const LEGACY_CORE_SERVICE_CONFIG_FILE: &str = "easytier-core-service.toml";

pub(crate) type BootstrapFingerprint = String;

#[derive(Debug, Clone, Default)]
pub(crate) struct ServiceStatusInfo {
    pub installed: bool,
    pub running: bool,
    pub state: Option<String>,
    pub binary_path: Option<String>,
}

pub(crate) fn service_not_installed(output: &std::process::Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    service_not_installed_text(&stdout, &stderr)
}

fn service_not_installed_text(stdout: &str, stderr: &str) -> bool {
    stdout.contains("Service is not installed")
        || stderr.contains("Service is not installed")
        || stdout.contains("1060")
        || stderr.contains("1060")
        || stdout.contains("does not exist as an installed service")
        || stderr.contains("does not exist as an installed service")
        || stdout.contains("指定的服务未安装")
        || stderr.contains("指定的服务未安装")
}

pub(crate) fn service_stopped_after_uninstall(output: &std::process::Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.contains("Service is stopped") || stderr.contains("Service is stopped")
}

#[cfg(windows)]
fn service_missing_in_sc(output: &std::process::Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    service_not_installed_text(&stdout, &stderr)
}

pub(crate) async fn service_is_installed(cli_path: &Path) -> bool {
    let install_dir = cli_path.parent().unwrap_or_else(|| Path::new(""));
    query_service_status(install_dir, Some(cli_path))
        .await
        .installed
}

pub(crate) async fn query_service_status(
    install_dir: &Path,
    cli_path: Option<&Path>,
) -> ServiceStatusInfo {
    #[cfg(windows)]
    {
        let _ = install_dir;
        let _ = cli_path;
        windows_service_status().await
    }

    #[cfg(target_os = "macos")]
    {
        let _ = install_dir;
        let status = macos_service_status().await;
        if status.installed {
            return status;
        }
        if let Some(cli_path) = cli_path {
            return cli_service_status(cli_path).await;
        }
        status
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = install_dir;
        let status = linux_service_status().await;
        if status.installed {
            return status;
        }
        if let Some(cli_path) = cli_path {
            return cli_service_status(cli_path).await;
        }
        status
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = install_dir;
        if let Some(cli_path) = cli_path {
            return cli_service_status(cli_path).await;
        }
        ServiceStatusInfo::default()
    }
}

#[cfg(windows)]
async fn windows_service_status() -> ServiceStatusInfo {
    let Ok(query) = tokio::process::Command::new("sc")
        .args(["query", SERVICE_NAME])
        .output()
        .await
    else {
        return ServiceStatusInfo::default();
    };

    if service_missing_in_sc(&query) {
        return ServiceStatusInfo::default();
    }

    let stdout = String::from_utf8_lossy(&query.stdout);
    let installed = query.status.success() || stdout.contains("SERVICE_NAME:");
    if !installed {
        return ServiceStatusInfo::default();
    }

    let state = parse_sc_state(&stdout);
    let running = state.as_deref() == Some("RUNNING") || stdout.contains("RUNNING");
    let binary_path = windows_service_binary_path().await;

    ServiceStatusInfo {
        installed,
        running,
        state,
        binary_path,
    }
}

#[cfg(windows)]
async fn windows_service_binary_path() -> Option<String> {
    let output = tokio::process::Command::new("sc")
        .args(["qc", SERVICE_NAME])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_sc_binary_path(&stdout)
}

#[cfg(target_os = "macos")]
async fn macos_service_status() -> ServiceStatusInfo {
    let system = launchctl_service_status(&format!("system/{}", SERVICE_NAME)).await;
    if system.installed {
        return system;
    }

    if let Some(uid) = current_uid().await {
        let gui = launchctl_service_status(&format!("gui/{}/{}", uid, SERVICE_NAME)).await;
        if gui.installed {
            return gui;
        }
    }

    ServiceStatusInfo::default()
}

#[cfg(target_os = "macos")]
async fn current_uid() -> Option<String> {
    let output = tokio::process::Command::new("id")
        .arg("-u")
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if uid.is_empty() { None } else { Some(uid) }
}

#[cfg(target_os = "macos")]
async fn launchctl_service_status(target: &str) -> ServiceStatusInfo {
    let Ok(output) = tokio::process::Command::new("launchctl")
        .args(["print", target])
        .output()
        .await
    else {
        return ServiceStatusInfo::default();
    };

    if !output.status.success() {
        return ServiceStatusInfo::default();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_launchctl_print(&stdout)
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn linux_service_status() -> ServiceStatusInfo {
    let Ok(output) = tokio::process::Command::new("systemctl")
        .args([
            "show",
            SERVICE_NAME,
            "--property=LoadState,ActiveState,ExecStart",
            "--no-pager",
        ])
        .output()
        .await
    else {
        return ServiceStatusInfo::default();
    };

    if !output.status.success() {
        return ServiceStatusInfo::default();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_systemctl_show(&stdout)
}

#[cfg_attr(windows, allow(dead_code))]
async fn cli_service_status(cli_path: &Path) -> ServiceStatusInfo {
    let Ok(output) = tokio::process::Command::new(cli_path)
        .args(["service", "--name", SERVICE_NAME, "status"])
        .output()
        .await
    else {
        return ServiceStatusInfo::default();
    };

    if service_not_installed(&output) {
        return ServiceStatusInfo::default();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = format!("{}\n{}", stdout, stderr);
    let state = parse_status_text_state(&text);
    let running = state.as_deref() == Some("RUNNING")
        || text.to_ascii_lowercase().contains("running")
        || text.to_ascii_lowercase().contains("active");

    ServiceStatusInfo {
        installed: true,
        running,
        state,
        binary_path: None,
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn parse_status_text_state(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("running") || lower.contains("active") {
        Some("RUNNING".to_string())
    } else if lower.contains("stopped") || lower.contains("inactive") {
        Some("STOPPED".to_string())
    } else {
        None
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn parse_sc_state(text: &str) -> Option<String> {
    text.lines()
        .find(|line| line.contains("STATE"))
        .and_then(|line| line.split_whitespace().last())
        .map(|state| state.trim().to_ascii_uppercase())
}

#[cfg_attr(not(windows), allow(dead_code))]
fn parse_sc_binary_path(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line.trim_start();
        let value = trimmed.strip_prefix("BINARY_PATH_NAME")?;
        let value = value.split_once(':')?.1.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_launchctl_print(text: &str) -> ServiceStatusInfo {
    let lower = text.to_ascii_lowercase();
    let state = text.lines().find_map(|line| {
        let trimmed = line.trim();
        let value = trimmed.strip_prefix("state =")?.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_ascii_uppercase())
        }
    });
    let running = state.as_deref() == Some("RUNNING")
        || lower.contains("state = running")
        || text.lines().any(|line| {
            let trimmed = line.trim();
            let Some(value) = trimmed.strip_prefix("pid =") else {
                return false;
            };
            value.trim().parse::<u32>().is_ok_and(|pid| pid > 0)
        });
    let binary_path = text.lines().find_map(|line| {
        let trimmed = line.trim();
        let value = trimmed
            .strip_prefix("program =")
            .or_else(|| trimmed.strip_prefix("path ="))?
            .trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    });

    ServiceStatusInfo {
        installed: true,
        running,
        state,
        binary_path,
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn parse_systemctl_show(text: &str) -> ServiceStatusInfo {
    let load_state = key_value_line(text, "LoadState");
    let active_state = key_value_line(text, "ActiveState");
    let exec_start = key_value_line(text, "ExecStart");
    let installed = load_state.as_deref() == Some("loaded");
    let running = active_state.as_deref() == Some("active");
    let state = active_state.map(|state| state.to_ascii_uppercase());
    let binary_path = exec_start.and_then(|value| parse_systemctl_exec_path(&value));

    ServiceStatusInfo {
        installed,
        running,
        state,
        binary_path,
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn key_value_line(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (line_key, value) = line.split_once('=')?;
        if line_key == key {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

#[cfg_attr(windows, allow(dead_code))]
fn parse_systemctl_exec_path(value: &str) -> Option<String> {
    let start = value.find("path=")? + "path=".len();
    let rest = &value[start..];
    let end = rest
        .find(|ch: char| ch.is_whitespace() || ch == ';')
        .unwrap_or(rest.len());
    let path = rest[..end].trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
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
    machine_id: Option<&str>,
) -> anyhow::Result<()> {
    install_service_impl(cli_path, core_path, config_url, machine_id, false).await
}

pub(crate) async fn install_service_quiet(
    cli_path: &Path,
    core_path: &Path,
    config_url: &str,
    machine_id: Option<&str>,
) -> anyhow::Result<()> {
    install_service_impl(cli_path, core_path, config_url, machine_id, true).await
}

async fn install_service_impl(
    cli_path: &Path,
    core_path: &Path,
    config_url: &str,
    machine_id: Option<&str>,
    quiet: bool,
) -> anyhow::Result<()> {
    if let Ok(status) = tokio::process::Command::new(cli_path)
        .args(["service", "--name", SERVICE_NAME, "status"])
        .output()
        .await
        && !service_not_installed(&status)
    {
        let _ = tokio::process::Command::new(cli_path)
            .args(["service", "--name", SERVICE_NAME, "uninstall"])
            .output()
            .await;
        systemd_daemon_reload().await;
    }

    write_bootstrap_fingerprint(core_path, config_url)?;
    let args = service_install_args(core_path, config_url, machine_id);

    let output = tokio::process::Command::new(cli_path)
        .args(&args)
        .output()
        .await?;

    if output.status.success() {
        if !quiet {
            crate::style::kv("服务名:", SERVICE_NAME);
            crate::style::kv("程序路径:", &core_path.to_string_lossy());
            crate::style::kv("配置服务器:", config_url);
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !quiet {
            eprintln!("{}", stderr);
        }
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
        if !quiet && !stdout.trim().is_empty() {
            println!("  {}", stdout.trim());
        }
        if !quiet {
            crate::style::success("服务已安装并启动");
        }
    } else {
        let stderr = String::from_utf8_lossy(&start.stderr);
        if stderr.contains("already running") {
            if !quiet {
                crate::style::success("服务已安装并启动");
            }
        } else if quiet {
            anyhow::bail!("启动服务失败: {}", stderr.trim());
        } else if !quiet {
            println!("  {}", stderr.trim());
            crate::style::warning("服务已安装但启动失败，您可以稍后手动启动");
        }
    }

    Ok(())
}

fn service_install_args(
    core_path: &Path,
    config_url: &str,
    machine_id: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
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

    if let Some(mid) = machine_id {
        args.push("--machine-id".to_string());
        args.push(mid.to_string());
    }

    args
}

fn write_bootstrap_fingerprint(core_path: &Path, config_url: &str) -> anyhow::Result<()> {
    let install_dir = core_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("failed to resolve easytier-core install directory"))?;
    std::fs::create_dir_all(install_dir)?;
    let token = extract_bootstrap_token(config_url)
        .ok_or_else(|| anyhow::anyhow!("failed to parse bootstrap token from config server URL"))?;
    std::fs::write(
        install_dir.join(BOOTSTRAP_FINGERPRINT_FILE),
        format!("{}\n", hash_bootstrap_token(token)),
    )?;
    let _ = std::fs::remove_file(install_dir.join(LEGACY_CORE_SERVICE_CONFIG_FILE));
    Ok(())
}

#[cfg(windows)]
pub(crate) fn restrict_sensitive_file_permissions(config_path: &Path) -> anyhow::Result<()> {
    let output = std::process::Command::new("icacls")
        .arg(config_path)
        .args(windows_service_config_acl_args())
        .output()
        .map_err(|err| {
            let _ = std::fs::remove_file(config_path);
            anyhow::anyhow!("收紧服务配置文件权限失败: {}", err)
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_file(config_path);
        anyhow::bail!("收紧服务配置文件权限失败: {}", stderr.trim());
    }

    Ok(())
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn windows_service_config_acl_args() -> [&'static str; 4] {
    [
        "/inheritance:r",
        "/grant:r",
        "*S-1-5-18:F",
        "*S-1-5-32-544:F",
    ]
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

pub(crate) fn bootstrap_fingerprint(install_dir: &Path) -> Option<BootstrapFingerprint> {
    let fingerprint_path = install_dir.join(BOOTSTRAP_FINGERPRINT_FILE);
    if let Ok(contents) = std::fs::read_to_string(&fingerprint_path) {
        let fingerprint = contents.trim();
        if !fingerprint.is_empty() {
            return Some(fingerprint.to_string());
        }
    }

    let config_path = install_dir.join(LEGACY_CORE_SERVICE_CONFIG_FILE);
    let contents = std::fs::read_to_string(&config_path).ok()?;
    let config_server = find_config_server_field(&contents)?;
    let token = extract_bootstrap_token(&config_server)?;
    Some(hash_bootstrap_token(token))
}

pub(crate) fn bootstrap_fingerprint_for_token(token: &str) -> String {
    hash_bootstrap_token(token)
}

fn find_config_server_field(contents: &str) -> Option<String> {
    let parsed = contents.parse::<toml::Value>().ok()?;
    parsed
        .get("config-server")
        .or_else(|| parsed.get("config_server"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
}

fn extract_bootstrap_token(config_server: &str) -> Option<&str> {
    let trimmed = config_server.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let token = trimmed.rsplit('/').next()?;
    if token.is_empty() { None } else { Some(token) }
}

fn hash_bootstrap_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut out = String::with_capacity(32);
    for byte in &digest[..16] {
        use std::fmt::Write;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

pub async fn run_status(install_dir: Option<PathBuf>) -> anyhow::Result<()> {
    let install_dir = install_dir.unwrap_or_else(super::platform::default_install_dir);
    let cli_path = find_easytier_cli(&install_dir).ok();
    let status = query_service_status(&install_dir, cli_path.as_deref()).await;

    if !status.installed {
        anyhow::bail!("EasyTier service is not installed");
    }

    println!("service: {}", SERVICE_NAME);
    println!("state: {}", status.state.as_deref().unwrap_or("UNKNOWN"));
    println!("running: {}", status.running);
    if let Some(binary_path) = status.binary_path {
        println!("binary_path: {}", binary_path);
    }

    Ok(())
}

#[allow(dead_code)]
async fn run_status_legacy(install_dir: Option<PathBuf>) -> anyhow::Result<()> {
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
        let install_dir = install_dir
            .clone()
            .unwrap_or_else(super::platform::default_install_dir);
        let install_dir_arg = install_dir.to_string_lossy().to_string();
        let mut extra_args = vec!["uninstall"];
        if purge {
            extra_args.push("--purge");
        }
        extra_args.push("--install-dir");
        extra_args.push(&install_dir_arg);
        let status = super::platform::relaunch_elevated_with_replaced_args(&extra_args)?;
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
    uninstall_service_impl(cli_path, false).await
}

pub(crate) async fn uninstall_service_quiet(cli_path: &Path) -> anyhow::Result<()> {
    uninstall_service_impl(cli_path, true).await
}

async fn uninstall_service_impl(cli_path: &Path, quiet: bool) -> anyhow::Result<()> {
    if !quiet {
        crate::style::info("正在卸载服务...");
    }

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
        if service_stopped_after_uninstall(&output) {
            crate::style::debug("uninstall 返回 Service is stopped，按卸载成功处理");
            return Ok(());
        }
        if !stderr.trim().is_empty() {
            crate::style::debug(&format!("uninstall stderr={}", stderr.trim()));
            if !quiet {
                eprintln!("{}", stderr.trim());
            }
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
        } else if service_stopped_after_uninstall(&v) {
            crate::style::debug("verify 返回 Service is stopped，服务已卸载");
        } else if !v.status.success() {
            let verify_stderr = String::from_utf8_lossy(&v.stderr).to_lowercase();
            if verify_stderr.contains("access is denied")
                || verify_stderr.contains("permission")
                || verify_stderr.contains("拒绝访问")
            {
                crate::style::debug("verify 返回权限受限，按卸载成功处理");
                return Ok(());
            }
            if !quiet {
                crate::style::warning("卸载后状态探测失败，按卸载成功处理");
            }
            crate::style::debug("verify 探测失败，按卸载成功处理");
            return Ok(());
        } else {
            if !quiet {
                crate::style::warning("卸载未生效，服务仍然存在");
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_install_args_use_core_cli_flags() {
        let core_path = PathBuf::from(r"C:\EasyTier\easytier-core.exe");
        let args = service_install_args(
            &core_path,
            "tcp://console.easytier.cn:22020/bootstrap-token",
            Some("machine-id"),
        );

        assert!(args.contains(&"--config-server".to_string()));
        assert!(args.contains(&"tcp://console.easytier.cn:22020/bootstrap-token".to_string()));
        assert!(args.contains(&"--secure-mode=true".to_string()));
        assert!(args.contains(&"--machine-id".to_string()));
        assert!(args.contains(&"machine-id".to_string()));
        assert!(!args.contains(&"--config-file".to_string()));
    }

    #[test]
    fn detects_windows_service_missing_messages() {
        assert!(service_not_installed_text(
            "[SC] EnumQueryServicesStatus:OpenService FAILED 1060:",
            "",
        ));
        assert!(service_not_installed_text(
            "",
            "The specified service does not exist as an installed service.",
        ));
        assert!(service_not_installed_text("", "指定的服务未安装"));
    }

    #[test]
    fn parses_windows_sc_status_and_binary_path() {
        let query = r#"
SERVICE_NAME: easytier-pro
        TYPE               : 10  WIN32_OWN_PROCESS
        STATE              : 4  RUNNING
"#;
        let config = r#"
SERVICE_NAME: easytier-pro
        BINARY_PATH_NAME   : \??\C:\EasyTier\easytier-core.exe --config-server tcp://console/token --secure-mode=true
"#;

        assert_eq!(parse_sc_state(query), Some("RUNNING".to_string()));
        assert_eq!(
            parse_sc_binary_path(config),
            Some(
                r"\??\C:\EasyTier\easytier-core.exe --config-server tcp://console/token --secure-mode=true"
                    .to_string()
            )
        );
    }

    #[test]
    fn parses_launchctl_status() {
        let status = parse_launchctl_print(
            r#"
{
    state = running
    program = /usr/local/easytier/easytier-core
    pid = 42
}
"#,
        );

        assert!(status.installed);
        assert!(status.running);
        assert_eq!(status.state, Some("RUNNING".to_string()));
        assert_eq!(
            status.binary_path,
            Some("/usr/local/easytier/easytier-core".to_string())
        );
    }

    #[test]
    fn parses_systemctl_status() {
        let status = parse_systemctl_show(
            r#"
LoadState=loaded
ActiveState=active
ExecStart={ path=/opt/easytier/easytier-core ; argv[]=/opt/easytier/easytier-core --config-server tcp://console/token ; }
"#,
        );

        assert!(status.installed);
        assert!(status.running);
        assert_eq!(status.state, Some("ACTIVE".to_string()));
        assert_eq!(
            status.binary_path,
            Some("/opt/easytier/easytier-core".to_string())
        );
    }

    #[test]
    fn windows_service_config_acl_uses_sids() {
        assert_eq!(
            windows_service_config_acl_args(),
            [
                "/inheritance:r",
                "/grant:r",
                "*S-1-5-18:F",
                "*S-1-5-32-544:F",
            ]
        );
    }

    #[test]
    fn extracts_bootstrap_token_from_config_server() {
        assert_eq!(
            extract_bootstrap_token("tcp://console.easytier.net:22020/bootstrap-token"),
            Some("bootstrap-token")
        );
        assert_eq!(
            extract_bootstrap_token("tcp://console.easytier.net:22020/bootstrap-token/"),
            Some("bootstrap-token")
        );
        assert_eq!(extract_bootstrap_token(""), None);
    }

    #[test]
    fn finds_config_server_field_in_service_toml() {
        let toml = r#"
config-server = "tcp://console.easytier.net:22020/bootstrap-token"
secure-mode = true
"#;

        assert_eq!(
            find_config_server_field(toml),
            Some("tcp://console.easytier.net:22020/bootstrap-token".to_string())
        );
    }

    #[test]
    fn bootstrap_fingerprint_prefers_shared_fingerprint_file() {
        let dir = std::env::temp_dir().join(format!(
            "easytier-fingerprint-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(dir.join(BOOTSTRAP_FINGERPRINT_FILE), "persisted\n")
            .expect("write fingerprint");
        std::fs::write(
            dir.join(LEGACY_CORE_SERVICE_CONFIG_FILE),
            r#"config-server = "tcp://console.easytier.net:22020/legacy-token""#,
        )
        .expect("write legacy config");

        assert_eq!(bootstrap_fingerprint(&dir), Some("persisted".to_string()));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bootstrap_fingerprint_reads_legacy_service_toml() {
        let dir = std::env::temp_dir().join(format!(
            "easytier-legacy-fingerprint-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join(LEGACY_CORE_SERVICE_CONFIG_FILE),
            r#"config-server = "tcp://console.easytier.net:22020/bootstrap-token""#,
        )
        .expect("write legacy config");

        assert_eq!(
            bootstrap_fingerprint(&dir),
            Some(bootstrap_fingerprint_for_token("bootstrap-token"))
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
