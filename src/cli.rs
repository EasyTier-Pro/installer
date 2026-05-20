use crate::api::client::ConsoleClient;
use crate::auth::login;
use crate::auth::token_store::TokenStore;
use crate::config::Config;
use crate::deploy::{self, ExistingAction};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "easytier-pro-installer")]
#[command(about = "EasyTier Pro 一键部署工具")]
pub struct Cli {
    #[arg(short, long, env = "EASYTIER_CONSOLE_URL")]
    pub server: Option<String>,

    #[arg(long, env = "EASYTIER_CONFIG_SERVER")]
    pub config_server: Option<String>,

    #[arg(short, long, env = "EASYTIER_INSTALL_DIR")]
    pub install_dir: Option<PathBuf>,

    #[arg(short, long, env = "EASYTIER_VERSION")]
    pub version: Option<String>,

    /// 仅查看服务状态
    #[arg(long)]
    pub status: bool,

    /// 仅卸载服务
    #[arg(long)]
    pub uninstall: bool,

    /// 彻底删除安装目录和缓存压缩包
    #[arg(long)]
    pub purge: bool,

    /// 开启调试日志，默认写入当前目录下的 easytier-pro-installer.debug.log
    #[arg(long)]
    pub debug: bool,

    #[arg(long, hide = true)]
    pub install_service: bool,

    #[arg(long, hide = true)]
    pub upgrade: bool,

    #[arg(long, hide = true)]
    pub service_core_path: Option<PathBuf>,

    #[arg(long, hide = true)]
    pub service_config_url: Option<String>,

}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    crate::style::debug(&format!(
        "cli::run 开始: server={:?}, config_server={:?}, install_dir={:?}, version={:?}, status={}, uninstall={}, purge={}, debug={}, install_service={}, upgrade={}",
        cli.server,
        cli.config_server,
        cli.install_dir,
        cli.version,
        cli.status,
        cli.uninstall,
        cli.purge,
        cli.debug,
        cli.install_service,
        cli.upgrade
    ));

    if cli.status {
        return deploy::run_status(cli.install_dir).await;
    }

    if cli.uninstall {
        let install_dir = cli
            .install_dir
            .clone()
            .unwrap_or_else(deploy::default_install_dir);
        crate::style::debug(&format!(
            "进入 --uninstall 分支: install_dir={}, purge={}, elevated={}",
            install_dir.display(),
            cli.purge,
            deploy::platform::is_elevated()
        ));
        return deploy::run_uninstall(Some(install_dir), cli.purge).await;
    }

    if cli.install_service {
        let install_dir = cli
            .install_dir
            .clone()
            .unwrap_or_else(deploy::default_install_dir);
        let cli_path = deploy::service::find_easytier_cli(&install_dir)?;
        let core_path = cli
            .service_core_path
            .clone()
            .unwrap_or_else(|| install_dir.join(deploy::core_binary_name()));
        let config_url = cli
            .service_config_url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("缺少服务安装参数 service_config_url"))?;
        crate::style::debug(&format!(
            "进入 --install-service 分支: install_dir={}, cli_path={}, core_path={}",
            install_dir.display(),
            cli_path.display(),
            core_path.display()
        ));
        return deploy::service::install_service(&cli_path, &core_path, &config_url).await;
    }

    if cli.upgrade {
        let install_dir = cli
            .install_dir
            .clone()
            .unwrap_or_else(deploy::default_install_dir);
        let version = cli
            .version
            .clone()
            .ok_or_else(|| anyhow::anyhow!("缺少升级参数 version"))?;
        crate::style::debug(&format!(
            "进入 --upgrade 分支: install_dir={}, version={}, elevated={}",
            install_dir.display(),
            version,
            deploy::platform::is_elevated()
        ));
        return deploy::run_upgrade(&install_dir, &version).await;
    }

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
            return deploy::run_upgrade_from_console(
                &install_dir,
                &release,
                cli.version.clone(),
            )
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
        cli.version,
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
