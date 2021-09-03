pub use super::deserialize_packet::{
    read_whole_packet, DeserializePacket, PacketDeserializeError, PacketDeserializeResult,
    PacketDeserializerContext,
};
pub use super::serialize_packet::{
    write_whole_packet, PacketSerializeError, PacketSerializeResult, PacketSerializerContext,
    SerializePacket,
};

// server -> client
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ServerToClientPacket {
    ConnectAck { connection_id: u64 },
    DisconnectAck,
    // TODO
    MessageAck,
    ShutdownAck,

    PeerConnected { peer_id: u64 },
    PeerDisonnected { peer_id: u64 },
    PeerMessage { peer_id: u64, message: String },

    PeerListingResponse { peers: Vec<u64> },
    PeerInfoResponse {},
}

impl ServerToClientPacket {
    pub fn packet_id(&self) -> u32 {
        match self {
            ServerToClientPacket::ConnectAck { .. } => S2C_ID_CONNECT_ACK,
            ServerToClientPacket::DisconnectAck => S2C_ID_DISCONNECT_ACK,
            ServerToClientPacket::MessageAck => S2C_ID_MESSAGE_ACK,
            ServerToClientPacket::ShutdownAck => S2C_ID_SHUTDOWN_ACK,
            ServerToClientPacket::PeerConnected { .. } => S2C_ID_PEER_CONNECTED,
            ServerToClientPacket::PeerDisonnected { .. } => S2C_ID_PEER_DISCONNECTED,
            ServerToClientPacket::PeerMessage { .. } => S2C_ID_PEER_MESSAGE,
            ServerToClientPacket::PeerListingResponse { .. } => S2C_ID_PEER_LISTING_RESPONSE,
            ServerToClientPacket::PeerInfoResponse { .. } => S2C_ID_PEER_INFO_RESPONSE,
        }
    }
}

pub const S2C_ID_CONNECT_ACK: u32 = 0;
pub const S2C_ID_DISCONNECT_ACK: u32 = 1;
pub const S2C_ID_MESSAGE_ACK: u32 = 2;
pub const S2C_ID_SHUTDOWN_ACK: u32 = 3;
pub const S2C_ID_PEER_CONNECTED: u32 = 4;
pub const S2C_ID_PEER_DISCONNECTED: u32 = 5;
pub const S2C_ID_PEER_MESSAGE: u32 = 6;
pub const S2C_ID_PEER_LISTING_RESPONSE: u32 = 7;
pub const S2C_ID_PEER_INFO_RESPONSE: u32 = 8;

impl SerializePacket for ServerToClientPacket {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize::<u32>(&self.packet_id())?;
        match self {
            ServerToClientPacket::ConnectAck { connection_id } => {
                ctx.serialize::<u64>(connection_id)?
            }

            ServerToClientPacket::DisconnectAck
            | ServerToClientPacket::MessageAck
            | ServerToClientPacket::ShutdownAck => {}

            ServerToClientPacket::PeerConnected { peer_id } => ctx.serialize::<u64>(peer_id)?,
            ServerToClientPacket::PeerDisonnected { peer_id } => ctx.serialize::<u64>(peer_id)?,
            ServerToClientPacket::PeerMessage { peer_id, message } => {
                ctx.serialize::<u64>(peer_id)?;
                ctx.serialize::<String>(message)?;
            }

            ServerToClientPacket::PeerListingResponse { peers } => {
                ctx.serialize::<Vec<u64>>(peers)?
            }
            ServerToClientPacket::PeerInfoResponse {} => todo!(),
        }
        Ok(())
    }
}

impl DeserializePacket for ServerToClientPacket {
    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<ServerToClientPacket> {
        Ok(match ctx.deserialize::<u32>()? {
            S2C_ID_CONNECT_ACK => ServerToClientPacket::ConnectAck {
                connection_id: ctx.deserialize::<u64>()?,
            },
            S2C_ID_DISCONNECT_ACK => ServerToClientPacket::DisconnectAck,
            S2C_ID_MESSAGE_ACK => ServerToClientPacket::MessageAck,
            S2C_ID_SHUTDOWN_ACK => ServerToClientPacket::ShutdownAck,
            S2C_ID_PEER_CONNECTED => ServerToClientPacket::PeerConnected {
                peer_id: ctx.deserialize::<u64>()?,
            },
            S2C_ID_PEER_DISCONNECTED => ServerToClientPacket::PeerDisonnected {
                peer_id: ctx.deserialize::<u64>()?,
            },
            S2C_ID_PEER_MESSAGE => ServerToClientPacket::PeerMessage {
                peer_id: ctx.deserialize::<u64>()?,
                message: ctx.deserialize::<String>()?,
            },
            S2C_ID_PEER_LISTING_RESPONSE => ServerToClientPacket::PeerListingResponse {
                peers: ctx.deserialize::<Vec<u64>>()?,
            },
            S2C_ID_PEER_INFO_RESPONSE => ServerToClientPacket::PeerInfoResponse {},
            other => return Err(PacketDeserializeError::UnknownPacketId(other)),
        })
    }
}

// client -> server
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ClientToServerPacket {
    Connect,
    Disconnect,
    Message { message: String },
    Shutdown,
    RequestPeerListing,
    RequestPeerInfo { peer_ids: Vec<u64> },
}

pub const C2S_ID_CONNECT: u32 = 0;
pub const C2S_ID_DISCONNECT: u32 = 1;
pub const C2S_ID_MESSAGE: u32 = 2;
pub const C2S_ID_SHUTDOWN: u32 = 3;
pub const C2S_ID_REQUEST_PEER_LISTING: u32 = 4;
pub const C2S_ID_REQUEST_PEER_INFO: u32 = 5;

impl SerializePacket for ClientToServerPacket {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        match self {
            ClientToServerPacket::Connect => ctx.serialize::<u32>(&C2S_ID_CONNECT)?,
            ClientToServerPacket::Disconnect => ctx.serialize::<u32>(&C2S_ID_DISCONNECT)?,
            ClientToServerPacket::Message { message } => {
                ctx.serialize::<u32>(&C2S_ID_MESSAGE)?;
                ctx.serialize::<String>(message)?;
            }
            ClientToServerPacket::Shutdown => ctx.serialize::<u32>(&C2S_ID_SHUTDOWN)?,
            ClientToServerPacket::RequestPeerListing => {
                ctx.serialize::<u32>(&C2S_ID_REQUEST_PEER_LISTING)?
            }
            ClientToServerPacket::RequestPeerInfo { peer_ids } => {
                ctx.serialize::<u32>(&C2S_ID_REQUEST_PEER_INFO)?;
                ctx.serialize::<Vec<u64>>(peer_ids)?;
            }
        }
        Ok(())
    }
}

impl DeserializePacket for ClientToServerPacket {
    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<ClientToServerPacket> {
        Ok(match ctx.deserialize::<u32>()? {
            C2S_ID_CONNECT => ClientToServerPacket::Connect,
            C2S_ID_DISCONNECT => ClientToServerPacket::Disconnect,
            C2S_ID_MESSAGE => ClientToServerPacket::Message {
                message: ctx.deserialize::<String>()?.into(),
            },
            C2S_ID_SHUTDOWN => ClientToServerPacket::Shutdown,
            C2S_ID_REQUEST_PEER_LISTING => ClientToServerPacket::RequestPeerListing,
            C2S_ID_REQUEST_PEER_INFO => ClientToServerPacket::RequestPeerInfo {
                peer_ids: ctx.deserialize::<Vec<u64>>()?,
            },
            other => return Err(PacketDeserializeError::UnknownPacketId(other)),
        })
    }
}
