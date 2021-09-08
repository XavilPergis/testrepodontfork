use tokio::sync::mpsc;

use crate::{
    client::{connection_handler::ConnectionHandlerCommand, ChatLine},
    common::packet::{ClientToServerPacketKind, ServerToClientResponsePacket},
};

use super::{connection_handler::ClientTask, AppCommand, ClientResult};

async fn query_peers(task: &mut ClientTask, peers: Vec<u64>) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::RequestPeerInfo { peer_ids: peers })
        .await?;

    match task.recv().await.unwrap() {
        ServerToClientResponsePacket::PeerInfoResponse { peers } => {
            for (connection_id, info) in peers {
                task.send(ConnectionHandlerCommand::AddConnectionInfo {
                    connection_id,
                    info,
                })
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

    let connection_id = match task.recv().await.unwrap() {
        ServerToClientResponsePacket::ConnectAck { connection_id } => {
            task.send(ConnectionHandlerCommand::SetConnectionId(connection_id))
                .await?;
            query_peers(&mut task, vec![connection_id]).await?;
            connection_id
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
        connection_id,
        peer_ids: peers,
    }))
    .unwrap();

    Ok(())
}

pub async fn send_peer_message(
    mut task: ClientTask,
    cmd: mpsc::UnboundedSender<AppCommand>,
    self_id: u64,
    message: String,
) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::Message {
        message: message.clone(),
    })
    .await?;

    match task.recv().await.unwrap() {
        ServerToClientResponsePacket::MessageAck {} => {
            cmd.send(AppCommand::AddChatLine(ChatLine::Text {
                peer_id: self_id,
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
    peer_id: u64,
) -> ClientResult<()> {
    cmd.send(AppCommand::AddChatLine(ChatLine::Connected { peer_id }))
        .unwrap();
    query_peers(&mut task, vec![peer_id]).await?;

    Ok(())
}
