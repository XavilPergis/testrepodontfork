#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PacketSerializeError {}

pub type PacketSerializeResult<T> = Result<T, PacketSerializeError>;

pub trait SerializePacket<'buf>: Sized {
    fn serialize(&self, ctx: &mut PacketSerializerContext<'buf>) -> PacketSerializeResult<()>;
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

    pub fn serialize<T: SerializePacket<'buf>>(&mut self, value: &T) -> PacketSerializeResult<()> {
        value.serialize(self)
    }
}

impl<'buf> SerializePacket<'buf> for u8 {
    fn serialize(&self, ctx: &mut PacketSerializerContext<'buf>) -> PacketSerializeResult<()> {
        ctx.buffer.push(*self);
        Ok(())
    }
}

impl<'buf> SerializePacket<'buf> for u16 {
    fn serialize(&self, ctx: &mut PacketSerializerContext<'buf>) -> PacketSerializeResult<()> {
        ctx.buffer.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl<'buf> SerializePacket<'buf> for u32 {
    fn serialize(&self, ctx: &mut PacketSerializerContext<'buf>) -> PacketSerializeResult<()> {
        ctx.buffer.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl<'buf> SerializePacket<'buf> for u64 {
    fn serialize(&self, ctx: &mut PacketSerializerContext<'buf>) -> PacketSerializeResult<()> {
        ctx.buffer.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl<'buf, 's> SerializePacket<'buf> for &'s str {
    fn serialize(&self, ctx: &mut PacketSerializerContext<'buf>) -> PacketSerializeResult<()> {
        ctx.serialize(&(self.len() as u32))?;
        ctx.buffer.extend_from_slice(self.as_bytes());
        Ok(())
    }
}

impl<'buf> SerializePacket<'buf> for String {
    fn serialize(&self, ctx: &mut PacketSerializerContext<'buf>) -> PacketSerializeResult<()> {
        ctx.serialize(&self.as_str())?;
        Ok(())
    }
}

impl<'buf, T: SerializePacket<'buf>> SerializePacket<'buf> for Vec<T> {
    fn serialize(&self, ctx: &mut PacketSerializerContext<'buf>) -> PacketSerializeResult<()> {
        ctx.serialize(&(self.len() as u32))?;
        for item in self.iter() {
            ctx.serialize(item)?;
        }
        Ok(())
    }
}
