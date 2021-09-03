use std::convert::TryInto;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PacketDeserializeError {
    // These two variants indicate that more data should be coming in the future, so the parse should be retried later.
    UnknownPacketLength,
    MismatchedPacketLength {
        buffer_length: usize,
        expected_from_header: usize,
    },
    // these variants are normal parse errors, including the ones about running out of bytes
    OutOfBytes,
    MismatchedParsedLength {
        parsed_length: usize,
        expected_from_header: usize,
    },
    InvalidLength(usize),
    UnknownPacketId(u32),
    Utf8Error(std::str::Utf8Error),
}

impl From<std::str::Utf8Error> for PacketDeserializeError {
    fn from(err: std::str::Utf8Error) -> Self {
        PacketDeserializeError::Utf8Error(err)
    }
}

pub type PacketDeserializeResult<T> = Result<T, PacketDeserializeError>;

pub trait DeserializePacket<'buf>: Sized {
    fn deserialize(ctx: &mut PacketDeserializerContext<'buf>) -> PacketDeserializeResult<Self>;
}

impl<'buf> DeserializePacket<'buf> for u8 {
    fn deserialize(ctx: &mut PacketDeserializerContext<'buf>) -> PacketDeserializeResult<Self> {
        Ok(ctx.parse_byte_sequence(1)?[0])
    }
}

impl<'buf> DeserializePacket<'buf> for u16 {
    fn deserialize(ctx: &mut PacketDeserializerContext<'buf>) -> PacketDeserializeResult<Self> {
        let bytes = ctx.parse_byte_sequence(std::mem::size_of::<u16>())?;
        Ok(u16::from_be_bytes(bytes.try_into().unwrap()))
    }
}

impl<'buf> DeserializePacket<'buf> for u32 {
    fn deserialize(ctx: &mut PacketDeserializerContext<'buf>) -> PacketDeserializeResult<Self> {
        let bytes = ctx.parse_byte_sequence(std::mem::size_of::<u32>())?;
        Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
    }
}

impl<'buf> DeserializePacket<'buf> for u64 {
    fn deserialize(ctx: &mut PacketDeserializerContext<'buf>) -> PacketDeserializeResult<Self> {
        let bytes = ctx.parse_byte_sequence(std::mem::size_of::<u64>())?;
        Ok(u64::from_be_bytes(bytes.try_into().unwrap()))
    }
}

impl<'buf> DeserializePacket<'buf> for &'buf str {
    fn deserialize(ctx: &mut PacketDeserializerContext<'buf>) -> PacketDeserializeResult<Self> {
        let length = ctx.deserialize::<u32>()?;
        let bytes = ctx.parse_byte_sequence(length as usize)?;
        Ok(std::str::from_utf8(bytes)?)
    }
}

impl<'buf> DeserializePacket<'buf> for String {
    fn deserialize(ctx: &mut PacketDeserializerContext<'buf>) -> PacketDeserializeResult<Self> {
        Ok(ctx.deserialize::<&'buf str>()?.into())
    }
}

impl<'buf, T: DeserializePacket<'buf>> DeserializePacket<'buf> for Vec<T> {
    fn deserialize(ctx: &mut PacketDeserializerContext<'buf>) -> PacketDeserializeResult<Self> {
        let num_items = ctx.deserialize::<u32>()?;
        let mut values = Vec::new();
        for _ in 0..num_items {
            values.push(ctx.deserialize::<T>()?);
        }
        Ok(values)
    }
}

#[derive(Debug)]
pub struct PacketDeserializerContext<'buf> {
    packet: &'buf [u8],
    cursor: usize,
}

impl<'buf> PacketDeserializerContext<'buf> {
    pub fn new(packet: &'buf [u8]) -> Self {
        Self { packet, cursor: 0 }
    }

    pub fn parse_byte_sequence(
        &mut self,
        bytes: usize,
    ) -> Result<&'buf [u8], PacketDeserializeError> {
        if self.cursor + bytes > self.packet.len() {
            Err(PacketDeserializeError::OutOfBytes)
        } else {
            let data = &self.packet[self.cursor..self.cursor + bytes];
            self.cursor += bytes;
            Ok(data)
        }
    }

    pub fn deserialize<T: DeserializePacket<'buf>>(&mut self) -> PacketDeserializeResult<T> {
        T::deserialize(self)
    }

    pub fn parse<T: DeserializePacket<'buf>>(&mut self) -> PacketDeserializeResult<(T, usize)> {
        // println!("parsing {:?}", self.packet);
        if self.packet.len() < 4 {
            return Err(PacketDeserializeError::UnknownPacketLength);
        } else {
            let length = self.deserialize::<u32>()? as usize;
            if length > self.packet.len() {
                return Err(PacketDeserializeError::MismatchedPacketLength {
                    buffer_length: self.packet.len(),
                    expected_from_header: length,
                });
            } else if length < 4 {
                return Err(PacketDeserializeError::InvalidLength(length));
            }

            self.packet = &self.packet[..length];
        }

        let packet = self.deserialize::<T>()?;

        if self.cursor != self.packet.len() {
            return Err(PacketDeserializeError::MismatchedParsedLength {
                parsed_length: self.cursor,
                expected_from_header: self.packet.len(),
            });
        }

        Ok((packet, self.cursor))
    }
}
