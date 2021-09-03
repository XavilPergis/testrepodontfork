use std::collections::HashMap;
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream,
    },
    sync::mpsc,
};

use crate::common::{packet::*, CommonError};

#[derive(Debug)]
pub enum ServerError {
    Common(CommonError),
}

impl<T: Into<CommonError>> From<T> for ServerError {
    fn from(err: T) -> Self {
        ServerError::Common(err.into())
    }
}

pub type ServerResult<T> = Result<T, ServerError>;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SerializerMessage {
    Shutdown,
    Message { message: ServerToClientPacket },
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum DeserializerMessage {
    Connect {
        id: u64,
    },
    Disconnect {
        id: u64,
    },
    Message {
        id: u64,
        message: ClientToServerPacket,
    },
}

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

impl ClientDeserializerContext {
    async fn run_message_loop(&mut self) -> ServerResult<()> {
        while let Some(message) = read_whole_packet(&mut self.stream, &mut self.read_buffer).await?
        {
            log::trace!("d#{}: deserialized {:?}", self.connection_id, message);
            self.message_sender
                .send(DeserializerMessage::Message {
                    id: self.connection_id,
                    message,
                })
                .unwrap();
        }

        Ok(())
    }

    pub async fn run(&mut self) -> ServerResult<()> {
        self.message_sender
            .send(DeserializerMessage::Connect {
                id: self.connection_id,
            })
            .unwrap();
        let res = self.run_message_loop().await;
        self.message_sender
            .send(DeserializerMessage::Disconnect {
                id: self.connection_id,
            })
            .unwrap();
        res
    }
}

#[derive(Debug)]
pub struct ClientDeserializerContext {
    read_buffer: Vec<u8>,

    connection_id: u64,
    stream: OwnedReadHalf,
    message_sender: mpsc::UnboundedSender<DeserializerMessage>,
}

impl ClientDeserializerContext {
    pub fn new(
        connection_id: u64,
        stream: OwnedReadHalf,
        message_sender: mpsc::UnboundedSender<DeserializerMessage>,
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
    message_receiver: mpsc::UnboundedReceiver<SerializerMessage>,
}

impl ClientSerializerContext {
    pub fn new(
        connection_id: u64,
        stream: OwnedWriteHalf,
        message_receiver: mpsc::UnboundedReceiver<SerializerMessage>,
        _options: &ClientConnectionOptions,
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
            match message {
                SerializerMessage::Shutdown => break,
                SerializerMessage::Message { message } => {
                    write_whole_message(&mut self.stream, &mut self.write_buffer, &message).await?;
                    log::trace!("s#{}: serialized {:?}", self.connection_id, message);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ServerContext {
    current_client_id: u64,
    connections: HashMap<u64, ClientConnection>,
    running: bool,
}

impl ServerContext {
    fn new() -> Self {
        Self {
            current_client_id: 0,
            connections: HashMap::new(),
            running: true,
        }
    }

    fn handle_new_connection(&mut self, connection: ClientConnection) {
        self.connections.insert(connection.id, connection);
    }

    fn handle_packet(&mut self, id: u64, packet: ClientToServerPacket) {
        match packet {
            ClientToServerPacket::Connect => {
                for (&other_id, connection) in self.connections.iter() {
                    connection
                        .server_sender
                        .send(if id == other_id {
                            SerializerMessage::Message {
                                message: ServerToClientPacket::ConnectAck,
                            }
                        } else {
                            SerializerMessage::Message {
                                message: ServerToClientPacket::PeerConnected { peer_id: id },
                            }
                        })
                        .unwrap();
                }
            }

            ClientToServerPacket::Disconnect => {
                for (&other_id, connection) in self.connections.iter() {
                    connection
                        .server_sender
                        .send(if id == other_id {
                            SerializerMessage::Message {
                                message: ServerToClientPacket::DisconnectAck,
                            }
                        } else {
                            SerializerMessage::Message {
                                message: ServerToClientPacket::PeerDisonnected { peer_id: id },
                            }
                        })
                        .unwrap();
                }
            }

            ClientToServerPacket::Message { message } => {
                for (&other_id, connection) in self.connections.iter() {
                    connection
                        .server_sender
                        .send(if id == other_id {
                            SerializerMessage::Message {
                                message: ServerToClientPacket::MessageAck,
                            }
                        } else {
                            SerializerMessage::Message {
                                message: ServerToClientPacket::PeerMessage {
                                    peer_id: id,
                                    message: message.clone(),
                                },
                            }
                        })
                        .unwrap();
                }
            }

            ClientToServerPacket::Shutdown => {
                self.running = false;
            }

            ClientToServerPacket::RequestPeerListing => {
                self.connections[&id]
                    .server_sender
                    .send(SerializerMessage::Message {
                        message: ServerToClientPacket::PeerListingResponse {
                            peers: self
                                .connections
                                .keys()
                                .copied()
                                .filter(|&key| key != id)
                                .collect(),
                        },
                    })
                    .unwrap();
            }

            ClientToServerPacket::RequestPeerInfo { peer_ids: _ } => {}
        }
    }

    fn handle_deserializer_message(&mut self, message: DeserializerMessage) {
        match message {
            DeserializerMessage::Message { id, message } => {
                if self.connections.contains_key(&id) {
                    self.handle_packet(id, message);
                } else {
                    log::warn!("got packet from dead connection #{}", id);
                }
            }
            DeserializerMessage::Connect { id: _ } => {}
            DeserializerMessage::Disconnect { id } => {
                self.connections.remove(&id);
            }
        }
    }
}

fn create_client_connection(
    connection_id: u64,
    stream: TcpStream,
    client_tx: mpsc::UnboundedSender<DeserializerMessage>,
    options: ClientConnectionOptions,
) -> ClientConnection {
    let (server_tx, server_rx) = mpsc::unbounded_channel::<SerializerMessage>();
    let connection = ClientConnection::new(connection_id, server_tx);

    let (tcp_reader, tcp_writer) = stream.into_split();
    let mut serializer =
        ClientSerializerContext::new(connection_id, tcp_writer, server_rx, &options);
    tokio::spawn(async move {
        match serializer.run().await {
            Ok(_) => {}
            Err(err) => log::warn!("#{}: serializer error: {:?}", connection_id, err),
        }
    });

    let mut deserializer =
        ClientDeserializerContext::new(connection_id, tcp_reader, client_tx, &options);
    tokio::spawn(async move {
        match deserializer.run().await {
            Ok(_) => {}
            Err(err) => log::warn!("#{}: deserializer error: {:?}", connection_id, err),
        }
    });

    connection
}

#[derive(Debug)]
pub struct ClientConnection {
    id: u64,
    server_sender: mpsc::UnboundedSender<SerializerMessage>,
}

impl ClientConnection {
    pub fn new(id: u64, server_sender: mpsc::UnboundedSender<SerializerMessage>) -> Self {
        Self { id, server_sender }
    }
}

async fn handle_incoming_connections(
    connection_tx: mpsc::UnboundedSender<ClientConnection>,
    message_tx: mpsc::UnboundedSender<DeserializerMessage>,
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
        mpsc::unbounded_channel::<DeserializerMessage>();

    tokio::spawn(handle_incoming_connections(
        connection_tx,
        client_message_tx,
    ));

    loop {
        tokio::select! {
            connection = connection_rx.recv() => {
                server_context.handle_new_connection(connection.unwrap());
            }

            message = client_message_rx.recv() => {
                // TODO: proper cleanup
                server_context.handle_deserializer_message(message.unwrap());
                if !server_context.running {
                    break;
                }
            }
        }
    }

    Ok(())
}
