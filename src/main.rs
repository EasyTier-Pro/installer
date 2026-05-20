use clap::Parser;

mod api;
mod auth;
mod cli;
mod config;
mod deploy;
mod style;

#[tokio::main]
async fn main() {
    let _ = ctrlc::set_handler(|| {
        let term = console::Term::stdout();
        let _ = term.show_cursor();
        std::process::exit(130);
    });

    let cli = cli::Cli::parse();
    if cli.debug {
        let path = style::default_debug_log_path();
        style::set_debug_log_path(path.clone());
        style::debug(&format!("main 启动: 调试日志文件={}", path.display()));
        style::debug(&format!(
            "main 启动: 原始参数={:?}",
            std::env::args().collect::<Vec<_>>()
        ));
    }
    style::debug("main 启动: CLI 参数解析完成");

    if let Err(e) = cli::run(cli).await {
        style::debug(&format!("main 退出: {}", e));
        style::error(&format!("{}", e));
        std::process::exit(1);
    }
}
