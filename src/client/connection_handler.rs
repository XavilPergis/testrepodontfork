use std::collections::HashMap;

use futures::Future;
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::mpsc,
};

use crate::common::{
    packet::{
        read_whole_packet, write_whole_packet, ClientId, ClientToServerPacket,
        ClientToServerPacketKind, PeerInfo, ResponseId, ServerToClientPacket,
        ServerToClientResponsePacket,
    },
    CommonResult,
};

use super::{ClientError, ClientResult};

#[derive(Clone, Debug, Eq, PartialEq)]
enum ConnectionHandlerEvent {
    Packet(ServerToClientPacket),
    DeadTask(ResponseId),
    Command(ConnectionHandlerCommand),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ConnectionHandlerCommand {
    SetConnectionId(ClientId),
    SendPacket(ClientToServerPacket),
    AddPeer(ClientId),
    AddConnectionInfo { client_id: ClientId, info: PeerInfo },
}

#[derive(Debug)]
pub struct ConnectionHandler {
    stream: OwnedWriteHalf,
    write_buf: Vec<u8>,

    client_id: Option<ClientId>,
    peer_ids: Vec<ClientId>,
    connection_infos: HashMap<ClientId, PeerInfo>,

    inbound: mpsc::UnboundedReceiver<ConnectionHandlerEvent>,
    loopback: mpsc::UnboundedSender<ConnectionHandlerEvent>,

    tasks: HashMap<ResponseId, mpsc::UnboundedSender<ServerToClientResponsePacket>>,
    current_response_id: u32,
}

impl ConnectionHandler {
    pub fn new(stream: TcpStream) -> Self {
        let (tcp_reader, tcp_writer) = stream.into_split();
        let (loopback, inbound) = mpsc::unbounded_channel();
        let tx = loopback.clone();
        tokio::spawn(async move {
            match packet_read_loop(tcp_reader, tx).await {
                Ok(_) => {}
                Err(err) => println!("fucky wucky uwu {:?}", err),
            }
        });

        Self {
            stream: tcp_writer,
            write_buf: vec![],
            peer_ids: vec![],
            client_id: None,
            connection_infos: HashMap::default(),
            inbound,
            loopback,
            tasks: HashMap::new(),
            current_response_id: 0,
        }
    }

    pub fn spawn_task<Fn, Fut>(&mut self, func: Fn)
    where
        Fn: FnOnce(ClientTask) -> Fut,
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static,
    {
        let (packet_tx, packet_rx) = mpsc::unbounded_channel();
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();

        let rid = ResponseId(self.current_response_id);
        self.tasks.insert(rid, packet_tx);
        self.current_response_id += 1;

        let loopback = self.loopback.clone();
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                loopback.send(ConnectionHandlerEvent::Command(cmd)).unwrap();
            }
            loopback
                .send(ConnectionHandlerEvent::DeadTask(rid))
                .unwrap();
        });

        tokio::spawn(func(ClientTask {
            rid,
            inbound: packet_rx,
            outbound: cmd_tx,
        }));
    }

    async fn handle_command(&mut self, cmd: ConnectionHandlerCommand) -> ClientResult<()> {
        match cmd {
            ConnectionHandlerCommand::SetConnectionId(id) => self.client_id = Some(id),
            ConnectionHandlerCommand::SendPacket(packet) => self.write_packet(&packet).await?,
            ConnectionHandlerCommand::AddPeer(peer_id) => self.peer_ids.push(peer_id),
            ConnectionHandlerCommand::AddConnectionInfo { client_id, info } => {
                drop(self.connection_infos.insert(client_id, info))
            }
        }
        Ok(())
    }

    pub async fn recieve_packet(&mut self) -> ClientResult<Option<ServerToClientPacket>> {
        loop {
            match self.inbound.recv().await {
                Some(packet) => match packet {
                    ConnectionHandlerEvent::Packet(ServerToClientPacket::Response {
                        rid,
                        packet,
                    }) => {
                        if let Some(task_channel) = self.tasks.get_mut(&rid) {
                            task_channel.send(packet).unwrap();
                        }
                        return Ok(None);
                    }
                    ConnectionHandlerEvent::Packet(packet) => return Ok(Some(packet)),
                    ConnectionHandlerEvent::DeadTask(id) => {
                        self.tasks.remove(&id);
                    }
                    ConnectionHandlerEvent::Command(cmd) => {
                        self.handle_command(cmd).await?;
                        return Ok(None);
                    }
                },
                None => return Err(ClientError::UnexpectedEndOfStream),
            }
        }
    }

    pub async fn write_packet(&mut self, packet: &ClientToServerPacket) -> CommonResult<()> {
        write_whole_packet(&mut self.stream, &mut self.write_buf, packet).await
    }

    pub fn client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    pub fn connection_info(&self, id: ClientId) -> Option<&PeerInfo> {
        self.connection_infos.get(&id)
    }
}

async fn packet_read_loop(
    mut stream: OwnedReadHalf,
    channel: mpsc::UnboundedSender<ConnectionHandlerEvent>,
) -> ClientResult<()> {
    let mut read_buf = Vec::with_capacity(4096);

    loop {
        match read_whole_packet::<ServerToClientPacket, _>(&mut stream, &mut read_buf).await {
            Ok(None) => break,
            Ok(Some(packet)) => {
                if channel
                    .send(ConnectionHandlerEvent::Packet(packet))
                    .is_err()
                {
                    break;
                }
            }
            Err(err) => {
                return Err(err.into());
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
pub struct ClientTask {
    rid: ResponseId,
    inbound: mpsc::UnboundedReceiver<ServerToClientResponsePacket>,
    outbound: mpsc::UnboundedSender<ConnectionHandlerCommand>,
}

impl ClientTask {
    pub async fn send_packet(&mut self, packet: ClientToServerPacketKind) -> ClientResult<()> {
        self.send(ConnectionHandlerCommand::SendPacket(ClientToServerPacket {
            rid: self.rid,
            kind: packet,
        }))
        .await
    }

    pub async fn send(&mut self, cmd: ConnectionHandlerCommand) -> ClientResult<()> {
        let _ = self.outbound.send(cmd);
        Ok(())
    }

    pub async fn recv(&mut self) -> ClientResult<ServerToClientResponsePacket> {
        Ok(self
            .inbound
            .recv()
            .await
            .ok_or(ClientError::UnexpectedEndOfStream)?)
    }
}
