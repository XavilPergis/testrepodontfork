use crate::common::packet::*;
use structopt::StructOpt;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
};

// pub mod client;
pub mod common;
pub mod server;

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Deserialize(PacketDeserializeError),
    Serialize(PacketSerializeError),
    ConnectionReset,
}

impl From<std::io::Error> for ClientError {
    fn from(err: std::io::Error) -> Self {
        ClientError::Io(err)
    }
}

impl From<PacketDeserializeError> for ClientError {
    fn from(err: PacketDeserializeError) -> Self {
        ClientError::Deserialize(err)
    }
}

impl From<PacketSerializeError> for ClientError {
    fn from(err: PacketSerializeError) -> Self {
        ClientError::Serialize(err)
    }
}

pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Debug, StructOpt)]
pub enum RunArguments {
    Server {},
    Client {},
}

async fn read_message(stream: &mut OwnedReadHalf) -> ClientResult<Option<ServerToClientPacket>> {
    let mut buf = Vec::new();
    let (message, _) = loop {
        // attempt to parse a packet first, so that if we get multiple
        // packets at a time, we can actually parse both of them instead of
        // having to wait for more data from the socket first
        match PacketDeserializerContext::new(&buf).parse() {
            Ok(message) => break message,
            Err(PacketDeserializeError::UnknownPacketLength) => {}
            Err(PacketDeserializeError::MismatchedPacketLength {
                buffer_length,
                expected_from_header,
            }) if buffer_length < expected_from_header => {}
            Err(err) => return Err(err.into()),
        }

        match stream.read_buf(&mut buf).await? {
            0 if buf.is_empty() => return Ok(None),
            // getting to this read means we have an incomplete packet, but
            // the connection was closed, so the packet can never be
            // finished. this is an error condition.
            0 => return Err(ClientError::ConnectionReset),
            _ => {}
        }
    };
    Ok(Some(message))
}

async fn send_message(
    stream: &mut OwnedWriteHalf,
    message: ClientToServerPacket,
) -> ClientResult<()> {
    let mut buffer = Vec::new();
    PacketSerializerContext::new(&mut buffer).serialize(&message)?;

    stream.write_u32(buffer.len() as u32 + 4).await?;
    stream.write_all(&mut buffer).await?;

    Ok(())
}

// async fn read_messages(mut stream: OwnedReadHalf) -> ClientResult<()> {
//     loop {
//         let message = read_message(&mut stream).await?.unwrap();
//         println!("got {:?}", message);
//     }

//     Ok(())
// }

async fn run_client() -> ClientResult<()> {
    let (mut tcp_read, mut tcp_write) = TcpStream::connect("127.0.0.1:8080").await?.into_split();
    // tokio::spawn(read_messages(tcp_read));

    send_message(&mut tcp_write, ClientToServerPacket::Connect).await?;
    println!("got {:?}", read_message(&mut tcp_read).await?.unwrap());
    send_message(&mut tcp_write, ClientToServerPacket::RequestPeerListing).await?;
    println!("got {:?}", read_message(&mut tcp_read).await?.unwrap());
    send_message(&mut tcp_write, ClientToServerPacket::Disconnect).await?;
    println!("got {:?}", read_message(&mut tcp_read).await?.unwrap());

    Ok(())
}

#[tokio::main]
async fn main() {
    match RunArguments::from_args() {
        RunArguments::Server {} => match server::run_server().await {
            Ok(_) => {}
            Err(err) => eprintln!("{:?}", err),
        },
        RunArguments::Client {} => match run_client().await {
            Ok(_) => {}
            Err(err) => eprintln!("{:?}", err),
        },
    }
}
