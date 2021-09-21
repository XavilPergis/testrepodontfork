use async_trait::async_trait;
use std::pin::Pin;

use futures::Future;
use tokio::{
    net::TcpListener,
    sync::{mpsc::UnboundedSender, oneshot},
};

use crate::common::{CommonError, DynamicFuture};

#[derive(Debug)]
pub enum ServerError {
    Common(CommonError),
    ResponderClosed,
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

#[track_caller]
pub async fn get_responder<T, F, R>(func: F) -> ServerResult<T>
where
    F: FnOnce(Responder<T>) -> R,
{
    let (tx, rx) = oneshot::channel();
    func(Responder { sender: Some(tx) });
    rx.await.map_err(|_| ServerError::ResponderClosed)
}

#[derive(Debug)]
pub struct Responder<T> {
    sender: Option<oneshot::Sender<T>>,
}

impl<T> Responder<T> {
    #[track_caller]
    pub fn send(self, value: T) {
        if let Some(sender) = self.sender {
            let _ = sender.send(value);
        }
    }

    pub fn empty() -> Self {
        Self { sender: None }
    }
}

impl Responder<()> {
    #[track_caller]
    pub fn signal(self) {
        if let Some(sender) = self.sender {
            let _ = sender.send(());
        }
    }
}

/*

state

handler
   ^
   |
message

*/

// struct Actor<Ctx> {
//     ctx: Ctx,
// }

#[async_trait]
trait ErasedDispatcher<Ctx>: Send + 'static {
    async fn handle(self: Box<Self>, actor: Pin<&mut Ctx>);
}

#[derive(Debug)]
struct Dispatcher<M, H: MessageHandler<M>> {
    response: Responder<H::Response>,
    message: M,
}

#[async_trait]
impl<M, H> ErasedDispatcher<H> for Dispatcher<M, H>
where
    H: MessageHandler<M>,
    M: Send + 'static,
{
    async fn handle(self: Box<Self>, handler: Pin<&mut H>) {
        self.response.send(handler.handle(self.message).await);
    }
}

#[derive(Debug)]
pub struct ActorReference<H> {
    sender: UnboundedSender<Box<dyn ErasedDispatcher<H>>>,
}

impl<H> Clone for ActorReference<H> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<H> ActorReference<H> {
    pub fn new(sender: UnboundedSender<Box<dyn ErasedDispatcher<H>>>) -> Self {
        Self { sender }
    }

    async fn execute<M>(&self, message: M) -> ServerResult<H::Response>
    where
        H: MessageHandler<M>,
        M: Send + 'static,
    {
        get_responder(|response| self.sender.send(Box::new(Dispatcher { response, message }))).await
    }
}

#[async_trait]
trait MessageHandler<M>: Send + 'static {
    type Response: Send + 'static;
    async fn handle(self: Pin<&mut Self>, message: M) -> Self::Response;
}

pub mod client;
pub mod instance;
pub mod room;
