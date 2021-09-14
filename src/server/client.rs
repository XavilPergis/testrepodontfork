use tokio::sync::mpsc::UnboundedSender;

pub enum ClientMessage {}

#[derive(Clone, Debug)]
pub struct ClientRef {
    pub channel: UnboundedSender<ClientMessage>,
}

pub struct Client {
    pub instance: super::instance::InstanceRef,
}
