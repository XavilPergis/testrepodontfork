use crate::{
    client::widget::{AxisConstraint, BoxConstraints},
    common::{packet::*, CommonError},
};
use std::borrow::Cow;
use tokio::{net::TcpStream, sync::mpsc};

use self::{
    backend::{EventsBackend, TerminalBackend},
    connection_handler::ConnectionHandler,
    widget::{
        style::{StyledSegment, StyledText},
        widgets::*,
        VerticalSplitView, Widget,
    },
};

pub mod backend;
pub mod connection_handler;
pub mod net_handlers;
pub mod terminal;
pub mod widget;

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
        id: ClientId,
        message: String,
    },
    ConnectInfo {
        id: ClientId,
        peer_ids: Vec<ClientId>,
    },
    Connected {
        id: ClientId,
    },
    Disconnected {
        id: ClientId,
    },
}

pub fn last_of<'a, T>(num: usize, slice: &'a [T]) -> &'a [T] {
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

#[derive(Debug)]
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

    fn get_display_tag(&self, client_id: ClientId) -> Cow<str> {
        match self.client.connection_info(client_id) {
            Some(info) => match &info.username {
                Some(username) => username.as_str().into(),
                None => Cow::Owned(format!("?{}", client_id)),
            },
            None => Cow::Owned(format!("!{}", client_id)),
        }
    }

    fn add_connection_spans<'a>(&'a self, client_id: ClientId, text: &mut StyledText<'a>) {
        let (marker, color) = match client_id == self.client.client_id().unwrap() {
            true => ("~", Color::Cyan),
            false => ("", Color::Green),
        };

        text.add_span("<")
            .add_span(StyledSegment {
                text: marker.into(),
                style: TerminalCellStyle::default().with_fg_color(color),
            })
            .add_span(StyledSegment {
                text: self.get_display_tag(client_id),
                style: TerminalCellStyle::default().with_fg_color(color),
            })
            .add_span(">");
    }

    fn build_chat_line_widget<'a>(&'a self, line: &'a ChatLine) -> Box<dyn Widget + 'a> {
        let mut text = StyledText::new();
        match line {
            ChatLine::Text {
                id: peer_id,
                message,
            } => {
                self.add_connection_spans(*peer_id, &mut text);
                text.add_span(": ");
                text.add_span(message.as_str());
            }

            ChatLine::ConnectInfo {
                id: client_id,
                peer_ids,
            } => {
                text.add_span(StyledSegment {
                    text: "---".into(),
                    style: TerminalCellStyle::default().with_fg_color(Color::Blue),
                });

                text.add_span(" connected as ");
                self.add_connection_spans(*client_id, &mut text);

                if peer_ids.len() > 0 {
                    text.add_span(" to ");
                    self.add_connection_spans(peer_ids[0], &mut text);

                    for &peer_id in &peer_ids[1..] {
                        text.add_span(", ");
                        self.add_connection_spans(peer_id, &mut text);
                    }
                }

                text.add_span(StyledSegment {
                    text: " ---".into(),
                    style: TerminalCellStyle::default().with_fg_color(Color::Blue),
                });
            }

            ChatLine::Connected { id: peer_id } => {
                text.add_span(StyledSegment {
                    text: ">> ".into(),
                    style: TerminalCellStyle::default().with_fg_color(Color::Blue),
                });
                self.add_connection_spans(*peer_id, &mut text);
                text.add_span(" connected");
            }

            ChatLine::Disconnected { id: peer_id } => {
                text.add_span(StyledSegment {
                    text: "<< ".into(),
                    style: TerminalCellStyle::default().with_fg_color(Color::Red),
                });
                self.add_connection_spans(*peer_id, &mut text);
                text.add_span(" disconnected");
            }
        }

        Box::new(TextWidget::new(text))
    }

    fn build_chat_widget(&self) -> Box<dyn Widget + '_> {
        let chat_lines = VerticalListWidget::new(
            self.chat_lines
                .iter()
                .map(|line| self.build_chat_line_widget(line))
                .collect(),
        );

        let chat_bar = TextWidget::new(
            StyledText::new()
                .with_span(StyledSegment {
                    text: "=> ".into(),
                    style: TerminalCellStyle::default().with_fg_color(Color::Yellow),
                })
                .with_span(self.current_message_text.as_str()),
        );

        Box::new(VerticalSplitView::new(chat_bar, chat_lines))
    }

    fn build_room_list_widget(&self) -> Box<dyn Widget + '_> {
        let mut room_widgets = vec![];
        for (_, info) in self.client.rooms() {
            if let Some(info) = info {
                room_widgets.push(TextWidget::new(info.name.as_str()));
            }
        }
        Box::new(VerticalListWidget::new(room_widgets))
    }

    fn build_widget_tree(&self) -> Box<dyn Widget + '_> {
        // self.build_chat_widget()
        self.build_room_list_widget()
    }

    fn redraw_message_ui<B: TerminalBackend>(&self, terminal: &mut B) -> ClientResult<()> {
        let mut widget = self.build_widget_tree();

        widget.layout(BoxConstraints {
            width: AxisConstraint::exactly(terminal.size()?.width),
            height: AxisConstraint::exactly(terminal.size()?.height),
        });

        // eprintln!("{:?}", widget);

        let mut buffer = vec![
            TerminalCell::default();
            terminal.size()?.width as usize * terminal.size()?.height as usize
        ]
        .into_boxed_slice();

        widget.render(Frame::root(terminal.size()?, &mut buffer));

        terminal.redraw(&buffer)?;

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
                if let Some(self_id) = self.client.client_id() {
                    let line = std::mem::replace(&mut self.current_message_text, String::new());
                    let cmd = self.command_tx.clone();
                    self.client.spawn_task(|task| {
                        net_handlers::send_peer_message(task, cmd, self_id, line)
                    });
                }
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
            ServerToClientPacket::PeerConnected { room_id, peer_id } => {
                let cmd = self.command_tx.clone();
                self.client
                    .spawn_task(|task| net_handlers::handle_peer_connect(task, cmd, peer_id));
            }
            ServerToClientPacket::PeerDisconnected { room_id, peer_id } => {
                self.chat_lines.push(ChatLine::Disconnected { id: peer_id });
            }
            ServerToClientPacket::PeerMessage {
                room_id,
                peer_id,
                message,
            } => {
                self.chat_lines.push(ChatLine::Text {
                    id: peer_id,
                    message,
                });
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

    let (mut terminal, events) = backend::CrosstermBackend::new(std::io::stdout())?;
    app.run(&mut terminal, events, username).await?;

    Ok(())
}
