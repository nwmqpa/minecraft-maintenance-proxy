use tokio::{io::AsyncWriteExt, net::TcpStream};

use crate::args;

pub(crate) async fn send_proxy_flag(args: &args::CliCommandArgs) -> anyhow::Result<()> {
    let socket = args.socket.clone();

    let socket = TcpStream::connect(socket).await?;

    let mut socket = tokio::io::BufStream::new(socket);

    if args.enabling_proxy == "true" {
        socket.write(&[1]).await?;
    } else {
        socket.write(&[0]).await?;
    };

    socket.flush().await?;
    Ok(())
}