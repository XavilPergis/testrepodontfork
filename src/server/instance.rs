use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use tokio::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
};

use crate::{
    common::packet::{ClientId, PeerInfo, ServerToClientPacket},
    server::client::create_client,
};

use super::{
    client::{ClientMessage, ClientRef, Responder},
    ServerResult,
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
    },

    BroadcastMessage {
        sender: ClientRef,
        message: Arc<String>,
        responder: Responder<()>,
    },
    BroadcastConnection {
        client: ClientRef,
        responder: Responder<()>,
    },
    BroadcastDisconnection {
        client: ClientRef,
    },

    QueryPeers {
        client: ClientRef,
        responder: Responder<Vec<ClientRef>>,
    },
    QueryPeerInfo {
        client: ClientRef,
        peer: ClientId,
        responder: Responder<PeerInfo>,
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
}

#[derive(Debug)]
pub struct Instance {
    reference: InstanceRef,

    next_client_id: ClientId,
    clients: HashMap<ClientId, ClientRef>,
    usernames: HashMap<ClientId, Arc<String>>,
}

impl Instance {
    pub fn new(reference: InstanceRef) -> Self {
        Self {
            reference,
            next_client_id: ClientId(0),
            clients: HashMap::default(),
            usernames: HashMap::default(),
        }
    }

    fn next_client_id(&mut self) -> ClientId {
        let res = self.next_client_id;
        self.next_client_id.0 += 1;
        res
    }
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

fn handle_broadcast_disconnection(instance: &mut Instance, client: ClientRef) {
    for peer in instance.clients.values() {
        if peer.id() != client.id() {
            peer.send(ClientMessage::ClientDisconnected {
                client: peer.clone(),
            });
        }
    }
}

fn handle_broadcast_connection(
    instance: &mut Instance,
    client: ClientRef,
    responder: Responder<()>,
) {
    for peer in instance.clients.values() {
        if peer.id() != client.id() {
            peer.send(ClientMessage::ClientConnected {
                client: client.clone(),
            })
        }
    }

    responder.signal();
}

fn handle_broadcast_message(
    instance: &mut Instance,
    sender: ClientRef,
    message: Arc<String>,
    responder: Responder<()>,
) {
    for peer in instance.clients.values() {
        if peer.id() != sender.id() {
            peer.send(ClientMessage::ClientMessage {
                client: sender.clone(),
                message: Arc::clone(&message),
            })
        }
    }

    responder.signal();
}

fn handle_query_peers(
    instance: &mut Instance,
    client: ClientRef,
    responder: Responder<Vec<ClientRef>>,
) {
    responder.send(
        instance
            .clients
            .values()
            .filter(|peer| peer.id() != client.id())
            .cloned()
            .collect(),
    );
}

fn handle_query_peer_info(
    instance: &mut Instance,
    client: ClientId,
    responder: Responder<PeerInfo>,
) {
    responder.send(PeerInfo {
        username: instance.usernames.get(&client).map(|name| (**name).clone()),
    });
}

fn handle_update_client_username(instance: &mut Instance, client: ClientId, username: Arc<String>) {
    // TODO: broadcast username updates
    instance.usernames.insert(client, username);
}

async fn run_instance(
    reference: InstanceRef,
    mut messages: UnboundedReceiver<InstanceMessage>,
) -> ServerResult<()> {
    let mut instance = Instance::new(reference);
    while let Some(message) = messages.recv().await {
        match message {
            InstanceMessage::EstablishConnection { stream, .. } => {
                handle_establish_connection(&mut instance, stream);
            }
            InstanceMessage::DisconnectClient { client } => {
                handle_disconnect_client(&mut instance, client);
            }
            InstanceMessage::BroadcastMessage {
                sender,
                message,
                responder,
            } => {
                handle_broadcast_message(&mut instance, sender, message, responder);
            }
            InstanceMessage::BroadcastConnection { client, responder } => {
                handle_broadcast_connection(&mut instance, client, responder);
            }
            InstanceMessage::BroadcastDisconnection { client } => {
                handle_broadcast_disconnection(&mut instance, client);
            }
            InstanceMessage::QueryPeers { client, responder } => {
                handle_query_peers(&mut instance, client, responder);
            }
            InstanceMessage::QueryPeerInfo {
                client,
                peer,
                responder,
            } => handle_query_peer_info(&mut instance, peer, responder),
            InstanceMessage::UpdateClientUsername { client, username } => {
                handle_update_client_username(&mut instance, client, username)
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
