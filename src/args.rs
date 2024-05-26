use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub(crate) struct Config {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    Proxy(ProxyCommandArgs),
    Install(InstallCommandArgs),
    Cli(CliCommandArgs),
}

#[derive(Args, Debug)]
pub(crate) struct ProxyCommandArgs {
    #[arg(long, default_value = "localhost")]
    pub server_address: String,
    #[arg(long, default_value_t = 25565)]
    pub server_port: u16,
    #[arg(long, default_value = "0.0.0.0")]
    pub proxy_address: String,
    #[arg(long, default_value_t = 24565)]
    pub proxy_port: u16,
    #[arg(long, default_value = "127.0.0.1:4444")]
    pub socket: String
}

#[derive(Args, Debug)]
pub(crate) struct InstallCommandArgs {
    #[arg(long, default_value = "localhost")]
    pub server_address: String,
    #[arg(long, default_value_t = 25565)]
    pub server_port: u16,
    #[arg(long, default_value = "0.0.0.0")]
    pub proxy_address: String,
    #[arg(long, default_value_t = 24565)]
    pub proxy_port: u16,
    #[arg(long, default_value = "minecraft-maintenance-proxy.service")]
    pub service_name: String,
    #[arg(long, default_value = "127.0.0.1:4444")]
    pub socket: String
}

#[derive(Args, Debug)]
pub(crate) struct CliCommandArgs {
    #[arg(long, default_value = "127.0.0.1:4444")]
    pub socket: String,

    #[arg(long, default_value = "false")]
    pub enabling_proxy: String
}
