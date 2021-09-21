use async_trait::async_trait;
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::common::{
    packet::{ClientId, RoomId},
    DynamicFuture,
};

use super::{
    client::ClientRef, get_responder, instance::InstanceRef, ActorReference, ErasedDispatcher,
    MessageHandler, Responder, ServerError, ServerResult,
};

#[derive(Debug)]
pub struct BroadcastMessage {
    pub sender: ClientRef,
    pub message: Arc<String>,
}

#[async_trait]
impl MessageHandler<BroadcastMessage> for Room {
    type Response = ();

    async fn handle(self: Pin<&mut Self>, message: BroadcastMessage) -> Self::Response {
        for peer in self.clients.values() {
            if peer.id() != message.sender.id() {
                // peer.send(ClientMessage::ClientMessage {
                //     room: room.reference.clone(),
                //     client: self.sender.clone(),
                //     message: Arc::clone(&self.message),
                // })
            }
        }
    }
}

#[derive(Debug)]
pub struct BroadcastConnection {
    pub client: ClientRef,
}

#[async_trait]
impl MessageHandler<BroadcastConnection> for Room {
    type Response = ();

    async fn handle(self: Pin<&mut Self>, message: BroadcastConnection) -> Self::Response {
        for peer in self.clients.values() {
            if peer.id() != message.client.id() {
                // peer.send(ClientMessage::ClientConnected {
                //     room: room.reference.clone(),
                //     client: self.client.clone(),
                // })
            }
        }
    }
}

#[derive(Debug)]
pub struct BroadcastDisconnection {
    pub client: ClientRef,
}

#[async_trait]
impl MessageHandler<BroadcastDisconnection> for Room {
    type Response = ();

    async fn handle(self: Pin<&mut Self>, message: BroadcastDisconnection) -> Self::Response {
        for peer in self.clients.values() {
            if peer.id() != message.client.id() {
                // peer.send(ClientMessage::ClientDisconnected {
                //     room: room.reference.clone(),
                //     client: peer.clone(),
                // });
            }
        }
    }
}

#[derive(Debug)]
pub struct QueryPeers {
    pub client: ClientRef,
}

#[async_trait]
impl MessageHandler<QueryPeers> for Room {
    type Response = Vec<ClientRef>;

    async fn handle(self: Pin<&mut Self>, message: QueryPeers) -> Self::Response {
        self.clients
            .values()
            .filter(|peer| peer.id() != message.client.id())
            .cloned()
            .collect()
    }
}

#[derive(Debug)]
pub struct AddClient {
    pub client: ClientRef,
}

#[async_trait]
impl MessageHandler<AddClient> for Room {
    type Response = ();

    async fn handle(self: Pin<&mut Self>, message: AddClient) -> Self::Response {
        todo!()
    }
}

#[derive(Debug)]
pub struct RemoveClient {
    pub client: ClientId,
}

#[async_trait]
impl MessageHandler<RemoveClient> for Room {
    type Response = ();

    async fn handle(self: Pin<&mut Self>, message: RemoveClient) -> Self::Response {
        todo!()
    }
}

// #[derive(Debug)]
// pub enum RoomMessage {
//     BroadcastMessage(Responder<()>, BroadcastMessage),
//     BroadcastConnection(Responder<()>, BroadcastConnection),
//     BroadcastDisconnection(Responder<()>, BroadcastDisconnection),

//     QueryPeers(Responder<Vec<ClientRef>>, QueryPeers),

//     AddClient(Responder<()>, AddClient),
//     RemoveClient(Responder<()>, RemoveClient),
// }

#[derive(Clone, Debug)]
struct RoomRefInner {
    channel: ActorReference<Room>,
    id: RoomId,
}

#[derive(Clone, Debug)]
pub struct RoomRef(Arc<RoomRefInner>);

impl RoomRef {
    pub fn new(id: RoomId, channel: ActorReference<Room>) -> Self {
        Self(Arc::new(RoomRefInner { channel, id }))
    }

    pub async fn execute<M>(
        &self,
        message: M,
    ) -> ServerResult<<Room as MessageHandler<M>>::Response>
    where
        Room: MessageHandler<M>,
        M: Send + 'static,
    {
        self.0.channel.execute(message).await
    }

    // pub fn send<M>(&self, message: M) -> ServerResult<()>
    // where
    //     Room: MessageHandler<M>,
    // {
    //     self.0
    //         .channel
    //         .send(message.into_message(Responder::empty()))
    //         .map_err(|_| ServerError::ResponderClosed)
    // }

    // pub async fn execute<M>(&self, message: M) -> ServerResult<M::Response>
    // where
    //     Room: MessageHandler<M>,
    // {
    //     get_responder(|responder| {
    //         self.0
    //             .channel
    //             .send(message.into_message(responder))
    //             .unwrap()
    //     })
    //     .await
    // }

    pub fn id(&self) -> RoomId {
        self.0.id
    }
}

#[derive(Debug)]
struct Room {
    instance: InstanceRef,
    reference: RoomRef,

    clients: HashMap<ClientId, ClientRef>,
}

// impl Actor for Room {
//     type Message = RoomMessage;

//     fn handle_message(self: Pin<&mut Self>, message: Self::Message) -> DynamicFuture<()> {
//         Box::pin(async move {
//             match message {
//                 RoomMessage::BroadcastMessage(res, msg) => res.send(msg.execute(self)),
//                 RoomMessage::BroadcastConnection(res, msg) => res.send(msg.execute(self)),
//                 RoomMessage::BroadcastDisconnection(res, msg) => res.send(msg.execute(self)),
//                 RoomMessage::QueryPeers(res, msg) => res.send(msg.execute(self)),
//                 RoomMessage::AddClient(res, msg) => res.send(msg.execute(self)),
//                 RoomMessage::RemoveClient(res, msg) => res.send(msg.execute(self)),
//             }
//         })
//     }
// }

impl Room {
    fn new(reference: RoomRef, instance: InstanceRef) -> Self {
        Self {
            instance,
            reference,
            clients: HashMap::default(),
        }
    }
}

async fn run_room(
    reference: RoomRef,
    id: RoomId,
    instance: InstanceRef,
    mut messages: UnboundedReceiver<Box<dyn ErasedDispatcher<Room>>>,
) -> ServerResult<()> {
    let mut room = Box::pin(Room::new(reference, instance));
    while let Some(message) = messages.recv().await {
        message.handle(room.as_mut());
    }
    Ok(())
}

pub fn create_room(room_id: RoomId, instance: InstanceRef) -> RoomRef {
    let (tx, rx) = mpsc::unbounded_channel();
    let room = RoomRef::new(room_id, ActorReference::new(tx.clone()));

    tokio::spawn(run_room(room.clone(), room_id, instance.clone(), rx));

    room
}
