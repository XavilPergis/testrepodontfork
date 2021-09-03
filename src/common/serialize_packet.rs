use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::CommonResult;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PacketSerializeError {}

pub type PacketSerializeResult<T> = Result<T, PacketSerializeError>;

pub trait SerializePacket: Sized {
    fn serialize<'buf>(&self, ctx: &mut PacketSerializerContext<'buf>)
        -> PacketSerializeResult<()>;
}

/// important: packet length is NOT included in the buffer! it must be sent down
/// the connection before the buffer is written.
pub struct PacketSerializerContext<'buf> {
    buffer: &'buf mut Vec<u8>,
}

impl<'buf> PacketSerializerContext<'buf> {
    pub fn new(buffer: &'buf mut Vec<u8>) -> Self {
        Self { buffer }
    }

    pub fn serialize<T: SerializePacket>(&mut self, value: &T) -> PacketSerializeResult<()> {
        value.serialize(self)
    }
}

impl SerializePacket for u8 {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.buffer.push(*self);
        Ok(())
    }
}

impl SerializePacket for u16 {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.buffer.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl SerializePacket for u32 {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.buffer.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl SerializePacket for u64 {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.buffer.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl SerializePacket for String {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize(&(self.len() as u32))?;
        ctx.buffer.extend_from_slice(self.as_bytes());
        Ok(())
    }
}

impl<T: SerializePacket> SerializePacket for Vec<T> {
    fn serialize(&self, ctx: &mut PacketSerializerContext) -> PacketSerializeResult<()> {
        ctx.serialize(&(self.len() as u32))?;
        for item in self.iter() {
            ctx.serialize(item)?;
        }
        Ok(())
    }
}

pub async fn write_whole_packet<P: SerializePacket, W: AsyncWrite + Unpin>(
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
