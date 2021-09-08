use crate::common::{packet::*, CommonError, CommonResult};
use futures::{Future, FutureExt};
use std::{collections::HashMap, io::Write};
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::{mpsc, oneshot},
};

use self::backend::{EventsBackend, TerminalBackend};

pub mod backend;
pub mod ui;

#[derive(Debug)]
pub enum ClientError {
    Common(CommonError),
    UnexpectedEndOfStream,
    NoConnectAck,
    NoDisconnectAck,
    NoPeerListingResponse,
    NoPeerInfoResponse,
    NoMessageAck,
}

impl<T: Into<CommonError>> From<T> for ClientError {
    fn from(err: T) -> Self {
        ClientError::Common(err.into())
    }
}

pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Clone, Debug, Eq, PartialEq)]
enum ClientContextEvent {
    Packet(ServerToClientPacket),
    DeadTask(ResponseId),
    Command(ClientContextCommand),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum ClientContextCommand {
    SetConnectionId(u64),
    SendPacket(ClientToServerPacket),
    AddPeer(u64),
    AddConnectionInfo { connection_id: u64, info: PeerInfo },
}

#[derive(Debug)]
struct ClientContext {
    stream: OwnedWriteHalf,
    write_buf: Vec<u8>,

    connection_id: Option<u64>,
    peer_ids: Vec<u64>,
    connection_infos: HashMap<u64, PeerInfo>,

    inbound: mpsc::UnboundedReceiver<ClientContextEvent>,
    loopback: mpsc::UnboundedSender<ClientContextEvent>,

    tasks: HashMap<ResponseId, mpsc::UnboundedSender<ServerToClientResponsePacket>>,
    current_response_id: u32,
}

impl ClientContext {
    fn new(stream: TcpStream) -> Self {
        let (tcp_reader, tcp_writer) = stream.into_split();
        let (loopback, inbound) = mpsc::unbounded_channel();
        let tx = loopback.clone();
        tokio::spawn(async move {
            match packet_read_loop(tcp_reader, tx).await {
                Ok(_) => {}
                Err(err) => println!("fucky wucky uwu {:?}", err),
            }
        });

        Self {
            stream: tcp_writer,
            write_buf: vec![],
            peer_ids: vec![],
            connection_id: None,
            connection_infos: HashMap::default(),
            inbound,
            loopback,
            tasks: HashMap::new(),
            current_response_id: 0,
        }
    }

    fn spawn_task<Fn, Fut>(&mut self, func: Fn)
    where
        Fn: FnOnce(ClientTask) -> Fut,
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static,
    {
        let (packet_tx, packet_rx) = mpsc::unbounded_channel();
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();

        let rid = ResponseId(self.current_response_id);
        self.tasks.insert(rid, packet_tx);
        self.current_response_id += 1;

        let loopback = self.loopback.clone();
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                loopback.send(ClientContextEvent::Command(cmd)).unwrap();
            }
            loopback.send(ClientContextEvent::DeadTask(rid)).unwrap();
        });

        tokio::spawn(func(ClientTask {
            rid,
            inbound: packet_rx,
            outbound: cmd_tx,
        }));
    }

    async fn handle_command(&mut self, cmd: ClientContextCommand) -> ClientResult<()> {
        match cmd {
            ClientContextCommand::SetConnectionId(id) => self.connection_id = Some(id),
            ClientContextCommand::SendPacket(packet) => self.write_packet(&packet).await?,
            ClientContextCommand::AddPeer(peer_id) => self.peer_ids.push(peer_id),
            ClientContextCommand::AddConnectionInfo {
                connection_id,
                info,
            } => drop(self.connection_infos.insert(connection_id, info)),
        }
        Ok(())
    }

    async fn recieve_packet(&mut self) -> ClientResult<Option<ServerToClientPacket>> {
        loop {
            match self.inbound.recv().await {
                Some(packet) => match packet {
                    ClientContextEvent::Packet(ServerToClientPacket::Response { rid, packet }) => {
                        if let Some(task_channel) = self.tasks.get_mut(&rid) {
                            task_channel.send(packet).unwrap();
                        }
                        return Ok(None);
                    }
                    ClientContextEvent::Packet(packet) => return Ok(Some(packet)),
                    ClientContextEvent::DeadTask(id) => drop(self.tasks.remove(&id)),
                    ClientContextEvent::Command(cmd) => {
                        self.handle_command(cmd).await?;
                        return Ok(None);
                    }
                },
                None => return Err(ClientError::UnexpectedEndOfStream),
            }
        }
    }

    async fn write_packet(&mut self, packet: &ClientToServerPacket) -> CommonResult<()> {
        write_whole_packet(&mut self.stream, &mut self.write_buf, packet).await
    }
}

async fn packet_read_loop(
    mut stream: OwnedReadHalf,
    channel: mpsc::UnboundedSender<ClientContextEvent>,
) -> ClientResult<()> {
    let mut read_buf = Vec::with_capacity(4096);

    loop {
        match read_whole_packet::<ServerToClientPacket, _>(&mut stream, &mut read_buf).await {
            Ok(None) => break,
            Ok(Some(packet)) => {
                if channel.send(ClientContextEvent::Packet(packet)).is_err() {
                    break;
                }
            }
            Err(err) => {
                // println!("fuck you!!! fcuky wucky!! {:?}", err);
                return Err(err.into());
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum ChatLine {
    Text {
        peer_id: u64,
        message: String,
    },
    ConnectInfo {
        connection_id: u64,
        peer_ids: Vec<u64>,
    },
    Connected {
        peer_id: u64,
    },
    Disconnected {
        peer_id: u64,
    },
}

fn last_n<'a, T>(num: usize, slice: &'a [T]) -> &'a [T] {
    if slice.len() <= num {
        slice
    } else {
        &slice[slice.len() - num..]
    }
}

enum ControlFlow {
    Exit,
    Continue,
}

// struct MainState {
//     chat_lines: Vec<ChatLine>,
//     current_message_text: String,
// }

// impl MainState {
//     fn new() -> Self {
//         Self {
//             chat_lines: vec![],
//             current_message_text: String::new(),
//         }
//     }
// }

// struct SharedData {}

#[derive(Clone, Debug)]
enum AppEvent {
    IncomingPacket(ServerToClientPacket),
    Input(TerminalEvent),
}

#[derive(Clone, Debug)]
enum AppCommand {
    AddChatLine(ChatLine),
}

struct App {
    client: ClientContext,
    chat_lines: Vec<ChatLine>,
    current_message_text: String,

    current_username_text: String,

    command_tx: mpsc::UnboundedSender<AppCommand>,
    command_rx: mpsc::UnboundedReceiver<AppCommand>,
}

impl App {
    fn new(client: ClientContext) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        Self {
            client,
            chat_lines: vec![],
            current_message_text: String::new(),
            current_username_text: String::new(),
            command_rx,
            command_tx,
        }
    }

    fn render_peer<B: TerminalBackend>(&self, terminal: &mut B, peer_id: u64) -> ClientResult<()> {
        let (marker, color) = match peer_id == self.client.connection_id.unwrap() {
            true => ("~", Color::Cyan),
            false => ("", Color::Green),
        };

        write!(terminal.writer(), "<")?;
        terminal.submit_set_foreground_color(color)?;
        write!(terminal.writer(), "{}", marker)?;

        match self.client.connection_infos.get(&peer_id) {
            Some(info) => match &info.username {
                Some(username) => write!(terminal.writer(), "{}", username)?,
                None => write!(terminal.writer(), "?{}", peer_id)?,
            },
            None => {
                write!(terminal.writer(), "!{}", peer_id)?;
            }
        }

        terminal.submit_set_foreground_color(Color::Reset)?;
        write!(terminal.writer(), ">")?;

        Ok(())
    }

    fn render_chat_lines<B: TerminalBackend>(
        &self,
        terminal: &mut B,
        base: TerminalPos,
        lines: usize,
    ) -> ClientResult<()> {
        for (row, line) in last_n(lines, &self.chat_lines).iter().enumerate() {
            terminal.submit_goto(TerminalPos {
                row: base.row + row as u16,
                column: base.column,
            })?;
            match line {
                ChatLine::Text { peer_id, message } => {
                    self.render_peer(terminal, *peer_id)?;
                    write!(terminal.writer(), ": {}", message)?;
                }
                ChatLine::ConnectInfo {
                    connection_id,
                    peer_ids,
                } => {
                    terminal.submit_set_foreground_color(Color::Blue)?;
                    write!(terminal.writer(), "---")?;
                    terminal.submit_set_foreground_color(Color::Reset)?;

                    write!(terminal.writer(), " connected as ")?;
                    self.render_peer(terminal, *connection_id)?;

                    if peer_ids.len() > 0 {
                        write!(terminal.writer(), " to ")?;
                        self.render_peer(terminal, peer_ids[0])?;

                        for peer_id in &peer_ids[1..] {
                            write!(terminal.writer(), ", ")?;
                            self.render_peer(terminal, *peer_id)?;
                        }
                    }

                    terminal.submit_set_foreground_color(Color::Blue)?;
                    write!(terminal.writer(), " ---")?;
                    terminal.submit_set_foreground_color(Color::Reset)?;
                }
                ChatLine::Connected { peer_id } => {
                    terminal.submit_set_foreground_color(Color::Blue)?;
                    write!(terminal.writer(), ">> ")?;
                    terminal.submit_set_foreground_color(Color::Reset)?;
                    self.render_peer(terminal, *peer_id)?;
                    write!(terminal.writer(), " connected")?;
                }
                ChatLine::Disconnected { peer_id } => {
                    terminal.submit_set_foreground_color(Color::Blue)?;
                    write!(terminal.writer(), "<< ")?;
                    terminal.submit_set_foreground_color(Color::Reset)?;
                    self.render_peer(terminal, *peer_id)?;
                    write!(terminal.writer(), " disconnected")?;
                }
            }
        }

        Ok(())
    }

    fn render_chat_bar<B: TerminalBackend>(
        &self,
        terminal: &mut B,
        base: TerminalPos,
    ) -> ClientResult<()> {
        terminal.submit_goto(TerminalPos {
            row: base.row,
            column: base.column,
        })?;

        terminal.submit_set_foreground_color(Color::Yellow)?;
        write!(terminal.writer(), "=>")?;
        terminal.submit_set_foreground_color(Color::Reset)?;
        write!(terminal.writer(), " {}", self.current_message_text)?;

        Ok(())
    }

    fn redraw_message_ui<B: TerminalBackend>(&self, terminal: &mut B) -> ClientResult<()> {
        terminal.submit_clear()?;
        terminal.submit_goto(TerminalPos { row: 0, column: 0 })?;

        let size = terminal.size()?;

        self.render_chat_lines(
            terminal,
            TerminalPos { row: 0, column: 0 },
            size.rows as usize - 1,
        )?;
        self.render_chat_bar(
            terminal,
            TerminalPos {
                row: size.rows - 1,
                column: 0,
            },
        )?;

        terminal.flush()?;

        Ok(())
    }

    fn redraw_login_ui<B: TerminalBackend>(&self, terminal: &mut B) -> ClientResult<()> {
        terminal.submit_clear()?;
        terminal.submit_goto(TerminalPos { row: 0, column: 0 })?;

        let size = terminal.size()?;

        terminal.submit_set_foreground_color(Color::Yellow)?;
        write!(terminal.writer(), "username ::")?;
        terminal.submit_set_foreground_color(Color::Reset)?;
        write!(terminal.writer(), " {}", self.current_username_text)?;

        terminal.flush()?;

        Ok(())
    }

    async fn handle_input_event_main(&mut self, event: TerminalEvent) -> ClientResult<ControlFlow> {
        match event {
            TerminalEvent::Key(TerminalKeyEvent {
                kind: KeyEventKind::Char('c'),
                modifiers: KeyModifiers { control: true, .. },
            }) => return Ok(ControlFlow::Exit),

            TerminalEvent::Key(TerminalKeyEvent {
                kind: KeyEventKind::Backspace,
                ..
            }) => {
                self.current_message_text.pop();
            }

            TerminalEvent::Key(TerminalKeyEvent {
                kind: KeyEventKind::Enter,
                ..
            }) => {
                let line = std::mem::replace(&mut self.current_message_text, String::new());
                let self_id = self.client.connection_id.unwrap();
                let cmd = self.command_tx.clone();
                self.client.spawn_task(|mut task| async move {
                    send_peer_message(&mut task, cmd, self_id, line).await
                });
            }

            TerminalEvent::Key(TerminalKeyEvent {
                kind: KeyEventKind::Char(ch),
                ..
            }) => {
                self.current_message_text.push(ch);
            }
            _ => {}
        }

        Ok(ControlFlow::Continue)
    }

    async fn handle_input_event_login(
        &mut self,
        event: TerminalEvent,
    ) -> ClientResult<ControlFlow> {
        match event {
            TerminalEvent::Key(TerminalKeyEvent {
                kind: KeyEventKind::Backspace,
                ..
            }) => {
                self.current_username_text.pop();
            }

            TerminalEvent::Key(TerminalKeyEvent {
                kind: KeyEventKind::Enter,
                ..
            }) => {
                return Ok(ControlFlow::Exit);
            }

            TerminalEvent::Key(TerminalKeyEvent {
                kind: KeyEventKind::Char(ch),
                ..
            }) => {
                self.current_username_text.push(ch);
            }
            _ => {}
        }

        Ok(ControlFlow::Continue)
    }

    async fn handle_socket_event_main(
        &mut self,
        packet: ServerToClientPacket,
    ) -> ClientResult<ControlFlow> {
        match packet {
            ServerToClientPacket::PeerConnected { peer_id } => {
                self.chat_lines.push(ChatLine::Connected { peer_id });
            }
            ServerToClientPacket::PeerDisonnected { peer_id } => {
                self.chat_lines.push(ChatLine::Disconnected { peer_id });
            }
            ServerToClientPacket::PeerMessage { peer_id, message } => {
                self.chat_lines.push(ChatLine::Text { peer_id, message });
            }
            _ => todo!(),
        }

        Ok(ControlFlow::Continue)
    }

    async fn handle_event_main(&mut self, event: AppEvent) -> ClientResult<ControlFlow> {
        match event {
            AppEvent::IncomingPacket(event) => self.handle_socket_event_main(event).await,
            AppEvent::Input(event) => self.handle_input_event_main(event).await,
        }
    }

    async fn handle_event_login(&mut self, event: AppEvent) -> ClientResult<ControlFlow> {
        match event {
            AppEvent::IncomingPacket(event) => self.handle_socket_event_main(event).await,
            AppEvent::Input(event) => self.handle_input_event_login(event).await,
        }
    }

    async fn handle_command(&mut self, cmd: AppCommand) -> ClientResult<ControlFlow> {
        match cmd {
            AppCommand::AddChatLine(line) => self.chat_lines.push(line),
        }
        Ok(ControlFlow::Continue)
    }

    async fn run<B: TerminalBackend, E: EventsBackend>(
        &mut self,
        terminal: &mut B,
        mut events: E,
        username: Option<&str>,
    ) -> ClientResult<()> {
        let username = username.unwrap_or("[unknown]").into();

        let connect_sender = self.command_tx.clone();
        self.client.spawn_task(|mut task| async move {
            negotiate_connection(&mut task, connect_sender, username).await
        });

        self.redraw_message_ui(terminal)?;

        loop {
            tokio::select! {
                input_event = events.poll_event() => {
                    match self.handle_event_main(AppEvent::Input(input_event?.unwrap())).await? {
                        ControlFlow::Exit => break,
                        ControlFlow::Continue => {}
                    }
                }
                packet = self.client.recieve_packet() => {
                    if let Some(packet) = packet? {
                        match self.handle_event_main(AppEvent::IncomingPacket(packet)).await? {
                            ControlFlow::Exit => break,
                            ControlFlow::Continue => {}
                        }
                    }
                }
                cmd = self.command_rx.recv() => {
                    match self.handle_command(cmd.unwrap()).await? {
                        ControlFlow::Exit => break,
                        ControlFlow::Continue => {}
                    }
                }
            }

            self.redraw_message_ui(terminal)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
struct ClientTask {
    rid: ResponseId,
    inbound: mpsc::UnboundedReceiver<ServerToClientResponsePacket>,
    outbound: mpsc::UnboundedSender<ClientContextCommand>,
}

impl ClientTask {
    async fn send_packet(&mut self, packet: ClientToServerPacketKind) -> ClientResult<()> {
        self.send(ClientContextCommand::SendPacket(ClientToServerPacket {
            rid: self.rid,
            kind: packet,
        }))
        .await
    }

    async fn send(&mut self, cmd: ClientContextCommand) -> ClientResult<()> {
        self.outbound
            .send(cmd)
            .map_err(|err| ClientError::UnexpectedEndOfStream)?;
        Ok(())
    }

    async fn recv(&mut self) -> ClientResult<ServerToClientResponsePacket> {
        Ok(self
            .inbound
            .recv()
            .await
            .ok_or(ClientError::UnexpectedEndOfStream)?)
    }
}

async fn query_peers(task: &mut ClientTask, peers: Vec<u64>) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::RequestPeerInfo { peer_ids: peers })
        .await?;

    match task.recv().await.unwrap() {
        ServerToClientResponsePacket::PeerInfoResponse { peers } => {
            for (connection_id, info) in peers {
                task.send(ClientContextCommand::AddConnectionInfo {
                    connection_id,
                    info,
                })
                .await?;
            }
        }
        _ => todo!(),
    }

    Ok(())
}

async fn negotiate_connection(
    task: &mut ClientTask,
    cmd: mpsc::UnboundedSender<AppCommand>,
    username: String,
) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::Connect { username })
        .await?;

    let connection_id = match task.recv().await.unwrap() {
        ServerToClientResponsePacket::ConnectAck { connection_id } => {
            task.send(ClientContextCommand::SetConnectionId(connection_id))
                .await?;
            query_peers(task, vec![connection_id]).await?;
            connection_id
        }
        _ => todo!(),
    };

    task.send_packet(ClientToServerPacketKind::RequestPeerListing {})
        .await?;

    let peers = match task.recv().await.unwrap() {
        ServerToClientResponsePacket::PeerListingResponse { peers } => {
            for peer_id in peers.iter().copied() {
                task.send(ClientContextCommand::AddPeer(peer_id)).await?;
            }
            query_peers(task, peers.clone()).await?;
            peers
        }
        _ => todo!(),
    };

    cmd.send(AppCommand::AddChatLine(ChatLine::ConnectInfo {
        connection_id,
        peer_ids: peers,
    }))
    .unwrap();

    Ok(())
}

async fn send_peer_message(
    task: &mut ClientTask,
    cmd: mpsc::UnboundedSender<AppCommand>,
    self_id: u64,
    message: String,
) -> ClientResult<()> {
    task.send_packet(ClientToServerPacketKind::Message {
        message: message.clone(),
    })
    .await?;

    match task.recv().await.unwrap() {
        ServerToClientResponsePacket::MessageAck {} => {
            cmd.send(AppCommand::AddChatLine(ChatLine::Text {
                peer_id: self_id,
                message,
            }))
            .unwrap();
        }
        _ => todo!(),
    }

    Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalPos {
    pub column: u16,
    pub row: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalRect {
    pub start: TerminalPos,
    pub end: TerminalPos,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum KeyEventKind {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Insert,
    Null,
    Esc,

    Function(u8),
    Char(char),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct KeyModifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct TerminalKeyEvent {
    pub kind: KeyEventKind,
    pub modifiers: KeyModifiers,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollDown,
    ScrollUp,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct TerminalMouseEvent {
    pub kind: MouseEventKind,
    pub location: TerminalPos,
    pub modifiers: KeyModifiers,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum TerminalEvent {
    Key(TerminalKeyEvent),
    Mouse(TerminalMouseEvent),
    Resize(TerminalSize),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Color {
    Reset,
    Black,
    DarkGrey,
    Red,
    DarkRed,
    Green,
    DarkGreen,
    Yellow,
    DarkYellow,
    Blue,
    DarkBlue,
    Magenta,
    DarkMagenta,
    Cyan,
    DarkCyan,
    White,
    Grey,
    Rgb { r: u8, g: u8, b: u8 },
    AnsiValue(u8),
}

pub async fn run_client(username: Option<&str>) -> ClientResult<()> {
    let stream = TcpStream::connect("127.0.0.1:8080").await?;

    let client = ClientContext::new(stream);
    let mut app = App::new(client);

    let mut terminal = backend::CrosstermBackend::new(std::io::stdout())?;
    let events = backend::CrosstermEventsBackend::new();
    app.run(&mut terminal, events, username).await?;

    Ok(())
}
