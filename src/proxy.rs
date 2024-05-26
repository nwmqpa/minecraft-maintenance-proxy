use base64::prelude::*;
use bytes::{Buf, BufMut, BytesMut};
use nom::{
    bytes::streaming::take,
    number::streaming::{be_i64, be_u16, be_u64, be_u8},
    IResult,
};
use rust_embed::Embed;
use serde::Serialize;
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch::Sender,
};

use crate::args;

#[derive(Embed)]
#[folder = "assets"]
struct Assets;

enum ServerboundPacket {
    Handshake {
        protocol_version: i32,
        server_address: String,
        server_port: u16,
        next_state: i32,
    },
    StatusRequest,
    PingRequest {
        payload: i64,
    },
    LoginStart {
        username: String,
    },
}

enum ClientboundPacket {
    PingResponse { payload: i64 },
    StatusResponse { json_response: String },
    DisconnectResponse { reason: String },
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum ConnectionState {
    Handshaking,
    Status,
    Login,
    Play,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    version: VersionResponse,
    description: DescriptionResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    players: Option<PlayersResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    favicon: Option<String>,
}

#[derive(Debug, Serialize)]
struct VersionResponse {
    name: String,
    protocol: i32,
}

#[derive(Debug, Serialize)]
struct PlayersResponse {
    max: i32,
    online: i32,
    sample: Vec<PlayerSample>,
}

#[derive(Debug, Serialize)]
struct PlayerSample {
    name: String,
    id: String,
}

#[derive(Debug, Serialize)]
struct DescriptionResponse {
    text: String,
}

const SEGMENT_BITS: u8 = 0x7f;
const CONTINUE_BIT: u8 = 0x80;

fn parse_varint(mut input: &[u8]) -> IResult<&[u8], i32> {
    let mut value = 0;
    let mut position = 0;

    loop {
        let (inner_input, current_byte) = be_u8(input)?;

        value |= ((current_byte & SEGMENT_BITS) as i32) << position;

        if current_byte & CONTINUE_BIT == 0 {
            return Ok((inner_input, value));
        }

        position += 7;

        if position > 32 {
            break Err(nom::Err::Failure(nom::error::Error::new(
                inner_input,
                nom::error::ErrorKind::TooLarge,
            )));
        }

        input = inner_input;
    }
}

fn write_varint(mut value: i32, buf: &mut BytesMut) -> usize {
    let mut bytes_written = 0;

    loop {
        let current_value = (value & 0xFF) as u8;

        if current_value & !SEGMENT_BITS == 0 {
            buf.put_u8(current_value);
            break bytes_written + 1;
        }

        buf.put_u8((current_value & SEGMENT_BITS) | CONTINUE_BIT);

        value >>= 7;

        bytes_written += 1;
    }
}

fn parse_string(_max_size: usize, input: &[u8]) -> IResult<&[u8], String> {
    let (input, length) = parse_varint(input)?;
    let (input, string) = take(length as usize)(input)?;

    Ok((input, String::from_utf8_lossy(string).to_string()))
}

fn parse_packet(
    input: &[u8],
    connection_state: ConnectionState,
) -> IResult<&[u8], ServerboundPacket> {
    let (input, packet_id) = parse_varint(input)?;

    match connection_state {
        ConnectionState::Handshaking => match packet_id {
            0x00 => {
                let (input, protocol_version) = parse_varint(input)?;
                let (input, server_address) = parse_string(255, input)?;
                let (input, server_port) = be_u16(input)?;
                let (input, next_state) = parse_varint(input)?;

                Ok((
                    input,
                    ServerboundPacket::Handshake {
                        protocol_version,
                        server_address,
                        server_port,
                        next_state,
                    },
                ))
            }
            _ => {
                println!("Packet ID: {packet_id}, Connection State: {connection_state:?}");
                unimplemented!()
            }
        },
        ConnectionState::Status => match packet_id {
            0x00 => Ok((input, ServerboundPacket::StatusRequest)),
            0x01 => {
                let (input, payload) = be_i64(input)?;

                Ok((input, ServerboundPacket::PingRequest { payload }))
            }
            _ => {
                println!("Packet ID: {packet_id}, Connection State: {connection_state:?}");
                unimplemented!()
            }
        },
        ConnectionState::Login => match packet_id {
            0x00 => {
                let (input, username) = parse_string(16, input)?;

                Ok((
                    input,
                    ServerboundPacket::LoginStart {
                        username,
                    },
                ))
            }
            _ => {
                println!("Packet ID: {packet_id}, Connection State: {connection_state:?}");
                unimplemented!()
            }
        },
        _ => unimplemented!(),
    }
}

fn write_packet(packet: ClientboundPacket) -> BytesMut {
    let packet_buf = match packet {
        ClientboundPacket::PingResponse { payload } => {
            let mut buf = BytesMut::with_capacity(9);
            buf.put_u8(0x01);
            buf.put_i64(payload);

            buf
        }
        ClientboundPacket::StatusResponse { json_response } => {
            let mut buf = BytesMut::with_capacity(3 + json_response.len());

            write_varint(0x00, &mut buf);
            write_varint(json_response.len() as i32, &mut buf);
            buf.put(json_response.as_bytes());

            buf
        }
        ClientboundPacket::DisconnectResponse { reason } => {
            let mut buf = BytesMut::with_capacity(3 + reason.len());

            write_varint(0x00, &mut buf);
            write_varint(reason.len() as i32, &mut buf);
            buf.put(reason.as_bytes());

            buf
        }
    };

    let mut length_buf = BytesMut::with_capacity(3 + packet_buf.len());
    let length = packet_buf.len() as i32;

    write_varint(length, &mut length_buf);
    length_buf.put(packet_buf);

    length_buf
}

/// See https://wiki.vg/Protocol#Packet_format
const PACKET_MAX_SIZE: usize = 2097151;

/// Packet length is a varint, which can be up to 3 bytes long
const PACKET_LENGTH_FIELD_MAX_SIZE: usize = 3;

async fn process_socket(
    mut socket: TcpStream,
    minecraft_socket_address: String,
    should_proxy: bool,
) -> io::Result<()> {
    if should_proxy {
        let mut egress = TcpStream::connect(&minecraft_socket_address).await?;

        match tokio::io::copy_bidirectional(&mut socket, &mut egress).await {
            Ok((to_egress, to_ingress)) => {
                println!(
                    "Connection ended gracefully ({to_egress} bytes from client, {to_ingress} bytes from server)"
                );
            }
            Err(err) => {
                println!("Error while proxying: {}", err);
            }
        }
        Ok(())
    } else {
        let mut buf = BytesMut::with_capacity(2 * PACKET_MAX_SIZE + 1);
        let mut connection_state = ConnectionState::Handshaking;
        let mut protocol_version = Option::<i32>::None;

        loop {
            socket.readable().await?;
            let n = socket.read_buf(&mut buf).await?;

            if n == 0 {
                break Ok(());
            }

            'parse_packets: loop {
                if buf.is_empty() {
                    break 'parse_packets;
                }

                let provisional_packet_length_field_max_size =
                    PACKET_LENGTH_FIELD_MAX_SIZE.clamp(1, buf.len());

                let (remainder, packet_length) =
                    parse_varint(&buf[..provisional_packet_length_field_max_size]).unwrap();

                if remainder.len() == 0 {
                    // Not enough data to parse packet after the length field
                    break 'parse_packets;
                }

                let packet_length_field_length =
                    provisional_packet_length_field_max_size - remainder.len();

                if buf.len() < packet_length as usize + packet_length_field_length {
                    // Not enough data to parse packet
                    break 'parse_packets;
                }

                buf.advance(packet_length_field_length);
                let packet_buf = buf.split_to(packet_length as usize);

                let (previous_data, packet) = parse_packet(&packet_buf, connection_state).unwrap();

                // Previous data should be empty
                assert_eq!(previous_data.len(), 0);

                match packet {
                    ServerboundPacket::Handshake {
                        protocol_version: packet_protocol_version,
                        next_state,
                        ..
                    } => {
                        protocol_version = Some(packet_protocol_version);

                        connection_state = match next_state {
                            1 => ConnectionState::Status,
                            2 => ConnectionState::Login,
                            _ => {
                                eprintln!("Invalid next state: {}", next_state);
                                break 'parse_packets;
                            }
                        };
                    }
                    ServerboundPacket::StatusRequest => {
                        let maintenance_icon = Assets::get("maintenance.png").unwrap();

                        let maintenace_icon_b64 =
                            BASE64_STANDARD.encode(maintenance_icon.data.as_ref());

                        let wrapped_cols = maintenace_icon_b64
                            .chars()
                            .collect::<Vec<_>>()
                            .chunks(76)
                            .map(|chars| chars.iter().collect::<String>())
                            .collect::<Vec<_>>()
                            .join("\n");

                        let status_response = StatusResponse {
                            version: VersionResponse {
                                name: "1.7.10".to_string(),
                                protocol: protocol_version.unwrap(),
                            },
                            description: DescriptionResponse {
                                text: "Server is currently in maintenance".to_string(),
                            },
                            players: None,
                            favicon: Some(format!("data:image/png;base64,{}", wrapped_cols)),
                        };

                        let json_response = serde_json::to_string(&status_response).unwrap();

                        let src = write_packet(ClientboundPacket::StatusResponse {
                            json_response: json_response,
                        });

                        socket.writable().await?;

                        socket.write_all(&src).await?;
                    }
                    ServerboundPacket::PingRequest { payload } => {
                        let src = write_packet(ClientboundPacket::PingResponse { payload });

                        socket.writable().await?;

                        socket.write_all(&src).await?;
                    }
                    ServerboundPacket::LoginStart { .. } => {
                        let src = write_packet(ClientboundPacket::DisconnectResponse {
                            reason: "{\"text\": \"Server is currently in maintenance\"}".to_string(),
                        });

                        socket.writable().await?;

                        socket.write_all(&src).await?;
                    }
                }
            }

            // Reserve space for the next packet
            buf.reserve(2 * PACKET_MAX_SIZE);
        }
    }
}

struct ChannelConfig {
    is_proxy: bool,
}

async fn process_control_socket(
    mut socket: TcpStream,
    tx: Sender<ChannelConfig>,
) -> anyhow::Result<()> {
    let mut buf = BytesMut::with_capacity(1);

    loop {
        socket.readable().await?;
        let n = socket.read_buf(&mut buf).await?;

        if n == 0 {
            break Ok(());
        }

        if buf.len() == 1 {
            let is_proxy = buf.get_u8() == 1;

            println!("Proxy flag set to {is_proxy}");

            tx.send(ChannelConfig { is_proxy })?;
        }
    }
}

pub(crate) async fn start_proxy(args: &args::ProxyCommandArgs) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::watch::channel(ChannelConfig { is_proxy: true });

    let proxy_address = &args.proxy_address;
    let proxy_port = args.proxy_port;

    let minecraft_address = &args.server_address;
    let minecraft_port = args.server_port;

    let minecraft_socket_address = format!("{minecraft_address}:{minecraft_port}");
    let mut should_proxy = true;

    let listener = TcpListener::bind(format!("{proxy_address}:{proxy_port}")).await?;
    let control_listener = TcpListener::bind(&args.socket).await?;

    loop {
        let mut rx = rx.clone();
        let tx = tx.clone();
        let minecraft_socket_address = minecraft_socket_address.clone();

        tokio::select! {
            _ = rx.changed() => {
                should_proxy = rx.borrow().is_proxy;
            },
            accepted_socket = listener.accept() => {
                if let Ok((socket, _)) = accepted_socket {
                    tokio::spawn(async move {
                        if let Err(why) = process_socket(socket, minecraft_socket_address, should_proxy).await {
                            eprintln!("Error: {}", why);
                        }
                    });
                } else {
                    anyhow::bail!("Error accepting connection");
                }
            }
            accepted_socket = control_listener.accept() => {
                if let Ok((socket, _)) = accepted_socket {
                    println!("Accepted control connection");
                    tokio::spawn(async move {
                        if let Err(why) = process_control_socket(socket, tx).await {
                            eprintln!("Error: {}", why);
                        }
                    });
                } else {
                    anyhow::bail!("Error accepting connection");
                }
            }
        }
    }
}
