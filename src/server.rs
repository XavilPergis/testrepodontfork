use std::{collections::HashMap, convert::TryInto, sync::Arc};

use tokio::{
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream,
    },
    sync::mpsc,
};

#[derive(Debug)]
pub enum ServerError {
    Io(std::io::Error),
    PacketParse(PacketParseError),
    ConnectionReset,
}

impl From<std::io::Error> for ServerError {
    fn from(err: std::io::Error) -> Self {
        ServerError::Io(err)
    }
}

impl From<PacketParseError> for ServerError {
    fn from(err: PacketParseError) -> Self {
        ServerError::PacketParse(err)
    }
}

pub type ServerResult<T> = Result<T, ServerError>;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ConnectionTagged<T> {
    connection_id: u64,
    value: T,
}

// server -> client
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ServerMessage {
    ConnectAck,
    DisconnectAck,
    // TODO
    MessageAck,
}

pub const S2C_ID_CONNECT_ACK: u32 = 0;
pub const S2C_ID_DISCONNECT_ACK: u32 = 1;
pub const S2C_ID_MESSAGE_ACK: u32 = 2;

// client -> server
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ClientMessage {
    Connect,
    Disconnect,
    Message(String),
}

pub const C2S_ID_CONNECT: u32 = 0;
pub const C2S_ID_DISCONNECT: u32 = 1;
pub const C2S_ID_MESSAGE: u32 = 2;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ClientConnectionOptions {
    pub read_buffer_size: usize,
}

impl Default for ClientConnectionOptions {
    fn default() -> Self {
        Self {
            read_buffer_size: 4096,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PacketParseError {
    // These two variants indicate that more data should be coming in the future, so the parse should be retried later.
    UnknownPacketLength,
    MismatchedPacketLength {
        buffer_length: usize,
        expected_from_header: usize,
    },
    // these variants are normal parse errors, including the ones about running out of bytes
    OutOfBytes,
    MismatchedParsedLength {
        parsed_length: usize,
        expected_from_header: usize,
    },
    InvalidLength(usize),
    UnknownPacketId(u32),
    Utf8Error(std::str::Utf8Error),
}

impl From<std::str::Utf8Error> for PacketParseError {
    fn from(err: std::str::Utf8Error) -> Self {
        PacketParseError::Utf8Error(err)
    }
}

#[derive(Debug)]
pub struct PacketSerializerContext<'pkt> {
    buffer: &'pkt mut Vec<u8>,
}

impl<'pkt> PacketSerializerContext<'pkt> {
    pub fn new(buffer: &'pkt mut Vec<u8>) -> Self {
        Self { buffer }
    }

    fn serialize_u32(&mut self, value: u32) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    fn serialize_str(&mut self, value: &str) {
        self.serialize_u32(value.len() as u32);
        self.buffer.extend_from_slice(value.as_bytes());
    }

    fn serialize_message(&mut self, value: &str) {
        self.serialize_u32(C2S_ID_MESSAGE);
        self.serialize_str(value);
    }

    fn serialize_body(&mut self, packet: &ClientMessage) {
        match packet {
            ClientMessage::Connect => self.serialize_u32(C2S_ID_CONNECT),
            ClientMessage::Disconnect => self.serialize_u32(C2S_ID_DISCONNECT),
            ClientMessage::Message(msg) => self.serialize_message(msg),
        }
    }

    pub fn serialize(&mut self, packet: &ClientMessage) {
        let packet_start = self.buffer.len();
        // reserve space to put the packet length in later, afterv we know that information.
        self.serialize_u32(0);
        self.serialize_body(packet);
        let packet_end = self.buffer.len();

        let packet_length = (packet_end - packet_start) as u32;
        self.buffer[packet_start..packet_start + 4].clone_from_slice(&packet_length.to_be_bytes());
    }
}

#[derive(Debug)]
pub struct PacketParserContext<'pkt> {
    packet: &'pkt [u8],
    cursor: usize,
}

impl<'pkt> PacketParserContext<'pkt> {
    fn new(packet: &'pkt [u8]) -> Self {
        Self { packet, cursor: 0 }
    }

    fn parse_byte_sequence(&mut self, bytes: usize) -> Result<&'pkt [u8], PacketParseError> {
        if self.cursor + bytes > self.packet.len() {
            Err(PacketParseError::OutOfBytes)
        } else {
            let data = &self.packet[self.cursor..self.cursor + bytes];
            self.cursor += bytes;
            Ok(data)
        }
    }

    fn parse_u32(&mut self) -> Result<u32, PacketParseError> {
        let bytes = self.parse_byte_sequence(std::mem::size_of::<u32>())?;
        Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
    }

    fn parse_str(&mut self) -> Result<&'pkt str, PacketParseError> {
        let length = self.parse_u32()?;
        let bytes = self.parse_byte_sequence(length as usize)?;
        Ok(std::str::from_utf8(bytes)?)
    }

    fn parse_body(&mut self) -> Result<ClientMessage, PacketParseError> {
        Ok(match self.parse_u32()? {
            C2S_ID_CONNECT => ClientMessage::Connect,
            C2S_ID_DISCONNECT => ClientMessage::Disconnect,
            C2S_ID_MESSAGE => ClientMessage::Message(self.parse_str()?.into()),
            other => return Err(PacketParseError::UnknownPacketId(other)),
        })
    }

    pub fn parse(&mut self) -> Result<(ClientMessage, usize), PacketParseError> {
        if self.packet.len() < 4 {
            return Err(PacketParseError::UnknownPacketLength);
        } else {
            let length = self.parse_u32()? as usize;
            if length > self.packet.len() {
                return Err(PacketParseError::MismatchedPacketLength {
                    buffer_length: self.packet.len(),
                    expected_from_header: length,
                });
            } else if length < 4 {
                return Err(PacketParseError::InvalidLength(length));
            }

            self.packet = &self.packet[..length];
        }

        let message = self.parse_body()?;

        if self.cursor != self.packet.len() {
            return Err(PacketParseError::MismatchedParsedLength {
                parsed_length: self.cursor,
                expected_from_header: self.packet.len(),
            });
        }

        Ok((message, self.cursor))
    }
}

impl ClientDeserializerContext {
    async fn read_message(&mut self) -> ServerResult<Option<ClientMessage>> {
        let (message, parsed_length) = loop {
            // attempt to parse a packet first, so that if we get multiple
            // packets at a time, we can actually parse both of them instead of
            // having to wait for more data from the socket first
            match PacketParserContext::new(&self.read_buffer).parse() {
                Ok(message) => break message,
                Err(PacketParseError::UnknownPacketLength) => {}
                Err(PacketParseError::MismatchedPacketLength {
                    buffer_length,
                    expected_from_header,
                }) if buffer_length < expected_from_header => {}
                Err(err) => return Err(err.into()),
            }

            match self.stream.read_buf(&mut self.read_buffer).await? {
                0 if self.read_buffer.is_empty() => return Ok(None),
                // getting to this read means we have an incomplete packet, but
                // the connection was closed, so the packet can never be
                // finished. this is an error condition.
                0 => return Err(ServerError::ConnectionReset),
                _ => {}
            }
        };

        self.read_buffer.drain(..parsed_length);
        Ok(Some(message))
    }

    pub async fn run(&mut self) -> ServerResult<()> {
        while let Some(message) = self.read_message().await? {
            println!("got message {:?}", message);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ClientDeserializerContext {
    read_buffer: Vec<u8>,

    connection_id: u64,
    stream: OwnedReadHalf,
    message_sender: mpsc::UnboundedSender<ConnectionTagged<ClientMessage>>,
}

impl ClientDeserializerContext {
    pub fn new(
        connection_id: u64,
        stream: OwnedReadHalf,
        message_sender: mpsc::UnboundedSender<ConnectionTagged<ClientMessage>>,
        options: &ClientConnectionOptions,
    ) -> Self {
        Self {
            read_buffer: Vec::with_capacity(options.read_buffer_size),
            connection_id,
            stream,
            message_sender,
        }
    }
}

#[derive(Debug)]
pub struct ClientSerializerContext {
    write_buffer: Vec<u8>,

    connection_id: u64,
    stream: OwnedWriteHalf,
    message_receiver: mpsc::UnboundedReceiver<ConnectionTagged<ServerMessage>>,
}

impl ClientSerializerContext {
    pub fn new(
        connection_id: u64,
        stream: OwnedWriteHalf,
        message_receiver: mpsc::UnboundedReceiver<ConnectionTagged<ServerMessage>>,
        options: &ClientConnectionOptions,
    ) -> Self {
        Self {
            write_buffer: Vec::new(),
            connection_id,
            stream,
            message_receiver,
        }
    }

    pub async fn run(&mut self) -> ServerResult<()> {
        while let Some(message) = self.message_receiver.recv().await {
            println!("serializing {:?}", message);
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ServerContext {
    current_client_id: u64,
    connections: HashMap<u64, ClientConnection>,
}

impl ServerContext {
    fn new() -> Self {
        Self {
            current_client_id: 0,
            connections: HashMap::new(),
        }
    }
}

fn create_client_connection(
    connection_id: u64,
    stream: TcpStream,
    client_tx: mpsc::UnboundedSender<ConnectionTagged<ClientMessage>>,
    options: ClientConnectionOptions,
) -> ClientConnection {
    let (server_tx, server_rx) = mpsc::unbounded_channel::<ConnectionTagged<ServerMessage>>();
    let connection = ClientConnection::new(connection_id, server_tx);

    let (tcp_reader, tcp_writer) = stream.into_split();
    let mut serializer =
        ClientSerializerContext::new(connection_id, tcp_writer, server_rx, &options);
    tokio::spawn(async move {
        match serializer.run().await {
            Ok(_) => {}
            Err(err) => eprintln!("{:?}", err),
        }
    });

    let mut deserializer =
        ClientDeserializerContext::new(connection_id, tcp_reader, client_tx, &options);
    tokio::spawn(async move {
        match deserializer.run().await {
            Ok(_) => {}
            Err(err) => eprintln!("{:?}", err),
        }
    });

    connection
}

#[derive(Debug)]
pub struct ClientConnection {
    id: u64,
    server_sender: mpsc::UnboundedSender<ConnectionTagged<ServerMessage>>,
}

impl ClientConnection {
    pub fn new(
        id: u64,
        server_sender: mpsc::UnboundedSender<ConnectionTagged<ServerMessage>>,
    ) -> Self {
        Self { id, server_sender }
    }
}

async fn handle_incoming_connections(
    connection_tx: mpsc::UnboundedSender<ClientConnection>,
    message_tx: mpsc::UnboundedSender<ConnectionTagged<ClientMessage>>,
) -> ServerResult<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    for id in 0.. {
        let (stream, _) = listener.accept().await?;
        let conn = create_client_connection(
            id,
            stream,
            message_tx.clone(),
            ClientConnectionOptions::default(),
        );
        connection_tx.send(conn).unwrap();
    }
    Ok(())
}

pub async fn run_server() -> ServerResult<()> {
    let mut server_context = ServerContext::new();

    let (connection_tx, mut connection_rx) = mpsc::unbounded_channel::<ClientConnection>();
    let (client_message_tx, mut client_message_rx) =
        mpsc::unbounded_channel::<ConnectionTagged<ClientMessage>>();

    tokio::spawn(handle_incoming_connections(
        connection_tx,
        client_message_tx,
    ));

    loop {
        tokio::select! {
            connection = connection_rx.recv() => {
                let connection = connection.unwrap();
                server_context.connections.insert(connection.id, connection);
            }

            message = client_message_rx.recv() => {
                let message = message.unwrap();
                println!("AAAA {:?}", message);
            }
        }
    }

    Ok(())
}

/*

c2s-parse-task:
    - read incoming tcp stream
    - parse messages into in-memory format
    - send messages to central context

s2c-send-task:
    - write outgoing tcp stream
    - serialize messages into buffers to be sent
    - recieve messages from central context

central-context:
    - broadcast messages


*/
