use crate::api::client::ConsoleClient;
use crate::auth::login;
use crate::auth::token_store::TokenStore;
use crate::config::Config;
use crate::deploy::{self, ExistingAction};
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
    Status,

    #[command(name = "install-service", hide = true)]
    InstallService(InstallServiceArgs),

    #[command(name = "upgrade-service", hide = true)]
    UpgradeService(UpgradeServiceArgs),
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
pub struct InstallServiceArgs {
    #[arg(long)]
    pub service_core_path: Option<PathBuf>,

    #[arg(long)]
    pub service_config_url: Option<String>,

    #[arg(long)]
    pub service_machine_id: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct UpgradeServiceArgs {
    #[arg(short, long, env = "EASYTIER_VERSION")]
    pub version: Option<String>,
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
        Command::Status => deploy::run_status(cli.install_dir).await,
        Command::InstallService(args) => run_install_service(cli, args).await,
        Command::UpgradeService(args) => run_upgrade_service(cli, args).await,
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
    let (tenant, get_started) = deploy::load_console_bootstrap(&client).await?;

    deploy::run_deploy(
        &config,
        &client,
        &tenant,
        &get_started,
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
    let config_url = args
        .service_config_url
        .clone()
        .ok_or_else(|| anyhow::anyhow!("缺少服务安装参数 service_config_url"))?;
    crate::style::debug(&format!(
        "进入 install-service 分支: install_dir={}, cli_path={}, core_path={}",
        install_dir.display(),
        cli_path.display(),
        core_path.display()
    ));
    let machine_id = args.service_machine_id.as_deref();
    deploy::service::install_service(&cli_path, &core_path, &config_url, machine_id).await
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
    deploy::run_upgrade(&install_dir, &version).await
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

    let me = client.get_me().await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

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

        assert!(matches!(cli.command, Some(Command::Status)));
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
    }

    #[test]
    fn parses_hidden_upgrade_service_with_global_install_dir() {
        let cli = Cli::parse_from([
            "easytier-pro-installer",
            "upgrade-service",
            "--version",
            "v2.6.4",
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
    }
}
