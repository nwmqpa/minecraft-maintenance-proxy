use crate::args;
use anyhow::Context;

#[cfg(target_os = "linux")]
pub(crate) fn install_systemd_service(args: &args::InstallCommandArgs) -> anyhow::Result<()> {
    if !nix::unistd::Uid::effective().is_root() {
        anyhow::bail!("You must run this executable with root permissions");
    }

    let (unit_file_name, unit_file) = match args {
        args::InstallCommandArgs {
            service_name,
            server_address,
            server_port,
            proxy_address,
            proxy_port,
            socket,
        } => {
            let executable = std::env::current_exe()?;
            let executable_location = executable.to_str().context("Invalid executable path")?;

            let service_content = format!(
                r#"
[Unit]
Description=Minecraft Maintenance Proxy
After=network.target

[Service]
Type=simple
User=root
Group=root
ExecStart={executable_location} proxy --socket {socket} --server-address {server_address} --server-port {server_port} --proxy-address {proxy_address} --proxy-port {proxy_port}

[Install]
WantedBy=multi-user.target
"#
            );

            (service_name, service_content)
        }
    };

    let service_path = format!(
        "/etc/systemd/system/{unit_file_name}"
    );

    std::fs::write(&service_path, unit_file)?;

    std::process::Command::new("systemctl")
        .args(&["daemon-reload"])
        .status()?;

    std::process::Command::new("systemctl")
        .args(&["enable", &unit_file_name])
        .status()?;

    std::process::Command::new("systemctl")
        .args(&["start", &unit_file_name])
        .status()?;

    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn install_systemd_service(_args: &args::InstallCommandArgs) -> anyhow::Result<()> {
    anyhow::bail!("This command is only supported on Linux");
}
