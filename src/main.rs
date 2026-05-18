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

    if let Err(e) = cli::run(cli).await {
        style::error(&format!("{}", e));
        std::process::exit(1);
    }
}
