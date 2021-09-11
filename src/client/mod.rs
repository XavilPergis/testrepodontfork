use crate::common::{packet::*, CommonError};
use std::borrow::Cow;
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalCell {
    pub style: TerminalCellStyle,
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

    pub fn render_widget<W: Widget>(&mut self, area: TerminalRect, widget: &mut W) {
        widget.render(Frame {
            root_size: self.root_size,
            root_buffer: self.root_buffer,
            frame_bounds: TerminalRect {
                start: self.frame_bounds.start + area.start,
                end: self.frame_bounds.end + area.end,
            },
        });
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
pub struct AxisConstraint {
    pub min: Option<u16>,
    pub max: Option<u16>,
}

impl AxisConstraint {
    pub fn unconstrained() -> AxisConstraint {
        Self {
            min: None,
            max: None,
        }
    }

    pub fn bounded_maximum(value: u16) -> AxisConstraint {
        Self {
            min: None,
            max: Some(value),
        }
    }

    pub fn bounded_minimum(value: u16) -> AxisConstraint {
        Self {
            min: Some(value),
            max: None,
        }
    }

    pub fn bounded(min: u16, max: u16) -> AxisConstraint {
        Self {
            min: Some(min),
            max: Some(max),
        }
    }
}

impl AxisConstraint {
    pub fn overflows(&self, value: u16) -> bool {
        match self.max {
            Some(max) => value > max,
            None => false,
        }
    }

    pub fn underflows(&self, value: u16) -> bool {
        match self.min {
            Some(min) => value < min,
            None => false,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct BoxConstraints {
    pub width: AxisConstraint,
    pub height: AxisConstraint,
}

impl Default for BoxConstraints {
    fn default() -> Self {
        Self {
            width: AxisConstraint::unconstrained(),
            height: AxisConstraint::unconstrained(),
        }
    }
}

pub struct WidgetContext {}

pub trait Widget: std::fmt::Debug {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize;
    fn render<'buf>(&mut self, frame: Frame<'buf>);
}

impl<'a> Widget for Box<dyn Widget + 'a> {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize {
        Widget::layout(&mut **self, constraints)
    }

    fn render<'buf>(&mut self, frame: Frame<'buf>) {
        Widget::render(&mut **self, frame)
    }
}

fn apply_axis_constraint(constraint: AxisConstraint, value: u16) -> u16 {
    let value = constraint.max.map(|max| value.min(max)).unwrap_or(value);
    let value = constraint.min.map(|min| value.max(min)).unwrap_or(value);
    value
}

fn apply_constraints(constraints: BoxConstraints, width: u16, height: u16) -> TerminalSize {
    TerminalSize {
        height: apply_axis_constraint(constraints.height, height),
        width: apply_axis_constraint(constraints.width, width),
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalCellStyle {
    pub foreground_color: Color,
    pub background_color: Color,
}

impl TerminalCellStyle {
    pub fn with_fg_color(mut self, color: Color) -> Self {
        self.foreground_color = color;
        self
    }
    pub fn with_bg_color(mut self, color: Color) -> Self {
        self.foreground_color = color;
        self
    }
}

#[derive(Clone, Debug)]
pub struct StyledSpan<'a> {
    pub text: Cow<'a, str>,
    pub style: TerminalCellStyle,
}

impl<'a> From<&'a str> for StyledSpan<'a> {
    fn from(text: &'a str) -> Self {
        Self {
            text: text.into(),
            style: TerminalCellStyle::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Styled<'a> {
    pub spans: Vec<StyledSpan<'a>>,
}

impl<'a> Styled<'a> {
    pub fn new() -> Self {
        Self {
            spans: Vec::default(),
        }
    }

    pub fn add_span<I: Into<StyledSpan<'a>>>(&mut self, span: I) -> &mut Self {
        self.spans.push(span.into());
        self
    }

    pub fn with_span<I: Into<StyledSpan<'a>>>(mut self, span: I) -> Self {
        self.spans.push(span.into());
        self
    }
}

impl<'a> From<&'a str> for Styled<'a> {
    fn from(text: &'a str) -> Self {
        Styled {
            spans: vec![StyledSpan {
                text: text.into(),
                style: TerminalCellStyle::default(),
            }],
        }
    }
}

impl<'a> From<String> for Styled<'a> {
    fn from(text: String) -> Self {
        Styled {
            spans: vec![StyledSpan {
                text: text.into(),
                style: TerminalCellStyle::default(),
            }],
        }
    }
}

impl<'a> From<Vec<StyledSpan<'a>>> for Styled<'a> {
    fn from(spans: Vec<StyledSpan<'a>>) -> Self {
        Styled { spans }
    }
}

#[derive(Clone, Debug)]
pub struct TextWidget<'a> {
    text: Styled<'a>,
    computed_size: Option<TerminalSize>,
    computed_line_lengths: Vec<u16>,
}

impl<'a> TextWidget<'a> {
    pub fn new<S: Into<Styled<'a>>>(text: S) -> Self {
        Self {
            text: text.into(),
            computed_size: None,
            computed_line_lengths: Vec::new(),
        }
    }
}

impl<'a> Widget for TextWidget<'a> {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize {
        let max_width = constraints.width.max;

        let mut width_bound = 0;
        let mut current_column = 0;

        assert!(max_width.map_or(true, |max| max > 0));

        for span in self.text.spans.iter() {
            for _ch in span.text.chars() {
                if max_width.map(|max| current_column >= max).unwrap_or(false) {
                    self.computed_line_lengths.push(current_column - 1);
                    current_column = 0;
                }

                current_column += 1;
                width_bound = width_bound.max(current_column);
            }
        }

        if current_column > 0 {
            self.computed_line_lengths.push(current_column);
        }

        let bounds = apply_constraints(
            constraints,
            width_bound,
            self.computed_line_lengths.len() as u16,
        );

        self.computed_size = Some(bounds);
        bounds
    }

    fn render<'buf>(&mut self, mut frame: Frame<'buf>) {
        let mut pos = TerminalPos::default();
        let mut line_index = 0;
        for span in self.text.spans.iter() {
            for ch in span.text.chars() {
                frame.put(
                    pos,
                    TerminalCell {
                        style: span.style,
                        character: Some(ch),
                    },
                );
                if pos.column == self.computed_line_lengths[line_index] {
                    pos.column = 0;
                    pos.row += 1;
                    line_index += 1;
                } else {
                    pos.column += 1;
                }
            }
        }
    }
}

pub fn horizontal<'a, A: Widget + 'a, B: Widget + 'a>(
    first: A,
    last: B,
) -> WrappedListWidget<Box<dyn Widget + 'a>> {
    WrappedListWidget::new(vec![Box::new(first), Box::new(last)])
}

#[derive(Clone, Debug, Default)]
pub struct WrappedListWidget<W> {
    children: Vec<W>,
    computed_child_sizes: Vec<TerminalSize>,
    computed_size: Option<TerminalSize>,
}

impl<W> WrappedListWidget<W> {
    pub fn new(children: Vec<W>) -> Self {
        Self {
            children,
            computed_child_sizes: Vec::default(),
            computed_size: Option::default(),
        }
    }
}

impl<W: Widget> Widget for WrappedListWidget<W> {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize {
        let mut current_pos = TerminalPos::default();
        let mut max_row_height = 0;

        let mut max_width = 0;
        let mut max_height = 0;

        for child in self.children.iter_mut() {
            let child_size = child.layout(constraints);
            self.computed_child_sizes.push(child_size);

            if constraints
                .width
                .overflows(child_size.width + current_pos.column)
            {
                current_pos.column = 0;
                current_pos.row += max_row_height;
                max_row_height = 0;
            }

            max_width = max_width.max(current_pos.column + child_size.width);
            max_height = max_height.max(current_pos.row + child_size.height);

            current_pos.column += child_size.width;
            max_row_height = max_row_height.max(child_size.height);
        }

        let bounds = apply_constraints(constraints, max_width, max_height);
        self.computed_size = Some(bounds);
        bounds
    }

    fn render<'buf>(&mut self, mut frame: Frame<'buf>) {
        let mut current_pos = TerminalPos::default();
        let mut max_row_height = 0;

        for (child, size) in self
            .children
            .iter_mut()
            .zip(self.computed_child_sizes.iter())
        {
            if current_pos.column + size.width > self.computed_size.unwrap().width {
                current_pos.column = 0;
                current_pos.row += max_row_height;
                max_row_height = 0;
            }

            frame.render_widget(
                TerminalRect {
                    start: current_pos,
                    end: current_pos + size,
                },
                child,
            );

            current_pos.column += size.width;
            max_row_height = max_row_height.max(size.height);
        }
    }
}

#[derive(Clone, Debug)]
pub struct VerticalListWidget<W> {
    children: Vec<W>,
    computed_child_sizes: Vec<TerminalSize>,
    computed_size: Option<TerminalSize>,
}

impl<W> VerticalListWidget<W> {
    pub fn new(children: Vec<W>) -> Self {
        Self {
            children,
            computed_child_sizes: Vec::default(),
            computed_size: Option::default(),
        }
    }
}

impl<W: Widget> Widget for VerticalListWidget<W> {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize {
        let mut total_height = 0;
        let mut max_width = 0;
        for child in self.children.iter_mut().rev() {
            // don't layout children we can't see!
            if constraints.height.overflows(total_height) {
                break;
            }

            let child_size = child.layout(BoxConstraints {
                height: AxisConstraint::unconstrained(),
                ..constraints
            });
            total_height += child_size.height;
            max_width = max_width.max(child_size.width);
            self.computed_child_sizes.push(child_size);
        }

        let bounds = apply_constraints(constraints, max_width, total_height);
        self.computed_size = Some(bounds);
        bounds
    }

    fn render<'buf>(&mut self, mut frame: Frame<'buf>) {
        let mut current_base = 0;
        for (child, &child_size) in Iterator::zip(
            self.children.iter_mut().rev(),
            self.computed_child_sizes.iter(),
        )
        .rev()
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

            frame.render_widget(foo, child);
            current_base += child_size.height;
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

    fn get_display_tag(&self, connection_id: u64) -> Cow<str> {
        match self.client.connection_info(connection_id) {
            Some(info) => match &info.username {
                Some(username) => username.as_str().into(),
                None => Cow::Owned(format!("?{}", connection_id)),
            },
            None => Cow::Owned(format!("!{}", connection_id)),
        }
    }

    fn add_connection_spans<'a>(&'a self, connection_id: u64, text: &mut Styled<'a>) {
        let (marker, color) = match connection_id == self.client.connection_id().unwrap() {
            true => ("~", Color::Cyan),
            false => ("", Color::Green),
        };

        text.add_span("<")
            .add_span(StyledSpan {
                text: marker.into(),
                style: TerminalCellStyle::default().with_fg_color(color),
            })
            .add_span(StyledSpan {
                text: self.get_display_tag(connection_id),
                style: TerminalCellStyle::default().with_fg_color(color),
            })
            .add_span(">");
    }

    fn build_chat_line_widget<'a>(&'a self, line: &'a ChatLine) -> impl Widget + 'a {
        let mut text = Styled::new();
        match line {
            ChatLine::Text { peer_id, message } => {
                self.add_connection_spans(*peer_id, &mut text);
                text.add_span(": ");
                text.add_span(message.as_str());
            }

            ChatLine::ConnectInfo {
                connection_id,
                peer_ids,
            } => {
                text.add_span(StyledSpan {
                    text: "---".into(),
                    style: TerminalCellStyle::default().with_fg_color(Color::Blue),
                });

                text.add_span(" connected as ");
                self.add_connection_spans(*connection_id, &mut text);

                if peer_ids.len() > 0 {
                    text.add_span(" to ");
                    self.add_connection_spans(peer_ids[0], &mut text);

                    for &peer_id in &peer_ids[1..] {
                        text.add_span(", ");
                        self.add_connection_spans(peer_id, &mut text);
                    }
                }

                text.add_span(StyledSpan {
                    text: " ---".into(),
                    style: TerminalCellStyle::default().with_fg_color(Color::Blue),
                });
            }

            ChatLine::Connected { peer_id } => {
                text.add_span(StyledSpan {
                    text: ">> ".into(),
                    style: TerminalCellStyle::default().with_fg_color(Color::Blue),
                });
                self.add_connection_spans(*peer_id, &mut text);
                text.add_span(" connected");
            }

            ChatLine::Disconnected { peer_id } => {
                text.add_span(StyledSpan {
                    text: "<< ".into(),
                    style: TerminalCellStyle::default().with_fg_color(Color::Red),
                });
                self.add_connection_spans(*peer_id, &mut text);
                text.add_span(" disconnected");
            }
        }

        TextWidget::new(text)
    }

    fn build_widget_tree(&self) -> impl Widget + '_ {
        let chat_lines = self
            .chat_lines
            .iter()
            .map(|line| self.build_chat_line_widget(line))
            .collect();
        VerticalListWidget::new(chat_lines)
    }

    fn redraw_message_ui<B: TerminalBackend>(&self, terminal: &mut B) -> ClientResult<()> {
        let mut widget = self.build_widget_tree();

        widget.layout(BoxConstraints {
            width: AxisConstraint::bounded_maximum(terminal.size()?.width),
            height: AxisConstraint::bounded_maximum(terminal.size()?.height),
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
                // modifiers: KeyModifiers { control: true, .. },
            }) => {
                let line = std::mem::replace(&mut self.current_message_text, String::new());
                let self_id = self.client.connection_id().unwrap();
                let cmd = self.command_tx.clone();
                self.client
                    .spawn_task(|task| net_handlers::send_peer_message(task, cmd, self_id, line));
            }

            // TerminalEvent::Key(TerminalKeyEvent {
            //     kind: KeyEventKind::Enter,
            //     ..
            // }) => {
            //     self.current_message_text.push('\n');
            // }

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

    let (mut terminal, events) = backend::CrosstermBackend::new(std::io::stdout())?;
    app.run(&mut terminal, events, username).await?;

    Ok(())
}
