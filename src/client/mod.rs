use crate::common::{packet::*, CommonError};
use std::io::Write;
use tokio::{net::TcpStream, sync::mpsc};

use self::{
    backend::{EventsBackend, TerminalBackend},
    connection_handler::ConnectionHandler,
};

pub mod backend;
pub mod connection_handler;
pub mod net_handlers;
pub mod terminal;
pub mod ui;

use terminal::*;

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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ChatLine {
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

#[derive(Clone, Debug)]
enum AppEvent {
    IncomingPacket(ServerToClientPacket),
    Input(TerminalEvent),
}

#[derive(Clone, Debug)]
pub enum AppCommand {
    AddChatLine(ChatLine),
}

struct App {
    client: ConnectionHandler,
    chat_lines: Vec<ChatLine>,
    current_message_text: String,

    command_tx: mpsc::UnboundedSender<AppCommand>,
    command_rx: mpsc::UnboundedReceiver<AppCommand>,
}

impl App {
    fn new(client: ConnectionHandler) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        Self {
            client,
            chat_lines: vec![],
            current_message_text: String::new(),
            command_rx,
            command_tx,
        }
    }

    fn render_peer<B: TerminalBackend>(&self, terminal: &mut B, peer_id: u64) -> ClientResult<()> {
        let (marker, color) = match peer_id == self.client.connection_id().unwrap() {
            true => ("~", Color::Cyan),
            false => ("", Color::Green),
        };

        write!(terminal.writer(), "<")?;
        terminal.submit_set_foreground_color(color)?;
        write!(terminal.writer(), "{}", marker)?;

        match self.client.connection_info(peer_id) {
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
                let self_id = self.client.connection_id().unwrap();
                let cmd = self.command_tx.clone();
                self.client
                    .spawn_task(|task| net_handlers::send_peer_message(task, cmd, self_id, line));
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

    async fn handle_socket_event_main(
        &mut self,
        packet: ServerToClientPacket,
    ) -> ClientResult<ControlFlow> {
        match packet {
            ServerToClientPacket::PeerConnected { peer_id } => {
                let cmd = self.command_tx.clone();
                self.client
                    .spawn_task(|task| net_handlers::handle_peer_connect(task, cmd, peer_id));
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
        self.client.spawn_task(move |task| {
            net_handlers::negotiate_connection(task, connect_sender, username)
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

pub async fn run_client(username: Option<&str>) -> ClientResult<()> {
    let stream = TcpStream::connect("127.0.0.1:8080").await?;

    let client = ConnectionHandler::new(stream);
    let mut app = App::new(client);

    let mut terminal = backend::CrosstermBackend::new(std::io::stdout())?;
    let events = backend::CrosstermEventsBackend::new();
    app.run(&mut terminal, events, username).await?;

    Ok(())
}
