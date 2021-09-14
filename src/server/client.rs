use std::{collections::HashMap, sync::Arc};

use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};

use crate::common::packet::{
    read_whole_packet, write_whole_packet, ClientId, ClientToServerPacket,
    ClientToServerPacketKind, ServerToClientPacket, ServerToClientResponsePacket,
};

use super::{
    get_responder,
    instance::{InstanceMessage, InstanceRef},
    room::RoomRef,
    ServerResult,
};

#[derive(Debug)]
pub enum ClientMessage {
    ReceivePacket(ClientToServerPacket),
    Disconnect,

    ClientMessage {
        client: ClientRef,
        message: Arc<String>,
    },
    ClientConnected {
        client: ClientRef,
    },
    ClientDisconnected {
        client: ClientRef,
    },
}

#[derive(Clone, Debug)]
struct ClientRefInner {
    channel: UnboundedSender<ClientMessage>,
    id: ClientId,
}

#[derive(Clone, Debug)]
pub struct ClientRef(Arc<ClientRefInner>);

impl ClientRef {
    pub fn new(id: ClientId, channel: UnboundedSender<ClientMessage>) -> Self {
        Self(Arc::new(ClientRefInner { channel, id }))
    }

    pub fn send<M: Into<ClientMessage>>(&self, message: M) {
        self.0.channel.send(message.into()).unwrap()
    }

    pub fn id(&self) -> ClientId {
        self.0.id
    }
}

struct Client {
    reference: ClientRef,
    instance: InstanceRef,

    rooms: Vec<RoomRef>,
}

impl Client {
    pub fn new(reference: ClientRef, instance: InstanceRef) -> Self {
        Self {
            reference,
            instance,
            rooms: Vec::default(),
        }
    }
}

async fn handle_receive_packet(
    ctx: &mut Client,
    packet_sender: UnboundedSender<ServerToClientPacket>,
    packet: ClientToServerPacket,
) {
    let rid = packet.rid;
    match packet.kind {
        ClientToServerPacketKind::Connect { username } => {
            ctx.instance.send(InstanceMessage::UpdateClientUsername {
                client: ctx.reference.id(),
                username: Arc::new(username),
            });
            get_responder(|responder| {
                ctx.instance.send(InstanceMessage::BroadcastConnection {
                    client: ctx.reference.clone(),
                    responder,
                })
            })
            .await;

            packet_sender
                .send(ServerToClientPacket::Response {
                    rid,
                    packet: ServerToClientResponsePacket::ConnectAck {
                        client_id: ctx.reference.id(),
                    },
                })
                .unwrap();
        }

        ClientToServerPacketKind::Message { message } => {
            get_responder(|responder| {
                ctx.instance.send(InstanceMessage::BroadcastMessage {
                    sender: ctx.reference.clone(),
                    message: Arc::new(message),
                    responder,
                })
            })
            .await;

            packet_sender
                .send(ServerToClientPacket::Response {
                    rid,
                    packet: ServerToClientResponsePacket::MessageAck {},
                })
                .unwrap();
        }

        ClientToServerPacketKind::Shutdown {} => {}
        ClientToServerPacketKind::RequestPeerListing {} => {
            let peers = get_responder(|responder| {
                ctx.instance.send(InstanceMessage::QueryPeers {
                    client: ctx.reference.clone(),
                    responder,
                })
            })
            .await;

            packet_sender
                .send(ServerToClientPacket::Response {
                    rid,
                    packet: ServerToClientResponsePacket::PeerListingResponse {
                        peers: peers.iter().map(|peer| peer.id()).collect(),
                    },
                })
                .unwrap();
        }
        ClientToServerPacketKind::RequestPeerInfo { peer_ids } => {
            let mut peer_infos = HashMap::new();

            for peer in peer_ids {
                let info = get_responder(|responder| {
                    ctx.instance
                        .send(InstanceMessage::QueryPeerInfo { peer, responder })
                })
                .await;
                peer_infos.insert(peer, info);
            }

            packet_sender
                .send(ServerToClientPacket::Response {
                    rid,
                    packet: ServerToClientResponsePacket::PeerInfoResponse { peers: peer_infos },
                })
                .unwrap();
        }
    }
}

fn handle_client_message(
    ctx: &mut Client,
    packet_sender: UnboundedSender<ServerToClientPacket>,
    client: ClientRef,
    message: Arc<String>,
) {
    packet_sender
        .send(ServerToClientPacket::PeerMessage {
            peer_id: client.id(),
            message: (*message).clone(),
        })
        .unwrap();
}

fn handle_client_connect(
    ctx: &mut Client,
    packet_sender: UnboundedSender<ServerToClientPacket>,
    client: ClientRef,
) {
    packet_sender
        .send(ServerToClientPacket::PeerConnected {
            peer_id: client.id(),
        })
        .unwrap();
}

fn handle_client_disconnect(
    ctx: &mut Client,
    packet_sender: UnboundedSender<ServerToClientPacket>,
    client: ClientRef,
) {
    packet_sender
        .send(ServerToClientPacket::PeerDisconnected {
            peer_id: client.id(),
        })
        .unwrap();
}

pub async fn run_client(
    reference: ClientRef,
    id: ClientId,
    instance: InstanceRef,
    mut messages: UnboundedReceiver<ClientMessage>,
    packet_sender: UnboundedSender<ServerToClientPacket>,
) -> ServerResult<()> {
    let mut ctx = Client::new(reference.clone(), instance.clone());
    while let Some(message) = messages.recv().await {
        match message {
            ClientMessage::ReceivePacket(packet) => {
                handle_receive_packet(&mut ctx, packet_sender.clone(), packet).await;
            }
            ClientMessage::Disconnect => {
                instance.send(InstanceMessage::BroadcastDisconnection { client: reference });
                instance.send(InstanceMessage::DisconnectClient { client: id });
                break;
            }

            ClientMessage::ClientMessage { client, message } => {
                handle_client_message(&mut ctx, packet_sender.clone(), client, message);
            }
            ClientMessage::ClientConnected { client } => {
                handle_client_connect(&mut ctx, packet_sender.clone(), client);
            }
            ClientMessage::ClientDisconnected { client } => {
                handle_client_disconnect(&mut ctx, packet_sender.clone(), client);
            }
        }
    }

    instance.send(InstanceMessage::DisconnectClient { client: id });
    Ok(())
}

pub fn create_client(client_id: ClientId, stream: TcpStream, instance: InstanceRef) -> ClientRef {
    let (tx, rx) = mpsc::unbounded_channel();
    let client = ClientRef::new(client_id, tx.clone());

    let (ser_tx, ser_rx) = mpsc::unbounded_channel();
    let (read_stream, write_stream) = stream.into_split();

    tokio::spawn(run_serializer(client_id, write_stream, ser_rx));
    tokio::spawn(run_deserializer(client_id, read_stream, tx));
    tokio::spawn(run_client(client.clone(), client_id, instance, rx, ser_tx));

    client
}

async fn run_deserializer(
    client_id: ClientId,
    mut read_stream: OwnedReadHalf,
    packet_channel: UnboundedSender<ClientMessage>,
) {
    let mut read_buffer = Vec::with_capacity(2048);
    let mut running = true;
    while running {
        let packet_res = read_whole_packet(&mut read_stream, &mut read_buffer).await;
        match packet_res {
            Ok(Some(packet)) => {
                log::trace!("{} deserialized {:?}", client_id, packet);
                running &= !packet_channel
                    .send(ClientMessage::ReceivePacket(packet))
                    .is_err()
            }
            Ok(None) => running = false,
            Err(err) => todo!("deserializer error: {:?}", err),
        }
    }

    packet_channel.send(ClientMessage::Disconnect).unwrap();
    log::debug!("deserializer for {} shut down", client_id);
}

async fn run_serializer(
    client_id: ClientId,
    mut write_stream: OwnedWriteHalf,
    mut packet_channel: UnboundedReceiver<ServerToClientPacket>,
) {
    let mut write_buffer = Vec::with_capacity(2048);
    while let Some(message) = packet_channel.recv().await {
        if let Err(err) = write_whole_packet(&mut write_stream, &mut write_buffer, &message).await {
            todo!("serializer error: {:?}", err);
        }
        log::trace!("{} serialized {:?}", client_id, message);
    }
    log::debug!("serializer for {} shut down", client_id);
}
