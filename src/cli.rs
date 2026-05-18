use crate::api::client::ConsoleClient;
use crate::auth::device_flow::DeviceFlow;
use crate::auth::token_store::TokenStore;
use crate::config::Config;
use crate::deploy;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "easytier-agent")]
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
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let config = Config::new(cli.server)?;
    let token_store = TokenStore::new(config.credentials_path.clone());

    if cli.status {
        return deploy::run_status(cli.install_dir).await;
    }

    if cli.uninstall {
        return deploy::run_uninstall(cli.install_dir).await;
    }

    let mut client = ConsoleClient::new(&config.console_base_url, token_store.clone());

    if !client.is_logged_in() {
        cmd_login(&config, token_store.clone()).await?;
        client = ConsoleClient::new(&config.console_base_url, token_store.clone());
    } else {
        let me = client.get_me().await?;
        let user_label = if let Some(name) = &me.user.display_name {
            format!("{} <{}>", name, me.user.email)
        } else {
            me.user.email.clone()
        };
        crate::style::ok_kv("已登录:", &user_label);
        if !confirm_continue()? {
            token_store.clear()?;
            println!();
            cmd_login(&config, token_store.clone()).await?;
            client = ConsoleClient::new(&config.console_base_url, token_store.clone());
        } else {
            println!();
        }
    }

    deploy::run_deploy(&config, &client, cli.install_dir, cli.config_server, cli.version).await
}

fn confirm_continue() -> anyhow::Result<bool> {
    let items = vec!["继续使用当前用户", "切换其他用户"];
    let selection = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
        .with_prompt("是否继续使用当前用户进行部署")
        .items(&items)
        .default(0)
        .interact()?;
    Ok(selection == 0)
}

async fn cmd_login(config: &Config, token_store: TokenStore) -> anyhow::Result<()> {
    let flow = DeviceFlow::new(&config.console_base_url);
    let info = flow.initiate().await?;

    crate::style::info("请在浏览器中打开下方链接完成登录：");
    crate::style::link(&info.verification_uri);
    println!();

    let pb = indicatif::ProgressBar::new_spinner();
    pb.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")?
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
    );
    pb.set_message("等待登录中...");

    let token = flow
        .poll_token(&info.device_code, info.interval, info.expires_in)
        .await?;
    pb.finish_and_clear();

    token_store.save(&token)?;
    crate::style::success("登录成功，凭证已保存");
    println!();
    Ok(())
}
