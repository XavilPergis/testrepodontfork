use std::collections::HashMap;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub use super::context::{
    PacketDeserializeError, PacketDeserializeResult, PacketDeserializerContext,
    PacketSerializeError, PacketSerializeResult, PacketSerializerContext,
};
use super::{CommonError, CommonResult};

pub trait PacketCodec: Sized {
    fn serialize<'buf>(&self, ctx: &mut PacketSerializerContext<'buf>)
        -> PacketSerializeResult<()>;

    // TODO: return untrusted wrapper so you have to explicitly verify
    // that user data has been validated
    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<Self>;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ClientId(pub u64);

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@client-{}", self.0)?;
        Ok(())
    }
}

impl PacketCodec for ClientId {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize(&self.0)
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<Self> {
        ctx.deserialize().map(ClientId)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct RoomId(pub u64);

impl std::fmt::Display for RoomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#room-{}", self.0)?;
        Ok(())
    }
}

impl PacketCodec for RoomId {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize(&self.0)
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<Self> {
        ctx.deserialize().map(RoomId)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ResponseId(pub u32);

impl PacketCodec for ResponseId {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize(&self.0)
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<Self> {
        ctx.deserialize().map(ResponseId)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PeerInfo {
    pub username: Option<String>,
}

impl PacketCodec for PeerInfo {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize(&self.username)?;
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<PeerInfo> {
        Ok(PeerInfo {
            username: ctx.deserialize()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct RoomInfo {
    pub name: String,
}

impl PacketCodec for RoomInfo {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize(&self.name)?;
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<RoomInfo> {
        Ok(RoomInfo {
            name: ctx.deserialize()?,
        })
    }
}

// server -> client
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerToClientResponsePacket {
    ConnectAck { client_id: ClientId },
    MessageAck {},
    ShutdownAck {},
    PeerListingResponse { peers: Vec<ClientId> },
    PeerInfoResponse { peers: HashMap<ClientId, PeerInfo> },
    RoomListingResponse { rooms: Vec<RoomId> },
    RoomInfoResponse { rooms: HashMap<RoomId, RoomInfo> },
}

impl PacketCodec for ServerToClientResponsePacket {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        match self {
            ServerToClientResponsePacket::ConnectAck { client_id } => {
                ctx.serialize(&S2C_RESPONSE_ID_CONNECT_ACK)?;
                ctx.serialize(client_id)?;
            }
            ServerToClientResponsePacket::MessageAck {} => {
                ctx.serialize(&S2C_RESPONSE_ID_MESSAGE_ACK)?;
            }
            ServerToClientResponsePacket::ShutdownAck {} => {
                ctx.serialize(&S2C_RESPONSE_ID_SHUTDOWN_ACK)?;
            }
            ServerToClientResponsePacket::PeerListingResponse { peers } => {
                ctx.serialize(&S2C_RESPONSE_ID_PEER_LISTING_RESPONSE)?;
                ctx.serialize(peers)?;
            }
            ServerToClientResponsePacket::PeerInfoResponse { peers } => {
                ctx.serialize(&S2C_RESPONSE_ID_PEER_INFO_RESPONSE)?;
                ctx.serialize(peers)?;
            }
            ServerToClientResponsePacket::RoomListingResponse { rooms } => {
                ctx.serialize(&S2C_RESPONSE_ID_ROOM_LISTING_RESPONSE)?;
                ctx.serialize(rooms)?;
            }
            ServerToClientResponsePacket::RoomInfoResponse { rooms } => {
                ctx.serialize(&S2C_RESPONSE_ID_ROOM_INFO_RESPONSE)?;
                ctx.serialize(rooms)?;
            }
        }
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<Self> {
        Ok(match ctx.deserialize::<u32>()? {
            S2C_RESPONSE_ID_CONNECT_ACK => ServerToClientResponsePacket::ConnectAck {
                client_id: ctx.deserialize()?,
            },
            S2C_RESPONSE_ID_MESSAGE_ACK => ServerToClientResponsePacket::MessageAck {},
            S2C_RESPONSE_ID_SHUTDOWN_ACK => ServerToClientResponsePacket::ShutdownAck {},
            S2C_RESPONSE_ID_PEER_LISTING_RESPONSE => {
                ServerToClientResponsePacket::PeerListingResponse {
                    peers: ctx.deserialize()?,
                }
            }
            S2C_RESPONSE_ID_PEER_INFO_RESPONSE => ServerToClientResponsePacket::PeerInfoResponse {
                peers: ctx.deserialize()?,
            },
            S2C_RESPONSE_ID_ROOM_LISTING_RESPONSE => {
                ServerToClientResponsePacket::RoomListingResponse {
                    rooms: ctx.deserialize()?,
                }
            }
            S2C_RESPONSE_ID_ROOM_INFO_RESPONSE => ServerToClientResponsePacket::RoomInfoResponse {
                rooms: ctx.deserialize()?,
            },
            other => return Err(PacketDeserializeError::UnknownPacketId(other)),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerToClientPacket {
    Response {
        rid: ResponseId,
        packet: ServerToClientResponsePacket,
    },
    // RoomAdded {
    //     room_id: RoomId,
    // },
    // RoomRemoved {
    //     room_id: RoomId,
    // },
    PeerConnected {
        room_id: RoomId,
        peer_id: ClientId,
    },
    PeerDisconnected {
        room_id: RoomId,
        peer_id: ClientId,
    },
    PeerMessage {
        room_id: RoomId,
        peer_id: ClientId,
        message: String,
    },
}

pub const S2C_RESPONSE_ID_CONNECT_ACK: u32 = 0;
pub const S2C_RESPONSE_ID_MESSAGE_ACK: u32 = 2;
pub const S2C_RESPONSE_ID_SHUTDOWN_ACK: u32 = 3;
pub const S2C_RESPONSE_ID_PEER_LISTING_RESPONSE: u32 = 4;
pub const S2C_RESPONSE_ID_PEER_INFO_RESPONSE: u32 = 5;
pub const S2C_RESPONSE_ID_ROOM_LISTING_RESPONSE: u32 = 6;
pub const S2C_RESPONSE_ID_ROOM_INFO_RESPONSE: u32 = 7;

pub const S2C_ID_RESPONSE: u32 = 0;
pub const S2C_ID_PEER_CONNECTED: u32 = 1;
pub const S2C_ID_PEER_DISCONNECTED: u32 = 2;
pub const S2C_ID_PEER_MESSAGE: u32 = 3;

impl PacketCodec for ServerToClientPacket {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        match self {
            ServerToClientPacket::Response { rid, packet } => {
                ctx.serialize(&S2C_ID_RESPONSE)?;
                ctx.serialize(rid)?;
                ctx.serialize(packet)?;
            }
            ServerToClientPacket::PeerConnected { room_id, peer_id } => {
                ctx.serialize(&S2C_ID_PEER_CONNECTED)?;
                ctx.serialize(room_id)?;
                ctx.serialize(peer_id)?;
            }
            ServerToClientPacket::PeerDisconnected { room_id, peer_id } => {
                ctx.serialize(&S2C_ID_PEER_DISCONNECTED)?;
                ctx.serialize(room_id)?;
                ctx.serialize(peer_id)?;
            }
            ServerToClientPacket::PeerMessage {
                room_id,
                peer_id,
                message,
            } => {
                ctx.serialize(&S2C_ID_PEER_MESSAGE)?;
                ctx.serialize(room_id)?;
                ctx.serialize(peer_id)?;
                ctx.serialize(message)?;
            }
        }
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<ServerToClientPacket> {
        Ok(match ctx.deserialize::<u32>()? {
            S2C_ID_RESPONSE => ServerToClientPacket::Response {
                rid: ctx.deserialize()?,
                packet: ctx.deserialize()?,
            },
            S2C_ID_PEER_CONNECTED => ServerToClientPacket::PeerConnected {
                room_id: ctx.deserialize()?,
                peer_id: ctx.deserialize()?,
            },
            S2C_ID_PEER_DISCONNECTED => ServerToClientPacket::PeerDisconnected {
                room_id: ctx.deserialize()?,
                peer_id: ctx.deserialize()?,
            },
            S2C_ID_PEER_MESSAGE => ServerToClientPacket::PeerMessage {
                room_id: ctx.deserialize()?,
                peer_id: ctx.deserialize()?,
                message: ctx.deserialize()?,
            },
            other => return Err(PacketDeserializeError::UnknownPacketId(other)),
        })
    }
}

// client -> server
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ClientToServerPacketKind {
    Connect { username: String },
    Message { room: RoomId, message: String },
    Shutdown {},
    RequestRoomListing {},
    RequestRoomInfo { room_ids: Vec<RoomId> },
    RequestPeerListing { room: RoomId },
    RequestPeerInfo { peer_ids: Vec<ClientId> },
}

pub const C2S_ID_CONNECT: u32 = 0;
pub const C2S_ID_MESSAGE: u32 = 2;
pub const C2S_ID_SHUTDOWN: u32 = 3;
pub const C2S_ID_REQUEST_PEER_LISTING: u32 = 4;
pub const C2S_ID_REQUEST_PEER_INFO: u32 = 5;
pub const C2S_ID_REQUEST_ROOM_LISTING: u32 = 6;
pub const C2S_ID_REQUEST_ROOM_INFO: u32 = 7;

impl PacketCodec for ClientToServerPacketKind {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        match self {
            ClientToServerPacketKind::Connect { username } => {
                ctx.serialize::<u32>(&C2S_ID_CONNECT)?;
                ctx.serialize(username)?;
            }
            ClientToServerPacketKind::Message { room, message } => {
                ctx.serialize::<u32>(&C2S_ID_MESSAGE)?;
                ctx.serialize(room)?;
                ctx.serialize(message)?;
            }
            ClientToServerPacketKind::Shutdown {} => {
                ctx.serialize::<u32>(&C2S_ID_SHUTDOWN)?;
            }
            ClientToServerPacketKind::RequestPeerListing { room } => {
                ctx.serialize::<u32>(&C2S_ID_REQUEST_PEER_LISTING)?;
                ctx.serialize(room)?;
            }
            ClientToServerPacketKind::RequestPeerInfo { peer_ids } => {
                ctx.serialize::<u32>(&C2S_ID_REQUEST_PEER_INFO)?;
                ctx.serialize(peer_ids)?;
            }
            ClientToServerPacketKind::RequestRoomListing {} => {
                ctx.serialize::<u32>(&C2S_ID_REQUEST_ROOM_LISTING)?;
            }
            ClientToServerPacketKind::RequestRoomInfo { room_ids } => {
                ctx.serialize::<u32>(&C2S_ID_REQUEST_ROOM_INFO)?;
                ctx.serialize(room_ids)?;
            }
        }
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<ClientToServerPacketKind> {
        Ok(match ctx.deserialize::<u32>()? {
            C2S_ID_CONNECT => ClientToServerPacketKind::Connect {
                username: ctx.deserialize()?,
            },
            C2S_ID_MESSAGE => ClientToServerPacketKind::Message {
                room: ctx.deserialize()?,
                message: ctx.deserialize()?,
            },
            C2S_ID_SHUTDOWN => ClientToServerPacketKind::Shutdown {},
            C2S_ID_REQUEST_PEER_LISTING => ClientToServerPacketKind::RequestPeerListing {
                room: ctx.deserialize()?,
            },
            C2S_ID_REQUEST_PEER_INFO => ClientToServerPacketKind::RequestPeerInfo {
                peer_ids: ctx.deserialize()?,
            },
            C2S_ID_REQUEST_ROOM_LISTING => ClientToServerPacketKind::RequestRoomListing {},
            C2S_ID_REQUEST_ROOM_INFO => ClientToServerPacketKind::RequestRoomInfo {
                room_ids: ctx.deserialize()?,
            },
            other => return Err(PacketDeserializeError::UnknownPacketId(other)),
        })
    }
}

// client -> server
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ClientToServerPacket {
    pub rid: ResponseId,
    pub kind: ClientToServerPacketKind,
}

impl PacketCodec for ClientToServerPacket {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize(&self.rid)?;
        ctx.serialize(&self.kind)?;
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<Self> {
        Ok(ClientToServerPacket {
            rid: ctx.deserialize()?,
            kind: ctx.deserialize()?,
        })
    }
}

pub async fn write_whole_packet<P: PacketCodec, W: AsyncWrite + Unpin>(
    stream: &mut W,
    buf: &mut Vec<u8>,
    message: &P,
) -> CommonResult<()> {
    PacketSerializerContext::new(buf).serialize(message)?;

    stream.write_u32(buf.len() as u32 + 4).await?;
    stream.write_all(buf).await?;
    buf.clear();

    Ok(())
}

pub async fn read_whole_packet<'buf, P: PacketCodec, S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &'buf mut Vec<u8>,
) -> CommonResult<Option<P>> {
    let (message, parsed_length) = loop {
        // attempt to parse a packet first, so that if we get multiple
        // packets at a time, we can actually parse both of them instead of
        // having to wait for more data from the socket first
        match PacketDeserializerContext::new(buf).parse() {
            Ok(message) => break message,
            Err(PacketDeserializeError::UnknownPacketLength) => {}
            Err(PacketDeserializeError::MismatchedPacketLength {
                buffer_length,
                expected_from_header,
            }) if buffer_length < expected_from_header => {}
            Err(err) => return Err(err.into()),
        }

        match stream.read_buf(buf).await? {
            0 if buf.is_empty() => return Ok(None),
            // getting to this read means we have an incomplete packet, but
            // the connection was closed, so the packet can never be
            // finished. this is an error condition.
            0 => return Err(CommonError::ConnectionReset),
            _ => {}
        }
    };

    buf.drain(..parsed_length);
    Ok(Some(message))
}
