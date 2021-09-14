use tokio::sync::mpsc;

use crate::{
    client::{connection_handler::ConnectionHandlerCommand, ChatLine},
    common::packet::{ClientId, ClientToServerPacketKind, ServerToClientResponsePacket},
};

use super::{connection_handler::ClientTask, AppCommand, ClientResult};

async fn query_peers(task: &mut ClientTask, peers: Vec<ClientId>) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::RequestPeerInfo { peer_ids: peers })
        .await?;

    match task.recv().await.unwrap() {
        ServerToClientResponsePacket::PeerInfoResponse { peers } => {
            for (client_id, info) in peers {
                task.send(ConnectionHandlerCommand::AddConnectionInfo { client_id, info })
                    .await?;
            }
        }
        _ => todo!(),
    }

    Ok(())
}

pub async fn negotiate_connection(
    mut task: ClientTask,
    cmd: mpsc::UnboundedSender<AppCommand>,
    username: String,
) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::Connect { username })
        .await?;

    let client_id = match task.recv().await.unwrap() {
        ServerToClientResponsePacket::ConnectAck { client_id } => {
            task.send(ConnectionHandlerCommand::SetConnectionId(client_id))
                .await?;
            query_peers(&mut task, vec![client_id]).await?;
            client_id
        }
        _ => todo!(),
    };

    task.send_packet(ClientToServerPacketKind::RequestPeerListing {})
        .await?;

    let peers = match task.recv().await.unwrap() {
        ServerToClientResponsePacket::PeerListingResponse { peers } => {
            for peer_id in peers.iter().copied() {
                task.send(ConnectionHandlerCommand::AddPeer(peer_id))
                    .await?;
            }
            query_peers(&mut task, peers.clone()).await?;
            peers
        }
        _ => todo!(),
    };

    cmd.send(AppCommand::AddChatLine(ChatLine::ConnectInfo {
        id: client_id,
        peer_ids: peers,
    }))
    .unwrap();

    Ok(())
}

pub async fn send_peer_message(
    mut task: ClientTask,
    cmd: mpsc::UnboundedSender<AppCommand>,
    self_id: ClientId,
    message: String,
) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::Message {
        message: message.clone(),
    })
    .await?;

    match task.recv().await.unwrap() {
        ServerToClientResponsePacket::MessageAck {} => {
            cmd.send(AppCommand::AddChatLine(ChatLine::Text {
                id: self_id,
                message,
            }))
            .unwrap();
        }
        _ => todo!(),
    }

    Ok(())
}

pub async fn handle_peer_connect(
    mut task: ClientTask,
    cmd: mpsc::UnboundedSender<AppCommand>,
    peer_id: ClientId,
) -> ClientResult<()> {
    cmd.send(AppCommand::AddChatLine(ChatLine::Connected { id: peer_id }))
        .unwrap();
    query_peers(&mut task, vec![peer_id]).await?;

    Ok(())
}
