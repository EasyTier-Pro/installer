use crate::auth::device_flow::DeviceFlow;
use crate::auth::token_store::TokenStore;
use crate::config::Config;

pub async fn cmd_login(config: &Config, token_store: TokenStore) -> anyhow::Result<()> {
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

pub fn confirm_continue() -> anyhow::Result<bool> {
    let items = vec!["继续使用当前用户", "切换其他用户"];
    let selection = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
        .with_prompt("是否继续使用当前用户进行部署")
        .items(&items)
        .default(0)
        .interact()?;
    Ok(selection == 0)
}
