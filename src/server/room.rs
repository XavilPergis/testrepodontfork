use tokio::sync::mpsc::UnboundedSender;

pub enum RoomMessage {}

#[derive(Clone, Debug)]
pub struct RoomRef {
    pub channel: UnboundedSender<RoomMessage>,
}

pub struct Room {
    pub instance: super::instance::InstanceRef,
}
