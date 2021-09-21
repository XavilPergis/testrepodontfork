use tokio::sync::mpsc;

use crate::{
    client::{connection_handler::ConnectionHandlerCommand, ChatLine},
    common::packet::{ClientId, ClientToServerPacketKind, RoomId, ServerToClientResponsePacket},
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

async fn query_rooms(task: &mut ClientTask, rooms: Vec<RoomId>) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::RequestRoomInfo { room_ids: rooms })
        .await?;

    match task.recv().await.unwrap() {
        ServerToClientResponsePacket::RoomInfoResponse { rooms } => {
            for (room_id, info) in rooms {
                task.send(ConnectionHandlerCommand::AddRoomInfo { room_id, info })
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

    match task.recv().await.unwrap() {
        ServerToClientResponsePacket::ConnectAck { client_id } => {
            task.send(ConnectionHandlerCommand::SetConnectionId(client_id))
                .await?;
            query_peers(&mut task, vec![client_id]).await?;
        }
        _ => todo!(),
    };

    task.send_packet(ClientToServerPacketKind::RequestRoomListing {})
        .await?;

    match task.recv().await.unwrap() {
        ServerToClientResponsePacket::RoomListingResponse { rooms } => {
            for room_id in rooms.iter().copied() {
                task.send(ConnectionHandlerCommand::AddRoom(room_id))
                    .await?;
            }
            query_rooms(&mut task, rooms.clone()).await?;
            rooms
        }
        _ => todo!(),
    };

    // cmd.send(AppCommand::AddChatLine(ChatLine::ConnectInfo {
    //     id: client_id,
    //     peer_ids: peers,
    // }))
    // .unwrap();

    Ok(())
}

pub async fn send_peer_message(
    mut task: ClientTask,
    cmd: mpsc::UnboundedSender<AppCommand>,
    self_id: ClientId,
    message: String,
) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::Message {
        room: todo!(),
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
