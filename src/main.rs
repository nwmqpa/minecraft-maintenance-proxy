mod args;
mod cli;
mod install;
mod proxy;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = args::Config::parse();

    match config.command {
        args::Commands::Cli(args) => cli::send_proxy_flag(&args).await,
        args::Commands::Proxy(args) => proxy::start_proxy(&args).await,
        args::Commands::Install(args) => install::install_systemd_service(&args),
    }
}
