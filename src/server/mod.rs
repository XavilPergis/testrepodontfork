use tokio::net::TcpListener;

use crate::common::CommonError;

#[derive(Debug)]
pub enum ServerError {
    Common(CommonError),
}

impl<T: Into<CommonError>> From<T> for ServerError {
    fn from(err: T) -> Self {
        ServerError::Common(err.into())
    }
}

pub type ServerResult<T> = Result<T, ServerError>;

pub async fn run_server() -> ServerResult<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    let instance = instance::create_instance();

    loop {
        let (stream, addr) = listener.accept().await?;
        instance.send(instance::InstanceMessage::EstablishConnection { stream, addr });
    }

    Ok(())
}

pub mod client;
pub mod instance;
pub mod room;
