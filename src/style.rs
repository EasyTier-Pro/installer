use colored::Colorize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

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
