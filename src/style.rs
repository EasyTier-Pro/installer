use colored::Colorize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const REDACTED: &str = "<redacted>";
const SENSITIVE_ARGS: [&str; 2] = ["--service-config-url", "--config-server"];
const SENSITIVE_JSON_FIELDS: [&str; 4] = [
    "bootstrap_token",
    "access_token",
    "id_token",
    "refresh_token",
];

fn debug_log_slot() -> &'static Mutex<Option<PathBuf>> {
    static SLOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

pub fn default_debug_log_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("easytier-pro-installer.debug.log")
}

pub fn set_debug_log_path(path: PathBuf) {
    if let Ok(mut slot) = debug_log_slot().lock() {
        *slot = Some(path);
    }
}

pub fn debug_log_path() -> Option<PathBuf> {
    debug_log_slot().lock().ok().and_then(|slot| slot.clone())
}

pub fn debug_enabled() -> bool {
    debug_log_path().is_some()
}

pub(crate) fn redact_sensitive_args(args: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            redacted.push(REDACTED.to_string());
            redact_next = false;
            continue;
        }

        if SENSITIVE_ARGS.contains(&arg.as_str()) {
            redacted.push(arg.clone());
            redact_next = true;
            continue;
        }

        if let Some((name, _)) = arg.split_once('=')
            && SENSITIVE_ARGS.contains(&name)
        {
            redacted.push(format!("{}={}", name, REDACTED));
            continue;
        }

        redacted.push(arg.clone());
    }
    redacted
}

pub(crate) fn redact_config_server_url(value: &str) -> String {
    let lowercase = value.to_ascii_lowercase();
    if !lowercase.starts_with("tcp://") && !lowercase.starts_with("tcps://") {
        return value.to_string();
    }

    let without_trailing_slashes = value.trim_end_matches('/');
    let trailing_slashes = &value[without_trailing_slashes.len()..];
    let Some(scheme_end) = without_trailing_slashes.find("://") else {
        return value.to_string();
    };
    let path_start = scheme_end + 3;
    let Some(last_slash) = without_trailing_slashes[path_start..].rfind('/') else {
        return value.to_string();
    };
    let secret_start = path_start + last_slash + 1;
    if secret_start >= without_trailing_slashes.len() {
        return value.to_string();
    }

    format!(
        "{}{}{}",
        &without_trailing_slashes[..secret_start],
        REDACTED,
        trailing_slashes
    )
}

pub(crate) fn redact_sensitive_text(text: &str) -> String {
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) {
        redact_json_value(&mut value);
        return serde_json::to_string(&value).unwrap_or_else(|_| text.to_string());
    }

    let args = text
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if args.iter().any(|arg| {
        SENSITIVE_ARGS.contains(&arg.as_str())
            || arg
                .split_once('=')
                .is_some_and(|(name, _)| SENSITIVE_ARGS.contains(&name))
    }) {
        return redact_sensitive_args(&args).join(" ");
    }

    redact_config_server_url(text)
}

fn redact_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if SENSITIVE_JSON_FIELDS.contains(&key.as_str()) {
                    *value = serde_json::Value::String(REDACTED.to_string());
                } else {
                    redact_json_value(value);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                redact_json_value(value);
            }
        }
        serde_json::Value::String(value) => {
            *value = redact_config_server_url(value);
        }
        _ => {}
    }
}

fn append_log_line(level: &str, msg: &str) {
    let Some(path) = debug_log_path() else {
        return;
    };
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let pid = std::process::id();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{}][pid:{}][{}] {}", timestamp, pid, level, msg);
    }
}

/// 自定义 dialoguer 主题：●/○ 圆点指示器
pub fn dialoguer_theme() -> dialoguer::theme::ColorfulTheme {
    use console::style;
    dialoguer::theme::ColorfulTheme {
        active_item_prefix: style("● ".to_string()).green(),
        inactive_item_prefix: style("○ ".to_string()).dim(),
        ..Default::default()
    }
}

/// 成功状态 ✓ 绿色
pub fn success(msg: &str) {
    println!("{} {}", "✓".green(), msg);
}

/// 步骤提示 → 青色
pub fn info(msg: &str) {
    println!("{} {}", "→".cyan(), msg);
}

/// 警告 ! 黄色
pub fn warning(msg: &str) {
    println!("{} {}", "!".yellow(), msg);
    append_log_line("warn", msg);
}

/// 错误 ✗ 红色，输出到 stderr
pub fn error(msg: &str) {
    eprintln!("{} {}", "✗".red(), msg);
    append_log_line("error", msg);
}

/// 调试日志，输出到 stderr
pub fn debug(msg: &str) {
    if !debug_enabled() {
        return;
    }
    eprintln!("{} {}", "[debug]".dimmed(), msg.dimmed());
    append_log_line("debug", msg);
}

/// URL 链接：缩进 + 青色
pub fn link(url: &str) {
    println!("  {}", url.cyan());
}

/// 标签: 值，缩进显示
pub fn kv(label: &str, value: &str) {
    println!("  {} {}", label.bold(), value.white());
}

/// 成功标签: 值
pub fn ok_kv(label: &str, value: &str) {
    println!("{} {} {}", "✓".green(), label.bold(), value.white());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_separate_sensitive_args() {
        let args = vec![
            "install".to_string(),
            "--service-config-url".to_string(),
            "tcp://console.easytier.cn:22020/<secret>".to_string(),
            "--config-server".to_string(),
            "tcp://console.easytier.cn:22020/<secret>".to_string(),
        ];

        assert_eq!(
            redact_sensitive_args(&args),
            vec![
                "install".to_string(),
                "--service-config-url".to_string(),
                "<redacted>".to_string(),
                "--config-server".to_string(),
                "<redacted>".to_string(),
            ]
        );
    }

    #[test]
    fn redact_inline_sensitive_args() {
        let args = vec![
            "install".to_string(),
            "--service-config-url=tcp://console.easytier.cn:22020/<secret>".to_string(),
            "--config-server=tcp://console.easytier.cn:22020/<secret>".to_string(),
        ];

        assert_eq!(
            redact_sensitive_args(&args),
            vec![
                "install".to_string(),
                "--service-config-url=<redacted>".to_string(),
                "--config-server=<redacted>".to_string(),
            ]
        );
    }

    #[test]
    fn redact_json_token_fields() {
        let text = r#"{"outer":{"bootstrap_token":"<secret>","access_token":"<secret>"},"items":[{"id_token":"<secret>","refresh_token":"<secret>"}]}"#;
        let redacted = redact_sensitive_text(text);
        let value: serde_json::Value = serde_json::from_str(&redacted).expect("json");

        assert_eq!(value["outer"]["bootstrap_token"], "<redacted>");
        assert_eq!(value["outer"]["access_token"], "<redacted>");
        assert_eq!(value["items"][0]["id_token"], "<redacted>");
        assert_eq!(value["items"][0]["refresh_token"], "<redacted>");
    }

    #[test]
    fn redact_config_server_url_suffix() {
        assert_eq!(
            redact_config_server_url("tcp://console.easytier.cn:22020/<secret>"),
            "tcp://console.easytier.cn:22020/<redacted>"
        );
        assert_eq!(
            redact_config_server_url("tcp://console.easytier.cn:22020/<secret>/"),
            "tcp://console.easytier.cn:22020/<redacted>/"
        );
    }
}
