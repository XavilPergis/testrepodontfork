use super::packet::PacketCodec;

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
    InvalidEnumVariant,
    Utf8Error(std::str::Utf8Error),
}

impl From<std::str::Utf8Error> for PacketDeserializeError {
    fn from(err: std::str::Utf8Error) -> Self {
        PacketDeserializeError::Utf8Error(err)
    }
}

pub type PacketDeserializeResult<T> = Result<T, PacketDeserializeError>;

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

    pub fn deserialize<T: PacketCodec>(&mut self) -> PacketDeserializeResult<T> {
        T::deserialize(self)
    }

    pub fn parse<T: PacketCodec>(&mut self) -> PacketDeserializeResult<(T, usize)> {
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PacketSerializeError {}

pub type PacketSerializeResult<T> = Result<T, PacketSerializeError>;
/// important: packet length is NOT included in the buffer! it must be sent down
/// the connection before the buffer is written.
pub struct PacketSerializerContext<'buf> {
    buffer: &'buf mut Vec<u8>,
}

impl<'buf> PacketSerializerContext<'buf> {
    pub fn new(buffer: &'buf mut Vec<u8>) -> Self {
        Self { buffer }
    }

    pub fn write_byte_sequence<I: IntoIterator<Item = u8>>(
        &mut self,
        iter: I,
    ) -> PacketSerializeResult<()> {
        self.buffer.extend(iter);
        Ok(())
    }

    pub fn serialize<T: PacketCodec>(&mut self, value: &T) -> PacketSerializeResult<()> {
        value.serialize(self)
    }
}
