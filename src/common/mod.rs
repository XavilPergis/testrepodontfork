use std::pin::Pin;

use futures::Future;

mod context;
mod impls;
pub mod packet;

pub type CommonResult<T> = Result<T, CommonError>;

pub type DynamicFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>;

#[derive(Debug)]
pub enum CommonError {
    Io(std::io::Error),
    Deserialize(packet::PacketDeserializeError),
    Serialize(packet::PacketSerializeError),
    ConnectionReset,
}

impl From<std::io::Error> for CommonError {
    fn from(err: std::io::Error) -> Self {
        CommonError::Io(err)
    }
}

impl From<packet::PacketDeserializeError> for CommonError {
    fn from(err: packet::PacketDeserializeError) -> Self {
        CommonError::Deserialize(err)
    }
}

impl From<packet::PacketSerializeError> for CommonError {
    fn from(err: packet::PacketSerializeError) -> Self {
        CommonError::Serialize(err)
    }
}
