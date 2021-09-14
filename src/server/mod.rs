use tokio::{net::TcpListener, sync::oneshot};

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

pub async fn get_responder<T, F, R>(func: F) -> T
where
    F: FnOnce(Responder<T>) -> R,
{
    let (tx, rx) = oneshot::channel();
    func(Responder { sender: Some(tx) });
    rx.await.unwrap()
}

#[derive(Debug)]
pub struct Responder<T> {
    sender: Option<oneshot::Sender<T>>,
}

impl<T> Responder<T> {
    pub fn send(self, value: T) {
        if let Some(sender) = self.sender {
            let _ = sender.send(value);
        }
    }
}

impl Responder<()> {
    pub fn signal(self) {
        if let Some(sender) = self.sender {
            let _ = sender.send(());
        }
    }
}

pub mod client;
pub mod instance;
pub mod room;
