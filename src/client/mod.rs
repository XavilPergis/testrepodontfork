use crate::common::{packet::*, CommonError, CommonResult};
use termion::{event::Key, input::TermRead, raw::IntoRawMode};
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::mpsc,
};
use tui::{
    backend::{Backend, TermionBackend},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Widget, Wrap},
    Frame, Terminal,
};

#[derive(Debug)]
pub enum ClientError {
    Common(CommonError),
    UnexpectedEndOfStream,
    NoConnectAck,
    NoDisconnectAck,
    NoPeerListing,
    NoMessageAck,
}

impl<T: Into<CommonError>> From<T> for ClientError {
    fn from(err: T) -> Self {
        ClientError::Common(err.into())
    }
}

pub type ClientResult<T> = Result<T, ClientError>;

// async fn read_messages(mut stream: OwnedReadHalf) -> ClientResult<()> {
//     loop {
//         let message = read_message(&mut stream).await?.unwrap();
//         println!("got {:?}", message);
//     }

//     Ok(())
// }

#[derive(Debug)]
struct ClientContext {
    stream: OwnedWriteHalf,
    write_buf: Vec<u8>,

    connection_id: Option<u64>,
    peer_ids: Vec<u64>,

    inbound_packets: mpsc::UnboundedReceiver<ServerToClientPacket>,
}

async fn packet_read_loop(
    mut stream: OwnedReadHalf,
    channel: mpsc::UnboundedSender<ServerToClientPacket>,
) -> ClientResult<()> {
    let mut read_buf = Vec::with_capacity(4096);

    loop {
        match read_whole_packet::<ServerToClientPacket, _>(&mut stream, &mut read_buf).await {
            Ok(None) => break,
            Ok(Some(packet)) => {
                if channel.send(packet).is_err() {
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

impl ClientContext {
    fn new(stream: TcpStream) -> Self {
        let (tcp_reader, tcp_writer) = stream.into_split();
        let (tx, inbound_packets) = mpsc::unbounded_channel();
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
            inbound_packets,
        }
    }

    async fn write_packet(&mut self, packet: &ClientToServerPacket) -> CommonResult<()> {
        write_whole_packet(&mut self.stream, &mut self.write_buf, packet).await
    }

    async fn expect_packet(&mut self) -> ClientResult<ServerToClientPacket> {
        match self.inbound_packets.recv().await {
            Some(value) => Ok(value),
            None => Err(ClientError::UnexpectedEndOfStream),
        }
    }

    async fn next_packet(&mut self) -> Option<ServerToClientPacket> {
        self.inbound_packets.recv().await
    }

    async fn negotiate_connection(&mut self) -> ClientResult<()> {
        self.write_packet(&ClientToServerPacket::Connect).await?;
        match self.expect_packet().await? {
            ServerToClientPacket::ConnectAck { connection_id } => {
                self.connection_id = Some(connection_id)
            }
            _ => return Err(ClientError::NoConnectAck),
        }

        self.write_packet(&ClientToServerPacket::RequestPeerListing)
            .await?;
        match self.expect_packet().await? {
            ServerToClientPacket::PeerListingResponse { peers } => self.peer_ids.extend(peers),
            _ => return Err(ClientError::NoPeerListing),
        }

        log::info!("connected to {} peers", self.peer_ids.len());
        Ok(())
    }

    async fn negotiate_disconnection(&mut self) -> ClientResult<()> {
        self.write_packet(&ClientToServerPacket::Disconnect).await?;
        match self.expect_packet().await? {
            ServerToClientPacket::DisconnectAck => {}
            _ => return Err(ClientError::NoDisconnectAck),
        }

        log::info!("disconnected from {} peers", self.peer_ids.len());
        Ok(())
    }

    async fn send_message(&mut self, message: &str) -> ClientResult<()> {
        self.write_packet(&ClientToServerPacket::Message {
            message: message.into(),
        })
        .await?;
        match self.expect_packet().await? {
            ServerToClientPacket::MessageAck => {}
            _ => return Err(ClientError::NoMessageAck),
        }

        Ok(())
    }
}

enum ChatLine {
    Text { peer_id: u64, message: String },
    Connected { peer_id: u64 },
    Disconnected { peer_id: u64 },
}

// struct MessageWindowWidget<'a> {
//     lines: Vec<ChatMessage<'a>>,
// }

// impl<'a> Widget for MessageWindowWidget<'a> {
//     fn render(self, area: Rect, buf: &mut tui::buffer::Buffer) {

//     }
// }

fn last_n<'a, T>(num: usize, slice: &'a [T]) -> &'a [T] {
    if slice.len() <= num {
        slice
    } else {
        &slice[slice.len() - num..]
    }
}

struct App {
    chat_lines: Vec<ChatLine>,
    current_message_text: String,
    running: bool,

    key_events: mpsc::UnboundedReceiver<Key>,
    client: ClientContext,
}

impl App {
    fn new(client: ClientContext, key_events: mpsc::UnboundedReceiver<Key>) -> Self {
        Self {
            chat_lines: Vec::new(),
            current_message_text: String::new(),
            running: true,
            key_events,
            client,
        }
    }

    fn add_peer_spans(&self, peer_id: u64, spans: &mut Vec<Span>) {
        if peer_id == self.client.connection_id.unwrap() {
            spans.push(Span::styled("%", Style::default().fg(Color::Cyan)));
            spans.push(Span::styled(
                peer_id.to_string(),
                Style::default().fg(Color::Cyan),
            ));
        } else {
            spans.push(Span::styled("#", Style::default().fg(Color::Green)));
            spans.push(Span::styled(
                peer_id.to_string(),
                Style::default().fg(Color::Green),
            ));
        }
    }

    fn render_chat_lines<B: Backend>(&mut self, frame: &mut Frame<B>, area: Rect) {
        let chat_line_items = last_n(area.height as usize, &self.chat_lines)
            .iter()
            .map(|line| {
                let mut spans = vec![];
                match line {
                    ChatLine::Text { peer_id, message } => {
                        self.add_peer_spans(*peer_id, &mut spans);
                        spans.push(Span::raw(": "));
                        spans.push(Span::raw(message));
                    }
                    ChatLine::Connected { peer_id } => {
                        spans.push(Span::styled(
                            ">> ",
                            Style::default()
                                .fg(Color::Blue)
                                .add_modifier(Modifier::BOLD),
                        ));
                        self.add_peer_spans(*peer_id, &mut spans);
                        spans.push(Span::raw(" connected"));
                    }
                    ChatLine::Disconnected { peer_id } => {
                        spans.push(Span::styled(
                            "<< ",
                            Style::default()
                                .fg(Color::Blue)
                                .add_modifier(Modifier::BOLD),
                        ));
                        self.add_peer_spans(*peer_id, &mut spans);
                        spans.push(Span::raw(" disconnected"));
                    }
                }

                ListItem::new(Text::from(Spans::from(spans)))
            })
            .collect::<Vec<_>>();

        frame.render_widget(List::new(chat_line_items), area);
    }

    fn redraw<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> ClientResult<()> {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [Constraint::Max(frame.size().height - 2), Constraint::Max(2)].as_ref(),
                )
                .split(frame.size());

            self.render_chat_lines(frame, chunks[0]);

            let message_text = vec![Spans::from(vec![
                Span::styled("=> ", Style::default().fg(Color::Yellow)),
                Span::raw(&self.current_message_text),
            ])];
            let paragraph = Paragraph::new(message_text).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, chunks[1]);
        })?;

        Ok(())
    }

    async fn handle_input_event(&mut self, event: Key) -> ClientResult<()> {
        match event {
            Key::Esc | Key::Ctrl('c') => self.running = false,
            Key::Char('\n') => {
                let line = std::mem::replace(&mut self.current_message_text, String::new());

                self.client.send_message(&line).await?;
                self.chat_lines.push(ChatLine::Text {
                    peer_id: self.client.connection_id.unwrap(),
                    message: line,
                });
            }
            Key::Char(ch) => {
                self.current_message_text.push(ch);
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_socket_event(&mut self, packet: ServerToClientPacket) -> ClientResult<()> {
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
        Ok(())
    }

    async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> ClientResult<()> {
        terminal.clear()?;
        self.redraw(terminal)?;

        self.client.negotiate_connection().await?;

        loop {
            tokio::select! {
                input_event = self.key_events.recv() => {
                    self.handle_input_event(input_event.unwrap()).await?;
                    self.redraw(terminal)?;

                    if !self.running {
                        break;
                    }
                }

                socket_event = self.client.next_packet() => {
                    if socket_event.is_none() {
                        break;
                    }

                    self.handle_socket_event(socket_event.unwrap()).await?;
                    self.redraw(terminal)?;

                    if !self.running {
                        break;
                    }
                }
            }
        }

        self.client.negotiate_disconnection().await?;

        Ok(())
    }
}

pub async fn run_client() -> ClientResult<()> {
    let stream = TcpStream::connect("127.0.0.1:8080").await?;
    let ctx = ClientContext::new(stream);

    let (tx, rx) = mpsc::unbounded_channel();

    // input collection task
    tokio::spawn(async move {
        for event in std::io::stdin().keys() {
            match event {
                Ok(event) => {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let stdout = std::io::stdout().into_raw_mode()?;
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(ctx, rx);
    app.run(&mut terminal).await?;

    Ok(())
}
