use crate::api::client::ConsoleClient;
use crate::auth::login;
use crate::auth::token_store::TokenStore;
use crate::config::Config;
use crate::deploy::{self, ExistingAction};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "easytier-pro-installer")]
#[command(about = "EasyTier 一键部署工具")]
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
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    crate::style::debug(&format!(
        "cli::run 开始: server={:?}, config_server={:?}, install_dir={:?}, version={:?}, status={}, uninstall={}, purge={}, debug={}",
        cli.server,
        cli.config_server,
        cli.install_dir,
        cli.version,
        cli.status,
        cli.uninstall,
        cli.purge,
        cli.debug
    ));
    let config = Config::new(cli.server.clone())?;
    let token_store = TokenStore::new(config.credentials_path.clone());

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

    match deploy::check_existing_install(&install_dir, cli.version.clone()).await {
        ExistingAction::Continue => {}
        ExistingAction::Handled(result) => return result,
    }

    let mut client = ConsoleClient::new(&config.console_base_url, token_store.clone());

    if !client.is_logged_in() {
        login::cmd_login(&config, token_store.clone()).await?;
        client = ConsoleClient::new(&config.console_base_url, token_store.clone());
    } else {
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
            login::cmd_login(&config, token_store.clone()).await?;
            client = ConsoleClient::new(&config.console_base_url, token_store.clone());
        } else {
            println!();
        }
    }

    deploy::run_deploy(
        &config,
        &client,
        cli.install_dir,
        cli.config_server,
        cli.version,
    )
    .await
}
