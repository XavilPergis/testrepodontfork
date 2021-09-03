mod deserialize_packet;
pub mod packet;
mod serialize_packet;

pub type CommonResult<T> = Result<T, CommonError>;

#[derive(Debug)]
pub enum CommonError {
    Io(std::io::Error),
    Deserialize(deserialize_packet::PacketDeserializeError),
    Serialize(serialize_packet::PacketSerializeError),
    ConnectionReset,
}

impl From<std::io::Error> for CommonError {
    fn from(err: std::io::Error) -> Self {
        CommonError::Io(err)
    }
}

impl From<deserialize_packet::PacketDeserializeError> for CommonError {
    fn from(err: deserialize_packet::PacketDeserializeError) -> Self {
        CommonError::Deserialize(err)
    }
}

impl From<serialize_packet::PacketSerializeError> for CommonError {
    fn from(err: serialize_packet::PacketSerializeError) -> Self {
        CommonError::Serialize(err)
    }
}
