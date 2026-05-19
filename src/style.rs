use colored::Colorize;

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
}

/// 错误 ✗ 红色，输出到 stderr
pub fn error(msg: &str) {
    eprintln!("{} {}", "✗".red(), msg);
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
