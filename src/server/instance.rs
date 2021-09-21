use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use tokio::{
    net::TcpStream,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};

use crate::{
    common::packet::{ClientId, PeerInfo, RoomId, RoomInfo},
    server::client::create_client,
};

use super::{
    client::ClientRef,
    get_responder,
    room::{create_room, RoomRef},
    Responder, ServerResult,
};

#[derive(Debug)]
pub enum InstanceMessage {
    EstablishConnection {
        stream: TcpStream,
        addr: SocketAddr,
    },
    DisconnectClient {
        client: ClientId,
    },
    UpdateClientUsername {
        client: ClientId,
        username: Arc<String>,
        responder: Responder<()>,
    },

    QueryClientInfo {
        peer: ClientId,
        responder: Responder<Option<PeerInfo>>,
    },
    QueryRoomInfo {
        room_id: RoomId,
        responder: Responder<Option<RoomInfo>>,
    },
    QueryRoomList {
        responder: Responder<Vec<RoomRef>>,
    },
}

#[derive(Clone, Debug)]
pub struct InstanceRef {
    channel: UnboundedSender<InstanceMessage>,
}

impl InstanceRef {
    pub fn send<M: Into<InstanceMessage>>(&self, message: M) {
        self.channel.send(message.into()).unwrap()
    }

    pub async fn disconnect_client(&self, client: ClientId) {
        self.send(InstanceMessage::DisconnectClient { client });
    }

    pub async fn update_client_username(
        &self,
        client: ClientId,
        username: Arc<String>,
    ) -> ServerResult<()> {
        get_responder(|responder| {
            self.send(InstanceMessage::UpdateClientUsername {
                client,
                username,
                responder,
            })
        })
        .await
    }

    pub async fn query_client_info(&self, client: ClientId) -> ServerResult<Option<PeerInfo>> {
        get_responder(|responder| {
            self.send(InstanceMessage::QueryClientInfo {
                peer: client,
                responder,
            })
        })
        .await
    }

    pub async fn query_room_info(&self, room_id: RoomId) -> ServerResult<Option<RoomInfo>> {
        get_responder(|responder| self.send(InstanceMessage::QueryRoomInfo { room_id, responder }))
            .await
    }

    pub async fn query_room_list(&self) -> ServerResult<Vec<RoomRef>> {
        get_responder(|responder| self.send(InstanceMessage::QueryRoomList { responder })).await
    }
}

#[derive(Debug)]
pub struct Instance {
    reference: InstanceRef,

    next_client_id: ClientId,
    clients: HashMap<ClientId, ClientRef>,
    usernames: HashMap<ClientId, Arc<String>>,

    next_room_id: RoomId,
    rooms: HashMap<RoomId, RoomRef>,
    room_names: HashMap<RoomId, Arc<String>>,
}

impl Instance {
    pub fn new(reference: InstanceRef) -> Self {
        Self {
            reference,
            next_client_id: ClientId(0),
            clients: HashMap::default(),
            usernames: HashMap::default(),
            next_room_id: RoomId(0),
            rooms: HashMap::default(),
            room_names: HashMap::default(),
        }
    }

    fn next_client_id(&mut self) -> ClientId {
        let res = self.next_client_id;
        self.next_client_id.0 += 1;
        res
    }

    fn next_room_id(&mut self) -> RoomId {
        let res = self.next_room_id;
        self.next_room_id.0 += 1;
        res
    }
}

fn handle_create_room(instance: &mut Instance, name: Arc<String>) {
    let id = instance.next_room_id();
    let room = create_room(id, instance.reference.clone());
    // TODO: broadcast room creation
    // for client in instance.clients.values() {
    //     client.send(ClientMessage::);
    // }
    instance.room_names.insert(id, name);
    instance.rooms.insert(id, room);
    log::debug!("room id {} created", id);
}

fn handle_establish_connection(instance: &mut Instance, stream: TcpStream) {
    let id = instance.next_client_id();
    let client = create_client(id, stream, instance.reference.clone());
    instance.clients.insert(id, client);
    log::debug!("client id {} connected", id);
}

fn handle_disconnect_client(instance: &mut Instance, client: ClientId) {
    instance.clients.remove(&client);
    log::debug!("client id {} disconnected", client);
}

fn handle_query_peer_info(instance: &mut Instance, client: ClientId) -> Option<PeerInfo> {
    if !instance.clients.contains_key(&client) {
        return None;
    }

    Some(PeerInfo {
        username: instance.usernames.get(&client).map(|name| (**name).clone()),
    })
}

fn handle_query_room_info(instance: &mut Instance, room_id: RoomId) -> Option<RoomInfo> {
    Some(RoomInfo {
        name: (**instance.room_names.get(&room_id)?).clone(),
    })
}

fn handle_update_client_username(instance: &mut Instance, client: ClientId, username: Arc<String>) {
    // TODO: broadcast username updates
    instance.usernames.insert(client, username);
}

fn handle_query_room_list(instance: &mut Instance) -> Vec<RoomRef> {
    instance.rooms.values().cloned().collect()
}

async fn run_instance(
    reference: InstanceRef,
    mut messages: UnboundedReceiver<InstanceMessage>,
) -> ServerResult<()> {
    let mut instance = Instance::new(reference);

    handle_create_room(&mut instance, String::from("general").into());
    handle_create_room(&mut instance, String::from("test").into());
    handle_create_room(&mut instance, String::from("unsafe_unsafe").into());

    while let Some(message) = messages.recv().await {
        match message {
            InstanceMessage::EstablishConnection { stream, .. } => {
                handle_establish_connection(&mut instance, stream)
            }
            InstanceMessage::DisconnectClient { client } => {
                handle_disconnect_client(&mut instance, client)
            }

            InstanceMessage::QueryClientInfo { peer, responder } => {
                responder.send(handle_query_peer_info(&mut instance, peer))
            }
            InstanceMessage::QueryRoomInfo { room_id, responder } => {
                responder.send(handle_query_room_info(&mut instance, room_id))
            }
            InstanceMessage::UpdateClientUsername {
                client,
                username,
                responder,
            } => responder.send(handle_update_client_username(
                &mut instance,
                client,
                username,
            )),
            InstanceMessage::QueryRoomList { responder } => {
                responder.send(handle_query_room_list(&mut instance))
            }
        }
    }
    Ok(())
}

pub fn create_instance() -> InstanceRef {
    let (tx, rx) = mpsc::unbounded_channel();
    let inst = InstanceRef { channel: tx };
    tokio::spawn(run_instance(inst.clone(), rx));
    inst
}
