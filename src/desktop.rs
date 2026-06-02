use crate::cli::{Cli, DesktopCommand, DesktopJsonArgs};
use crate::config::Config;
use crate::deploy::{self, platform};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;

pub(crate) async fn run(cli: Cli, command: DesktopCommand) -> anyhow::Result<()> {
    let result = match command {
        DesktopCommand::Install(args) => run_install(&cli, args).await,
        DesktopCommand::Status(args) => run_status(&cli, args).await,
        DesktopCommand::Uninstall(args) => run_uninstall(&cli, args).await,
        DesktopCommand::Update(args) => run_update(&cli, args).await,
    };

    if let Err(err) = &result {
        let _ = emit_error(err);
    }
    result
}

async fn run_install(cli: &Cli, args: DesktopJsonArgs) -> anyhow::Result<()> {
    ensure_json(args)?;
    let req: InstallRequest = read_request()?;
    let bootstrap_token = required(req.bootstrap_token, "bootstrap_token")?;
    let version = required(req.version, "version")?;
    let config_server = resolve_config_server(cli, req.config_server)?;
    let install_dir = resolve_install_dir(cli, req.install_dir);

    let mut emit_event = |event, data| emit(event, data);
    deploy::run_desktop_install(
        Some(install_dir),
        config_server,
        bootstrap_token,
        version,
        &mut emit_event,
    )
    .await
}

async fn run_status(cli: &Cli, args: DesktopJsonArgs) -> anyhow::Result<()> {
    ensure_json(args)?;
    let req: StatusRequest = read_optional_request()?;
    let install_dir = resolve_install_dir(cli, req.install_dir);
    let config_server = resolve_optional_config_server(cli, req.config_server)?;

    let mut emit_event = |event, data| emit(event, data);
    deploy::run_desktop_status(
        Some(install_dir),
        config_server,
        req.bootstrap_token,
        req.version,
        &mut emit_event,
    )
    .await
}

async fn run_update(cli: &Cli, args: DesktopJsonArgs) -> anyhow::Result<()> {
    ensure_json(args)?;
    let req: UpdateRequest = read_request()?;
    let version = required(req.version, "version")?;
    let install_dir = resolve_install_dir(cli, req.install_dir);

    let mut emit_event = |event, data| emit(event, data);
    deploy::run_desktop_update(Some(install_dir), &version, &mut emit_event).await
}

async fn run_uninstall(cli: &Cli, args: DesktopJsonArgs) -> anyhow::Result<()> {
    ensure_json(args)?;
    let req: UninstallRequest = read_request()?;
    let install_dir = resolve_install_dir(cli, req.install_dir);

    let mut emit_event = |event, data| emit(event, data);
    deploy::run_desktop_uninstall(Some(install_dir), req.purge, &mut emit_event).await
}

fn ensure_json(args: DesktopJsonArgs) -> anyhow::Result<()> {
    if !args.json {
        anyhow::bail!("desktop 子命令必须显式传入 --json");
    }
    Ok(())
}

fn read_request<T: DeserializeOwned>() -> anyhow::Result<T> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    if input.trim().is_empty() {
        anyhow::bail!("stdin JSON 请求不能为空");
    }
    serde_json::from_str(&input).map_err(|err| anyhow::anyhow!("desktop JSON 请求无效: {}", err))
}

fn read_optional_request<T>() -> anyhow::Result<T>
where
    T: Default + DeserializeOwned,
{
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(T::default());
    }

    let mut input = String::new();
    stdin.read_to_string(&mut input)?;
    if input.trim().is_empty() {
        return Ok(T::default());
    }

    serde_json::from_str(&input)
        .map_err(|err| anyhow::anyhow!("desktop JSON 璇锋眰鏃犳晥: {}", err))
}

fn resolve_install_dir(cli: &Cli, request_dir: Option<PathBuf>) -> PathBuf {
    request_dir
        .or_else(|| cli.install_dir.clone())
        .unwrap_or_else(deploy::default_install_dir)
}

fn resolve_config_server(cli: &Cli, request_base: Option<String>) -> anyhow::Result<String> {
    let config = Config::new(cli.server.clone())?;
    platform::build_config_server_url(
        &config.console_base_url,
        request_base.or_else(|| cli.config_server.clone()),
        "",
    )
}

fn resolve_optional_config_server(
    cli: &Cli,
    request_base: Option<String>,
) -> anyhow::Result<Option<String>> {
    if request_base.is_none() && cli.config_server.is_none() {
        return Ok(None);
    }

    resolve_config_server(cli, request_base).map(Some)
}

fn required(value: Option<String>, field: &str) -> anyhow::Result<String> {
    let value = value.unwrap_or_default();
    if value.trim().is_empty() {
        anyhow::bail!("{} 不能为空", field);
    }
    Ok(value)
}

fn emit(event: &'static str, data: serde_json::Value) -> anyhow::Result<()> {
    #[derive(Serialize)]
    struct Event {
        event: &'static str,
        data: serde_json::Value,
    }

    let line = serde_json::to_string(&Event { event, data })?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{}", line)?;
    stdout.flush()?;
    Ok(())
}

fn emit_error(err: &anyhow::Error) -> anyhow::Result<()> {
    emit(
        "error",
        json!({
            "code": error_code(err),
            "message": err.to_string(),
        }),
    )
}

fn error_code(err: &anyhow::Error) -> &'static str {
    let text = err.to_string();
    if text.contains("bootstrap_token")
        || text.contains("version")
        || text.contains("unknown field")
        || text.contains("stdin JSON")
        || text.contains("desktop JSON")
        || text.contains("--json")
        || text.contains("desktop 子命令")
        || text.contains("不能为空")
    {
        "invalid_request"
    } else if text.contains("权限") || text.contains("permission") || text.contains("Permission")
    {
        "permission_denied"
    } else {
        "internal_error"
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallRequest {
    #[serde(default)]
    bootstrap_token: Option<String>,
    #[serde(default)]
    install_dir: Option<PathBuf>,
    #[serde(default)]
    config_server: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusRequest {
    #[serde(default)]
    bootstrap_token: Option<String>,
    #[serde(default)]
    install_dir: Option<PathBuf>,
    #[serde(default)]
    config_server: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    #[serde(default)]
    install_dir: Option<PathBuf>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UninstallRequest {
    #[serde(default)]
    install_dir: Option<PathBuf>,
    #[serde(default)]
    purge: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_non_empty_fields() {
        assert!(required(Some("v2.6.4".to_string()), "version").is_ok());
        assert!(required(None, "version").is_err());
        assert!(required(Some("  ".to_string()), "version").is_err());
    }

    #[test]
    fn maps_missing_required_fields_to_invalid_request() {
        let err = anyhow::Error::msg("version 不能为空");

        assert_eq!(error_code(&err), "invalid_request");
    }

    #[test]
    fn maps_unknown_fields_to_invalid_request() {
        let err = anyhow::Error::msg("unknown field `access_token`, expected `install_dir`");

        assert_eq!(error_code(&err), "invalid_request");
    }

    #[test]
    fn maps_malformed_json_to_invalid_request() {
        let err = anyhow::Error::msg("desktop JSON 请求无效: expected value at line 1 column 1");

        assert_eq!(error_code(&err), "invalid_request");
    }

    #[test]
    fn maps_missing_json_flag_to_invalid_request() {
        let err = anyhow::Error::msg("desktop 子命令必须显式传入 --json");

        assert_eq!(error_code(&err), "invalid_request");
    }

    #[test]
    fn rejects_access_token_fields() {
        assert!(serde_json::from_str::<InstallRequest>(r#"{"access_token":"token"}"#).is_err());
        assert!(serde_json::from_str::<StatusRequest>(r#"{"access_token":"token"}"#).is_err());
        assert!(serde_json::from_str::<UpdateRequest>(r#"{"access_token":"token"}"#).is_err());
        assert!(serde_json::from_str::<UninstallRequest>(r#"{"access_token":"token"}"#).is_err());
    }
}
