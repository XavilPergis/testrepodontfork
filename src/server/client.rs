use std::{collections::HashMap, pin::Pin, sync::Arc};

use async_trait::async_trait;
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        oneshot, watch,
    },
};

use crate::{
    common::{
        packet::{
            read_whole_packet, write_whole_packet, ClientId, ClientToServerPacket,
            ClientToServerPacketKind, RoomId, ServerToClientPacket, ServerToClientResponsePacket,
        },
        DynamicFuture,
    },
    server::room::BroadcastDisconnection,
};

use super::{
    get_responder,
    instance::{InstanceMessage, InstanceRef},
    room::{BroadcastMessage, QueryPeers, RoomRef},
    ActorReference, ErasedDispatcher, MessageHandler, Responder, ServerError, ServerResult,
};

// struct HandlerChannel<A: Actor> {
//     messages: Arc<UnboundedSender<A::Message>>,
// }

// async fn send_message<M, H: MessageHandler<M>>(
//     handler: &HandlerChannel<H>,
//     message: M,
// ) -> H::Response {
//     todo!()
// }

pub mod message {
    use crate::server::MessageHandler;

    use super::*;

    #[derive(Debug)]
    pub struct ClientMessage {
        room: RoomRef,
        client: ClientRef,
        message: Arc<String>,
    }

    #[async_trait]
    impl MessageHandler<ClientMessage> for Client {
        type Response = ();

        async fn handle(self: Pin<&mut Self>, message: ClientMessage) -> Self::Response {
            self.packet_sender
                .send(ServerToClientPacket::PeerMessage {
                    room_id: message.room.id(),
                    peer_id: message.client.id(),
                    message: (*message.message).clone(),
                })
                .unwrap();
        }
    }

    #[derive(Debug)]
    pub struct ClientConnected {
        room: RoomRef,
        client: ClientRef,
    }

    #[async_trait]
    impl MessageHandler<ClientConnected> for Client {
        type Response = ();

        async fn handle(self: Pin<&mut Self>, message: ClientConnected) -> Self::Response {
            self.packet_sender
                .send(ServerToClientPacket::PeerConnected {
                    room_id: message.room.id(),
                    peer_id: message.client.id(),
                })
                .unwrap();
        }
    }

    #[derive(Debug)]
    pub struct ClientDisconnected {
        room: RoomRef,
        client: ClientRef,
    }

    #[async_trait]
    impl MessageHandler<ClientDisconnected> for Client {
        type Response = ();

        async fn handle(self: Pin<&mut Self>, message: ClientDisconnected) -> Self::Response {
            self.packet_sender
                .send(ServerToClientPacket::PeerDisconnected {
                    room_id: message.room.id(),
                    peer_id: message.client.id(),
                })
                .unwrap();
        }
    }

    #[derive(Debug)]
    pub struct JoinRoom {
        room: RoomRef,
    }

    #[async_trait]
    impl MessageHandler<JoinRoom> for Client {
        type Response = ();

        async fn handle(self: Pin<&mut Self>, message: JoinRoom) -> Self::Response {
            todo!()
        }
    }

    #[derive(Debug)]
    pub struct LeaveRoom {
        room: RoomRef,
    }

    #[async_trait]
    impl MessageHandler<LeaveRoom> for Client {
        type Response = ();

        async fn handle(self: Pin<&mut Self>, message: LeaveRoom) -> Self::Response {
            todo!()
        }
    }
}

// #[derive(Debug)]
// pub enum ClientMessage {
//     ReceivePacket(ClientToServerPacket),
//     Disconnect,

//     ClientMessage(Responder<()>, message::ClientMessage),
//     ClientConnected(Responder<()>, message::ClientConnected),
//     ClientDisconnected(Responder<()>, message::ClientDisconnected),
//     JoinRoom(Responder<()>, message::JoinRoom),
//     LeaveRoom(Responder<()>, message::LeaveRoom),
// }

#[derive(Clone, Debug)]
struct ClientRefInner {
    channel: ActorReference<Client>,
    id: ClientId,
}

#[derive(Clone, Debug)]
pub struct ClientRef(Arc<ClientRefInner>);

impl ClientRef {
    pub fn new(id: ClientId, channel: ActorReference<Client>) -> Self {
        Self(Arc::new(ClientRefInner { channel, id }))
    }

    // pub fn send<M>(&self, message: M) -> ServerResult<()>
    // where
    //     Client: MessageHandler<M>,
    // {
    //     self.0
    //         .channel
    //         .send(message.into_message(Responder::empty()))
    //         .map_err(|_| ServerError::ResponderClosed)
    // }

    pub async fn execute<M>(
        &self,
        message: M,
    ) -> ServerResult<<Client as MessageHandler<M>>::Response>
    where
        Client: MessageHandler<M>,
        M: Send + 'static,
    {
        self.0.channel.execute(message).await
    }

    pub fn id(&self) -> ClientId {
        self.0.id
    }
}

#[derive(Debug)]
pub struct Client {
    reference: ClientRef,
    instance: InstanceRef,
    packet_sender: UnboundedSender<ServerToClientPacket>,

    rooms: HashMap<RoomId, RoomRef>,
}

// impl Actor for Client {
//     type Message = ClientMessage;

//     fn handle_message(self: Pin<&mut Self>, message: Self::Message) -> DynamicFuture<()> {
//         Box::pin(async move {
//             match message {
//                 ClientMessage::ReceivePacket(packet) => {
//                     // handle_receive_packet(self, packet_sender, packet).await
//                 }
//                 ClientMessage::Disconnect => todo!(),

//                 ClientMessage::ClientMessage(res, msg) => res.send(msg.execute(self)),
//                 ClientMessage::ClientConnected(res, msg) => res.send(msg.execute(self)),
//                 ClientMessage::ClientDisconnected(res, msg) => res.send(msg.execute(self)),
//                 ClientMessage::JoinRoom(res, msg) => res.send(msg.execute(self)),
//                 ClientMessage::LeaveRoom(res, msg) => res.send(msg.execute(self)),
//             }
//         })
//     }
// }

impl Client {
    pub fn new(
        reference: ClientRef,
        instance: InstanceRef,
        packet_sender: UnboundedSender<ServerToClientPacket>,
    ) -> Self {
        Self {
            packet_sender,
            reference,
            instance,
            rooms: HashMap::default(),
        }
    }
}

async fn handle_receive_packet(
    ctx: &mut Client,
    packet_sender: UnboundedSender<ServerToClientPacket>,
    packet: ClientToServerPacket,
) -> ServerResult<()> {
    let rid = packet.rid;
    match packet.kind {
        ClientToServerPacketKind::Connect { username } => {
            get_responder(|responder| {
                ctx.instance.send(InstanceMessage::UpdateClientUsername {
                    client: ctx.reference.id(),
                    username: Arc::new(username),
                    responder,
                })
            })
            .await?;

            packet_sender
                .send(ServerToClientPacket::Response {
                    rid,
                    packet: ServerToClientResponsePacket::ConnectAck {
                        client_id: ctx.reference.id(),
                    },
                })
                .unwrap();
        }

        ClientToServerPacketKind::Message { room, message } => {
            if let Some(room) = ctx.rooms.get(&room) {
                room.execute(BroadcastMessage {
                    sender: ctx.reference.clone(),
                    message: Arc::new(message),
                })
                .await?;

                packet_sender
                    .send(ServerToClientPacket::Response {
                        rid,
                        packet: ServerToClientResponsePacket::MessageAck {},
                    })
                    .unwrap();
            }
        }

        ClientToServerPacketKind::Shutdown {} => {}

        ClientToServerPacketKind::RequestPeerListing { room } => {
            if let Some(room) = ctx.rooms.get(&room) {
                let peers = room
                    .execute(QueryPeers {
                        client: ctx.reference.clone(),
                    })
                    .await?;

                packet_sender
                    .send(ServerToClientPacket::Response {
                        rid,
                        packet: ServerToClientResponsePacket::PeerListingResponse {
                            peers: peers.iter().map(|peer| peer.id()).collect(),
                        },
                    })
                    .unwrap();
            }
        }

        ClientToServerPacketKind::RequestPeerInfo { peer_ids } => {
            let mut peer_infos = HashMap::new();

            for peer in peer_ids {
                if let Some(info) = ctx.instance.query_client_info(peer).await? {
                    peer_infos.insert(peer, info);
                }
            }

            packet_sender
                .send(ServerToClientPacket::Response {
                    rid,
                    packet: ServerToClientResponsePacket::PeerInfoResponse { peers: peer_infos },
                })
                .unwrap();
        }

        ClientToServerPacketKind::RequestRoomListing {} => {
            let rooms = ctx.instance.query_room_list().await?;

            packet_sender
                .send(ServerToClientPacket::Response {
                    rid,
                    packet: ServerToClientResponsePacket::RoomListingResponse {
                        rooms: rooms.iter().map(|room| room.id()).collect(),
                    },
                })
                .unwrap();
        }

        ClientToServerPacketKind::RequestRoomInfo { room_ids } => {
            let mut room_infos = HashMap::new();

            for room_id in room_ids {
                if let Some(info) = ctx.instance.query_room_info(room_id).await? {
                    room_infos.insert(room_id, info);
                }
            }

            packet_sender
                .send(ServerToClientPacket::Response {
                    rid,
                    packet: ServerToClientResponsePacket::RoomInfoResponse { rooms: room_infos },
                })
                .unwrap();
        }
    }

    Ok(())
}

// async fn handle_message(
//     ctx: &mut Client,
//     packet_sender: UnboundedSender<ServerToClientPacket>,
//     message: ClientMessage,
// ) -> ServerResult<bool> {
// match message {
//     ClientMessage::ReceivePacket(packet) => {
//         handle_receive_packet(ctx, packet_sender.clone(), packet).await?;
//     }
//     ClientMessage::Disconnect => {
//         for room in ctx.rooms.values() {
//             room.send(RoomMessage::BroadcastDisconnection(BroadcastDisconnection(
//                 BroadcastDisconnection {
//                     client: ctx.reference.clone(),
//                 },
//             )));
//         }
//         ctx.instance.send(InstanceMessage::DisconnectClient {
//             client: ctx.reference.id(),
//         });
//         return Ok(true);
//     }

//     ClientMessage::ClientMessage {
//         client,
//         room,
//         message,
//     } => {
//         handle_client_message(ctx, packet_sender.clone(), client, room, message);
//     }
//     ClientMessage::ClientConnected { client, room } => {
//         handle_client_connect(ctx, packet_sender.clone(), client, room);
//     }
//     ClientMessage::ClientDisconnected { client, room } => {
//         handle_client_disconnect(ctx, packet_sender.clone(), client, room);
//     }
//     ClientMessage::JoinRoom { room } => todo!(),
//     ClientMessage::LeaveRoom { room } => todo!(),
// }

//     Ok(false)
// }

pub async fn run_client(
    reference: ClientRef,
    instance: InstanceRef,
    mut messages: UnboundedReceiver<Box<dyn ErasedDispatcher<Client>>>,
    packet_sender: UnboundedSender<ServerToClientPacket>,
    force_shutdown: SignalSender,
) -> ServerResult<()> {
    let mut ctx = Box::pin(Client::new(
        reference.clone(),
        instance.clone(),
        packet_sender,
    ));

    log::info!("main client loop for {} starting", reference.id());

    while let Some(message) = messages.recv().await {
        message.handle(ctx.as_mut()).await;
    }

    log::info!("main client loop for {} shut down", reference.id());
    force_shutdown.signal();
    instance.send(InstanceMessage::DisconnectClient {
        client: reference.id(),
    });
    Ok(())
}

pub fn create_client(client_id: ClientId, stream: TcpStream, instance: InstanceRef) -> ClientRef {
    let (tx, rx) = mpsc::unbounded_channel();
    let client = ClientRef::new(client_id, ActorReference::new(tx.clone()));

    let (ser_tx, ser_rx) = mpsc::unbounded_channel();
    let (read_stream, write_stream) = stream.into_split();
    let (shutdown_tx, shutdown_rx) = signal_channel();

    tokio::spawn(run_serializer(client_id, write_stream, ser_rx));
    tokio::spawn(run_deserializer(
        client_id,
        read_stream,
        client.clone(),
        shutdown_rx,
    ));
    tokio::spawn(run_client(
        client.clone(),
        instance,
        rx,
        ser_tx,
        shutdown_tx,
    ));

    client
}

pub fn signal_channel() -> (SignalSender, SignalListener) {
    let (tx, rx) = watch::channel(());
    (SignalSender(tx), SignalListener(rx))
}

#[derive(Debug)]
pub struct SignalSender(watch::Sender<()>);

impl SignalSender {
    pub fn signal(&self) {
        let _ = self.0.send(());
    }
}

#[derive(Clone, Debug)]
pub struct SignalListener(watch::Receiver<()>);

impl SignalListener {
    pub async fn listen(&mut self) {
        let _ = self.0.changed().await;
    }
}

async fn run_deserializer(
    client_id: ClientId,
    mut read_stream: OwnedReadHalf,
    client_ref: ClientRef,
    mut force_shutdown: SignalListener,
) {
    async fn handle_packet(
        client_id: ClientId,
        read_stream: &mut OwnedReadHalf,
        read_buffer: &mut Vec<u8>,
        client_ref: &ClientRef,
    ) -> bool {
        match read_whole_packet(read_stream, read_buffer).await {
            Ok(Some(packet)) => {
                log::trace!("{} deserialized {:?}", client_id, packet);
                client_ref
                    .send(ClientMessage::ReceivePacket(packet))
                    .is_err()
            }
            Ok(None) => true,
            Err(err) => {
                log::error!("deserializer error: {:?}", err);
                true
            }
        }
    }

    let mut read_buffer = Vec::with_capacity(2048);
    loop {
        tokio::select! {
            shutdown = handle_packet(
                client_id,
                &mut read_stream,
                &mut read_buffer,
                &client_ref,
            ) => {
                if shutdown {
                    break;
                }
            }

            _ = force_shutdown.listen() => {
                break;
            }
        }
    }

    let _ = packet_channel.send(ClientMessage::Disconnect);
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
