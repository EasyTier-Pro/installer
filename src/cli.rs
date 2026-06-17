use crate::api::client::ConsoleClient;
use crate::auth::login;
use crate::auth::token_store::TokenStore;
use crate::config::Config;
use crate::deploy::{self, ExistingAction};
use crate::desktop;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "easytier-pro-installer")]
#[command(about = "EasyTier Pro 一键部署工具")]
pub struct Cli {
    #[arg(short, long, global = true, env = "EASYTIER_CONSOLE_URL")]
    pub server: Option<String>,

    #[arg(long, global = true, env = "EASYTIER_CONFIG_SERVER")]
    pub config_server: Option<String>,

    #[arg(short, long, global = true, env = "EASYTIER_INSTALL_DIR")]
    pub install_dir: Option<PathBuf>,

    /// 开启调试日志，默认写入当前目录下的 easytier-pro-installer.debug.log
    #[arg(long, global = true)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// 安装并完成首次部署
    Install(InstallArgs),

    /// 更新已安装的 EasyTier
    Update(UpdateArgs),

    /// 卸载 EasyTier 服务
    Uninstall(UninstallArgs),

    /// 查看服务状态
    Status(StatusArgs),

    /// 面向桌面端的 JSON 子进程协议
    #[command(subcommand, hide = true)]
    Desktop(DesktopCommand),

    #[command(name = "install-service", hide = true)]
    InstallService(InstallServiceArgs),

    #[command(name = "upgrade-service", hide = true)]
    UpgradeService(UpgradeServiceArgs),

    #[command(name = "uninstall-service", hide = true)]
    UninstallService(UninstallServiceArgs),
}

#[derive(Args, Debug, Clone, Default)]
pub struct InstallArgs {
    #[arg(short, long, env = "EASYTIER_VERSION")]
    pub version: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct UpdateArgs {
    #[arg(short, long, env = "EASYTIER_VERSION")]
    pub version: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct UninstallArgs {
    /// 彻底删除安装目录和缓存压缩包
    #[arg(long)]
    pub purge: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct StatusArgs {
    /// 以 JSON 对象输出机器可读状态
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct InstallServiceArgs {
    #[arg(long)]
    pub service_core_path: Option<PathBuf>,

    #[arg(long)]
    pub service_config_url: Option<String>,

    #[arg(long)]
    pub service_config_url_file: Option<PathBuf>,

    #[arg(long)]
    pub service_machine_id: Option<String>,

    #[arg(long)]
    pub service_strict_start: bool,

    #[arg(long, hide = true)]
    pub desktop_lock_dir: Option<PathBuf>,

    #[arg(long, hide = true)]
    pub desktop_parent_lock_held: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct UpgradeServiceArgs {
    #[arg(short, long, env = "EASYTIER_VERSION")]
    pub version: Option<String>,

    #[arg(long)]
    pub service_strict_start: bool,

    #[arg(long, hide = true)]
    pub desktop_lock_dir: Option<PathBuf>,

    #[arg(long, hide = true)]
    pub desktop_parent_lock_held: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct UninstallServiceArgs {
    #[arg(long)]
    pub purge: bool,

    #[arg(long, hide = true)]
    pub desktop_lock_dir: Option<PathBuf>,

    #[arg(long, hide = true)]
    pub desktop_parent_lock_held: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum DesktopCommand {
    /// 非交互安装并启动 EasyTier 服务
    Install(DesktopJsonArgs),

    /// Non-interactive service status for desktop clients.
    Status(DesktopJsonArgs),

    /// 非交互卸载 EasyTier 服务
    Uninstall(DesktopJsonArgs),

    /// 非交互更新 EasyTier 二进制并重启服务
    Update(DesktopJsonArgs),
}

#[derive(Args, Debug, Clone, Default)]
pub struct DesktopJsonArgs {
    #[arg(long)]
    pub json: bool,
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    crate::style::debug(&format!(
        "cli::run 开始: server={:?}, config_server={:?}, install_dir={:?}, debug={}, command={:?}",
        cli.server, cli.config_server, cli.install_dir, cli.debug, cli.command
    ));

    match cli.command.clone().unwrap_or_else(default_install_command) {
        Command::Install(args) => run_install(cli, args).await,
        Command::Update(args) => run_update(cli, args).await,
        Command::Uninstall(args) => run_uninstall(cli, args).await,
        Command::Status(args) => deploy::run_status(cli.install_dir, args.json).await,
        Command::Desktop(command) => desktop::run(cli, command).await,
        Command::InstallService(args) => run_install_service(cli, args).await,
        Command::UpgradeService(args) => run_upgrade_service(cli, args).await,
        Command::UninstallService(args) => run_uninstall_service(cli, args).await,
    }
}

fn default_install_command() -> Command {
    Command::Install(InstallArgs {
        version: std::env::var("EASYTIER_VERSION").ok(),
    })
}

async fn run_install(cli: Cli, args: InstallArgs) -> anyhow::Result<()> {
    let install_dir = cli
        .install_dir
        .clone()
        .unwrap_or_else(deploy::default_install_dir);
    if let Err(e) = deploy::check_install_dir_writable(&install_dir) {
        if !deploy::platform::is_elevated() {
            crate::style::warning("需要管理员权限，正在尝试自动提权...");
            let status = deploy::platform::relaunch_elevated()?;
            std::process::exit(status.code().unwrap_or(1));
        }
        return Err(e);
    }

    let config = Config::new(cli.server.clone())?;
    let token_store = TokenStore::new(config.credentials_path.clone());

    let mut client = ConsoleClient::new(&config.console_base_url, token_store.clone());

    match deploy::check_existing_install(&install_dir).await {
        ExistingAction::Continue => {}
        ExistingAction::UpdateRequested => {
            let release = client.get_latest_release().await?;
            return deploy::run_upgrade_from_console(&install_dir, &release, args.version.clone())
                .await;
        }
        ExistingAction::Handled(result) => return result,
    }

    client = ensure_logged_in(&config, token_store.clone(), client).await?;
    let (tenant, latest_release) = deploy::load_console_bootstrap(&client).await?;

    deploy::run_deploy(
        &config,
        &client,
        &tenant,
        &latest_release,
        cli.install_dir,
        cli.config_server,
        args.version,
    )
    .await
}

async fn run_update(cli: Cli, args: UpdateArgs) -> anyhow::Result<()> {
    let install_dir = cli
        .install_dir
        .clone()
        .unwrap_or_else(deploy::default_install_dir);
    deploy::service::find_easytier_cli(&install_dir)?;

    if let Some(version) = args.version {
        crate::style::debug(&format!(
            "进入 update 分支: install_dir={}, version={}, elevated={}",
            install_dir.display(),
            version,
            deploy::platform::is_elevated()
        ));
        return deploy::run_upgrade(&install_dir, &version).await;
    }

    let config = Config::new(cli.server.clone())?;
    let client = ConsoleClient::new(
        &config.console_base_url,
        TokenStore::new(config.credentials_path.clone()),
    );
    let release = client.get_latest_release().await?;
    deploy::run_upgrade_from_console(&install_dir, &release, None).await
}

async fn run_uninstall(cli: Cli, args: UninstallArgs) -> anyhow::Result<()> {
    let install_dir = cli
        .install_dir
        .clone()
        .unwrap_or_else(deploy::default_install_dir);
    crate::style::debug(&format!(
        "进入 uninstall 分支: install_dir={}, purge={}, elevated={}",
        install_dir.display(),
        args.purge,
        deploy::platform::is_elevated()
    ));
    deploy::run_uninstall(Some(install_dir), args.purge).await
}

async fn run_install_service(cli: Cli, args: InstallServiceArgs) -> anyhow::Result<()> {
    let install_dir = cli
        .install_dir
        .clone()
        .unwrap_or_else(deploy::default_install_dir);
    let cli_path = deploy::service::find_easytier_cli(&install_dir)?;
    let core_path = args
        .service_core_path
        .clone()
        .unwrap_or_else(|| install_dir.join(deploy::core_binary_name()));
    let config_url = read_service_config_url(&args)?;
    crate::style::debug(&format!(
        "进入 install-service 分支: install_dir={}, cli_path={}, core_path={}",
        install_dir.display(),
        cli_path.display(),
        core_path.display()
    ));
    let machine_id = args.service_machine_id.as_deref();
    if args.service_strict_start {
        deploy::run_desktop_install_service(
            install_dir,
            core_path,
            &config_url,
            machine_id,
            args.desktop_lock_dir,
            args.desktop_parent_lock_held,
        )
        .await
    } else {
        deploy::service::install_service(&cli_path, &core_path, &config_url, machine_id).await
    }
}

fn read_service_config_url(args: &InstallServiceArgs) -> anyhow::Result<String> {
    match (&args.service_config_url, &args.service_config_url_file) {
        (Some(_), Some(_)) => {
            anyhow::bail!("服务安装参数 service_config_url 和 service_config_url_file 只能指定一个")
        }
        (Some(config_url), None) => Ok(config_url.clone()),
        (None, Some(path)) => {
            let config_url = std::fs::read_to_string(path)
                .map_err(|err| anyhow::anyhow!("读取服务配置 URL 文件失败: {}", err))?
                .trim()
                .to_string();
            let _ = std::fs::remove_file(path);
            if config_url.is_empty() {
                anyhow::bail!("服务配置 URL 文件不能为空");
            }
            Ok(config_url)
        }
        (None, None) => anyhow::bail!("缺少服务安装参数 service_config_url"),
    }
}

async fn run_upgrade_service(cli: Cli, args: UpgradeServiceArgs) -> anyhow::Result<()> {
    let install_dir = cli
        .install_dir
        .clone()
        .unwrap_or_else(deploy::default_install_dir);
    let version = args
        .version
        .clone()
        .ok_or_else(|| anyhow::anyhow!("缺少升级参数 version"))?;
    crate::style::debug(&format!(
        "进入 upgrade-service 分支: install_dir={}, version={}, elevated={}",
        install_dir.display(),
        version,
        deploy::platform::is_elevated()
    ));
    if args.service_strict_start {
        deploy::run_desktop_update_service(
            install_dir,
            &version,
            args.desktop_lock_dir,
            args.desktop_parent_lock_held,
        )
        .await
    } else {
        deploy::run_upgrade(&install_dir, &version).await
    }
}

async fn run_uninstall_service(cli: Cli, args: UninstallServiceArgs) -> anyhow::Result<()> {
    let install_dir = cli
        .install_dir
        .clone()
        .unwrap_or_else(deploy::default_install_dir);
    deploy::run_desktop_uninstall_service(
        install_dir,
        args.purge,
        args.desktop_lock_dir,
        args.desktop_parent_lock_held,
    )
    .await
}

async fn ensure_logged_in(
    config: &Config,
    token_store: TokenStore,
    mut client: ConsoleClient,
) -> anyhow::Result<ConsoleClient> {
    if !client.is_logged_in() {
        login::cmd_login(config, token_store.clone()).await?;
        return Ok(ConsoleClient::new(
            &config.console_base_url,
            token_store.clone(),
        ));
    }

    let me = match client.get_me().await {
        Ok(me) => me,
        Err(err) if is_unauthorized_error(&err) => {
            crate::style::warning("登录已失效，正在重新登录...");
            token_store.clear()?;
            login::cmd_login(config, token_store.clone()).await?;
            client = ConsoleClient::new(&config.console_base_url, token_store.clone());
            client.get_me().await?
        }
        Err(err) => return Err(err),
    };
    let user_label = if let Some(name) = &me.user.display_name {
        format!("{} <{}>", name, me.user.email)
    } else {
        me.user.email.clone()
    };
    crate::style::ok_kv("已登录:", &user_label);
    if !login::confirm_continue()? {
        token_store.clear()?;
        println!();
        login::cmd_login(config, token_store.clone()).await?;
        client = ConsoleClient::new(&config.console_base_url, token_store.clone());
    } else {
        println!();
    }

    Ok(client)
}

fn is_unauthorized_error(err: &anyhow::Error) -> bool {
    let text = err.to_string();
    text.contains(r#""code":"unauthorized""#) || text.contains("unauthorized")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn parses_without_subcommand_as_default_install() {
        let cli = Cli::parse_from(["easytier-pro-installer"]);

        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_install_subcommand() {
        let cli = Cli::parse_from(["easytier-pro-installer", "install", "--version", "v2.6.4"]);

        let Some(Command::Install(args)) = cli.command else {
            panic!("expected install command");
        };
        assert_eq!(args.version.as_deref(), Some("v2.6.4"));
    }

    #[test]
    fn parses_update_subcommand_with_global_args_after_subcommand() {
        let cli = Cli::parse_from([
            "easytier-pro-installer",
            "update",
            "--version",
            "v2.6.4",
            "--install-dir",
            "/tmp/easytier",
            "--debug",
        ]);

        let Some(Command::Update(args)) = cli.command else {
            panic!("expected update command");
        };
        assert_eq!(args.version.as_deref(), Some("v2.6.4"));
        assert_eq!(
            cli.install_dir.as_deref(),
            Some(std::path::Path::new("/tmp/easytier"))
        );
        assert!(cli.debug);
    }

    #[test]
    fn parses_uninstall_subcommand() {
        let cli = Cli::parse_from(["easytier-pro-installer", "uninstall", "--purge"]);

        let Some(Command::Uninstall(args)) = cli.command else {
            panic!("expected uninstall command");
        };
        assert!(args.purge);
    }

    #[test]
    fn parses_status_subcommand() {
        let cli = Cli::parse_from(["easytier-pro-installer", "status"]);

        let Some(Command::Status(args)) = cli.command else {
            panic!("expected status command");
        };
        assert!(!args.json);
    }

    #[test]
    fn parses_status_json_subcommand() {
        let cli = Cli::parse_from(["easytier-pro-installer", "status", "--json"]);

        let Some(Command::Status(args)) = cli.command else {
            panic!("expected status command");
        };
        assert!(args.json);
    }

    #[test]
    fn parses_desktop_install_json_subcommand() {
        let cli = Cli::parse_from(["easytier-pro-installer", "desktop", "install", "--json"]);

        let Some(Command::Desktop(DesktopCommand::Install(args))) = cli.command else {
            panic!("expected desktop install command");
        };
        assert!(args.json);
    }

    #[test]
    fn parses_desktop_status_json_subcommand() {
        let cli = Cli::parse_from(["easytier-pro-installer", "desktop", "status", "--json"]);

        let Some(Command::Desktop(DesktopCommand::Status(args))) = cli.command else {
            panic!("expected desktop status command");
        };
        assert!(args.json);
    }

    #[test]
    fn hides_desktop_subcommand_from_help() {
        let help = Cli::command().render_help().to_string();

        assert!(!help.contains("\n  desktop"));
        assert!(help.contains("\n  install    安装并完成首次部署"));
        assert!(help.contains("\n  update     更新已安装的 EasyTier"));
        assert!(help.contains("\n  uninstall  卸载 EasyTier 服务"));
        assert!(help.contains("\n  status     查看服务状态"));
    }

    #[test]
    fn rejects_removed_desktop_key_subcommands() {
        let err = Cli::try_parse_from(["easytier-pro-installer", "desktop", "list-keys", "--json"])
            .unwrap_err();

        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn rejects_legacy_top_level_status_flag() {
        let err = Cli::try_parse_from(["easytier-pro-installer", "--status"]).unwrap_err();

        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn rejects_legacy_top_level_version_flag() {
        let err =
            Cli::try_parse_from(["easytier-pro-installer", "--version", "v2.6.4"]).unwrap_err();

        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn parses_hidden_install_service_with_global_install_dir() {
        let cli = Cli::parse_from([
            "easytier-pro-installer",
            "install-service",
            "--service-core-path",
            "/tmp/easytier/easytier-core",
            "--service-config-url",
            "tcp://console.easytier.cn:22020/token",
            "--service-machine-id",
            "machine-id",
            "--service-strict-start",
            "--desktop-lock-dir",
            "/tmp/easytier-locks",
            "--desktop-parent-lock-held",
            "--install-dir",
            "/tmp/easytier",
        ]);

        let Some(Command::InstallService(args)) = cli.command else {
            panic!("expected install-service command");
        };
        assert_eq!(
            cli.install_dir.as_deref(),
            Some(std::path::Path::new("/tmp/easytier"))
        );
        assert_eq!(
            args.service_core_path.as_deref(),
            Some(std::path::Path::new("/tmp/easytier/easytier-core"))
        );
        assert_eq!(
            args.service_config_url.as_deref(),
            Some("tcp://console.easytier.cn:22020/token")
        );
        assert_eq!(args.service_machine_id.as_deref(), Some("machine-id"));
        assert!(args.service_strict_start);
        assert_eq!(
            args.desktop_lock_dir.as_deref(),
            Some(std::path::Path::new("/tmp/easytier-locks"))
        );
        assert!(args.desktop_parent_lock_held);
    }

    #[test]
    fn parses_hidden_install_service_config_url_file() {
        let cli = Cli::parse_from([
            "easytier-pro-installer",
            "install-service",
            "--service-core-path",
            "/tmp/easytier/easytier-core",
            "--service-config-url-file",
            "/tmp/easytier/service-config.secret",
        ]);

        let Some(Command::InstallService(args)) = cli.command else {
            panic!("expected install-service command");
        };
        assert_eq!(
            args.service_config_url_file.as_deref(),
            Some(std::path::Path::new("/tmp/easytier/service-config.secret"))
        );
        assert!(args.service_config_url.is_none());
    }

    #[test]
    fn parses_hidden_upgrade_service_with_global_install_dir() {
        let cli = Cli::parse_from([
            "easytier-pro-installer",
            "upgrade-service",
            "--version",
            "v2.6.4",
            "--service-strict-start",
            "--desktop-lock-dir",
            "/tmp/easytier-locks",
            "--desktop-parent-lock-held",
            "--install-dir",
            "/tmp/easytier",
        ]);

        let Some(Command::UpgradeService(args)) = cli.command else {
            panic!("expected upgrade-service command");
        };
        assert_eq!(
            cli.install_dir.as_deref(),
            Some(std::path::Path::new("/tmp/easytier"))
        );
        assert_eq!(args.version.as_deref(), Some("v2.6.4"));
        assert!(args.service_strict_start);
        assert_eq!(
            args.desktop_lock_dir.as_deref(),
            Some(std::path::Path::new("/tmp/easytier-locks"))
        );
        assert!(args.desktop_parent_lock_held);
    }

    #[test]
    fn parses_hidden_uninstall_service_with_global_install_dir() {
        let cli = Cli::parse_from([
            "easytier-pro-installer",
            "uninstall-service",
            "--purge",
            "--desktop-lock-dir",
            "/tmp/easytier-locks",
            "--desktop-parent-lock-held",
            "--install-dir",
            "/tmp/easytier",
        ]);

        let Some(Command::UninstallService(args)) = cli.command else {
            panic!("expected uninstall-service command");
        };
        assert_eq!(
            cli.install_dir.as_deref(),
            Some(std::path::Path::new("/tmp/easytier"))
        );
        assert!(args.purge);
        assert_eq!(
            args.desktop_lock_dir.as_deref(),
            Some(std::path::Path::new("/tmp/easytier-locks"))
        );
        assert!(args.desktop_parent_lock_held);
    }

    #[test]
    fn detects_unauthorized_errors() {
        let err = anyhow::Error::msg(r#"请求失败: {"code":"unauthorized","error":"unauthorized"}"#);
        assert!(is_unauthorized_error(&err));
    }

    #[test]
    fn ignores_other_errors() {
        let err = anyhow::anyhow!("请求失败: internal server error");
        assert!(!is_unauthorized_error(&err));
    }
}
