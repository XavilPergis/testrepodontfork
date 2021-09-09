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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalCell {
    pub foreground_color: Color,
    pub background_color: Color,
    pub character: Option<char>,
}

pub struct Frame<'buf> {
    root_size: TerminalSize,
    root_buffer: &'buf mut [TerminalCell],

    frame_bounds: TerminalRect,
}

impl<'buf> Frame<'buf> {
    pub fn root(size: TerminalSize, buffer: &'buf mut [TerminalCell]) -> Self {
        Self {
            root_size: size,
            root_buffer: buffer,
            frame_bounds: TerminalRect {
                end: TerminalPos {
                    column: size.width,
                    row: size.height,
                },
                ..TerminalRect::default()
            },
        }
    }

    pub fn with_view(&'buf mut self, area: TerminalRect, func: impl FnOnce(Frame<'buf>)) {
        func(Self {
            root_size: self.root_size,
            root_buffer: self.root_buffer,
            frame_bounds: TerminalRect {
                start: self.frame_bounds.start + area.start,
                end: self.frame_bounds.end + area.end,
            },
        })
    }

    fn buffer_index(&self, offset: TerminalPos) -> usize {
        let root_offset = offset + self.frame_bounds.start;
        debug_assert!(root_offset.row <= self.frame_bounds.end.row);
        debug_assert!(root_offset.column <= self.frame_bounds.end.column);
        root_offset.row as usize * self.root_size.width as usize + root_offset.column as usize
    }

    pub fn put(&mut self, offset: TerminalPos, cell: TerminalCell) {
        self.root_buffer[self.buffer_index(offset)] = cell;
    }

    pub fn get(&self, offset: TerminalPos) -> &TerminalCell {
        &self.root_buffer[self.buffer_index(offset)]
    }

    pub fn width(&self) -> u16 {
        self.frame_bounds.width()
    }

    pub fn height(&self) -> u16 {
        self.frame_bounds.height()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum AxisConstraint {
    None,
    Exactly(u16),
    Min(u16),
    Max(u16),
}

impl AxisConstraint {
    pub fn max_bound(&self) -> Option<u16> {
        match self {
            AxisConstraint::Min(_) | AxisConstraint::None => None,
            AxisConstraint::Max(axis) | AxisConstraint::Exactly(axis) => Some(*axis),
        }
    }

    pub fn min_bound(&self) -> Option<u16> {
        match self {
            AxisConstraint::Max(_) | AxisConstraint::None => None,
            AxisConstraint::Min(axis) | AxisConstraint::Exactly(axis) => Some(*axis),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct BoxConstraints {
    width: AxisConstraint,
    height: AxisConstraint,
}

impl Default for BoxConstraints {
    fn default() -> Self {
        Self {
            width: AxisConstraint::None,
            height: AxisConstraint::None,
        }
    }
}

pub struct WidgetContext {}

pub trait Widget: std::fmt::Debug {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize;
    fn render(&mut self, frame: Frame<'_>);
}

fn apply_constraint(axis: u16, constraint: AxisConstraint) -> (bool, u16) {
    match constraint {
        AxisConstraint::None => (false, axis),
        AxisConstraint::Min(min) => (min > axis, u16::max(axis, min)),
        AxisConstraint::Max(max) => (axis > max, u16::min(axis, max)),
        AxisConstraint::Exactly(value) => (axis != value, value),
    }
}

fn apply_constraints(size: TerminalSize, constraints: BoxConstraints) -> TerminalSize {
    TerminalSize {
        height: apply_constraint(size.height, constraints.height).1,
        width: apply_constraint(size.width, constraints.width).1,
    }
}

#[derive(Clone, Debug)]
pub struct TextWidget<'a> {
    text: &'a str,
    computed_size: Option<TerminalSize>,
    computed_lines: Vec<&'a str>,
}

impl<'a> TextWidget<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            computed_size: None,
            computed_lines: Vec::new(),
        }
    }
}

impl<'a> Widget for TextWidget<'a> {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize {
        let mut bounds = TerminalSize {
            width: 0,
            height: 1,
        };

        let max_width = constraints.width.max_bound();

        let mut line_start = 0;
        let mut current_pos = TerminalPos::default();
        for (idx, ch) in self.text.char_indices() {
            match ch {
                '\n' => {
                    current_pos.row += 1;
                    current_pos.column = 0;
                    bounds.height = u16::max(bounds.height, current_pos.row);
                    self.computed_lines
                        .push(self.text[line_start..idx].trim_end());
                    line_start = idx + 1;
                }
                ch if ch.is_whitespace() => {
                    current_pos.column += 1;
                }
                _ => {
                    current_pos.column += 1;
                    if max_width
                        .map(|max| current_pos.column > max)
                        .unwrap_or(false)
                    {
                        current_pos.row += 1;
                        current_pos.column = 0;
                        bounds.height = u16::max(bounds.height, current_pos.row);
                        self.computed_lines
                            .push(&self.text[line_start..idx].trim_end());
                        line_start = idx + 1;
                    } else {
                        bounds.width = u16::max(bounds.width, current_pos.column);
                    }
                }
            }
        }
        self.computed_lines
            .push(&self.text[line_start..].trim_end());

        bounds = apply_constraints(bounds, constraints);
        self.computed_size = Some(bounds);
        bounds
    }

    fn render(&mut self, mut frame: Frame<'_>) {
        for (row, line) in self.computed_lines.iter().enumerate() {
            for (column, ch) in line.chars().enumerate() {
                frame.put(
                    TerminalPos {
                        column: column as u16,
                        row: row as u16,
                    },
                    TerminalCell {
                        character: Some(ch),
                        ..Default::default()
                    },
                );
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct VerticalListWidget<W> {
    children: Vec<W>,
    computed_sizes: Vec<TerminalSize>,
    computed_size: Option<TerminalSize>,
}

impl<W: Widget> Widget for VerticalListWidget<W> {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize {
        let mut total_height = 0;
        let mut max_width = 0;
        for child in self.children.iter_mut().rev() {
            // don't layout children we can't see!
            if constraints
                .height
                .max_bound()
                .map(|max| total_height > max)
                .unwrap_or(false)
            {
                break;
            }

            let child_size = child.layout(BoxConstraints {
                height: AxisConstraint::None,
                ..constraints
            });
            total_height += child_size.height;
            max_width = u16::max(max_width, child_size.width);
            self.computed_sizes.push(child_size);
        }

        let mut bounds = TerminalSize {
            width: max_width,
            height: total_height,
        };

        bounds = apply_constraints(bounds, constraints);
        self.computed_size = Some(bounds);
        bounds
    }

    fn render(&mut self, mut frame: Frame<'_>) {
        let mut current_base = self.computed_size.unwrap().height;
        for (child, &child_size) in
            Iterator::zip(self.children.iter_mut().rev(), self.computed_sizes.iter()).rev()
        {
            let foo = TerminalRect {
                start: TerminalPos {
                    column: 0,
                    row: current_base,
                },
                end: TerminalPos {
                    column: self.computed_size.unwrap().width,
                    row: current_base + child_size.height,
                },
            };
            frame.with_view(foo, move |frame2| {
                child.render(frame2);
            });
        }
    }
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

    // fn render_peer_2(&self, area: TerminalRect, peer_id: u64) -> ClientResult<Frame> {
    //     let (marker, color) = match peer_id == self.client.connection_id().unwrap() {
    //         true => ("~", Color::Cyan),
    //         false => ("", Color::Green),
    //     };

    //     write!(terminal.writer(), "<")?;
    //     terminal.submit_set_foreground_color(color)?;
    //     write!(terminal.writer(), "{}", marker)?;

    //     match self.client.connection_info(peer_id) {
    //         Some(info) => match &info.username {
    //             Some(username) => write!(terminal.writer(), "{}", username)?,
    //             None => write!(terminal.writer(), "?{}", peer_id)?,
    //         },
    //         None => {
    //             write!(terminal.writer(), "!{}", peer_id)?;
    //         }
    //     }

    //     terminal.submit_set_foreground_color(Color::Reset)?;
    //     write!(terminal.writer(), ">")?;

    //     Ok(frame)
    // }

    // fn render_peer<B: TerminalBackend>(&self, terminal: &mut B, peer_id: u64) -> ClientResult<()> {
    //     let (marker, color) = match peer_id == self.client.connection_id().unwrap() {
    //         true => ("~", Color::Cyan),
    //         false => ("", Color::Green),
    //     };

    //     write!(terminal.writer(), "<")?;
    //     terminal.submit_set_foreground_color(color)?;
    //     write!(terminal.writer(), "{}", marker)?;

    //     match self.client.connection_info(peer_id) {
    //         Some(info) => match &info.username {
    //             Some(username) => write!(terminal.writer(), "{}", username)?,
    //             None => write!(terminal.writer(), "?{}", peer_id)?,
    //         },
    //         None => {
    //             write!(terminal.writer(), "!{}", peer_id)?;
    //         }
    //     }

    //     terminal.submit_set_foreground_color(Color::Reset)?;
    //     write!(terminal.writer(), ">")?;

    //     Ok(())
    // }

    // fn render_chat_lines<B: TerminalBackend>(
    //     &self,
    //     terminal: &mut B,
    //     area: TerminalRect,
    // ) -> ClientResult<impl Widget> {
    //     for (row, line) in last_n(lines, &self.chat_lines).iter().enumerate() {
    //         terminal.submit_goto(TerminalPos {
    //             row: base.row + row as u16,
    //             column: base.column,
    //         })?;
    //         match line {
    //             ChatLine::Text { peer_id, message } => {
    //                 self.render_peer(terminal, *peer_id)?;
    //                 write!(terminal.writer(), ": {}", message)?;
    //             }
    //             ChatLine::ConnectInfo {
    //                 connection_id,
    //                 peer_ids,
    //             } => {
    //                 terminal.submit_set_foreground_color(Color::Blue)?;
    //                 write!(terminal.writer(), "---")?;
    //                 terminal.submit_set_foreground_color(Color::Reset)?;

    //                 write!(terminal.writer(), " connected as ")?;
    //                 self.render_peer(terminal, *connection_id)?;

    //                 if peer_ids.len() > 0 {
    //                     write!(terminal.writer(), " to ")?;
    //                     self.render_peer(terminal, peer_ids[0])?;

    //                     for peer_id in &peer_ids[1..] {
    //                         write!(terminal.writer(), ", ")?;
    //                         self.render_peer(terminal, *peer_id)?;
    //                     }
    //                 }

    //                 terminal.submit_set_foreground_color(Color::Blue)?;
    //                 write!(terminal.writer(), " ---")?;
    //                 terminal.submit_set_foreground_color(Color::Reset)?;
    //             }
    //             ChatLine::Connected { peer_id } => {
    //                 terminal.submit_set_foreground_color(Color::Blue)?;
    //                 write!(terminal.writer(), ">> ")?;
    //                 terminal.submit_set_foreground_color(Color::Reset)?;
    //                 self.render_peer(terminal, *peer_id)?;
    //                 write!(terminal.writer(), " connected")?;
    //             }
    //             ChatLine::Disconnected { peer_id } => {
    //                 terminal.submit_set_foreground_color(Color::Blue)?;
    //                 write!(terminal.writer(), "<< ")?;
    //                 terminal.submit_set_foreground_color(Color::Reset)?;
    //                 self.render_peer(terminal, *peer_id)?;
    //                 write!(terminal.writer(), " disconnected")?;
    //             }
    //         }
    //     }

    //     Ok(())
    // }

    // fn render_chat_bar<B: TerminalBackend>(
    //     &self,
    //     terminal: &mut B,
    //     area: TerminalRect,
    // ) -> ClientResult<()> {
    //     terminal.submit_goto(TerminalPos {
    //         row: area.start.row,
    //         column: area.start.column,
    //     })?;

    //     terminal.submit_set_foreground_color(Color::Yellow)?;
    //     write!(terminal.writer(), "=>")?;
    //     terminal.submit_set_foreground_color(Color::Reset)?;
    //     write!(terminal.writer(), " {}", self.current_message_text)?;

    //     Ok(())
    // }

    fn build_widget_tree(&self) -> impl Widget + '_ {
        TextWidget::new("hello!!!")
    }

    fn redraw_message_ui<B: TerminalBackend>(&self, terminal: &mut B) -> ClientResult<()> {
        let mut widget = self.build_widget_tree();

        widget.layout(BoxConstraints {
            width: AxisConstraint::Max(terminal.size()?.width),
            height: AxisConstraint::Max(terminal.size()?.height),
        });

        eprintln!("{:?}", widget);

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
