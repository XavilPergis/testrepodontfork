use tokio::{
    net::{TcpListener, ToSocketAddrs},
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};

use super::ServerResult;

pub enum InstanceMessage {}

#[derive(Clone, Debug)]
pub struct InstanceRef {
    pub channel: UnboundedSender<InstanceMessage>,
}

pub struct Instance {
    listener: TcpListener,
    channel: UnboundedReceiver<InstanceMessage>,
}

impl Instance {
    pub fn new(
        listener: TcpListener,
        channel: UnboundedReceiver<InstanceMessage>,
    ) -> ServerResult<Self> {
        Ok(Self { listener, channel })
    }

    pub async fn run(mut self) -> ServerResult<()> {
        loop {
            tokio::select! {
                message = self.channel.recv() => {
                    match message {
                        Some(message) => if self.handle_message(message) {
                            break
                        },
                        None => break
                    }
                }
                connection => self.listener.accept() => {}
            }
        }
        Ok(())
    }
}

impl Instance {
    fn handle_message(&mut self, message: InstanceMessage) -> ServerResult<bool> {
        Ok(true)
    }

    fn foo(&self) {}
}

pub async fn create_instance<A: ToSocketAddrs>(addr: A) -> ServerResult<InstanceRef> {
    let listener = TcpListener::bind(addr).await?;

    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(Instance::new(listener, rx)?.run());

    Ok(InstanceRef { channel: tx })
}
