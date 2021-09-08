use std::{io::Write, pin::Pin};

use crossterm::{
    event::{Event, EventStream},
    terminal::ClearType,
    QueueableCommand,
};
use futures::{Future, StreamExt};

use crate::client::{
    KeyEventKind, KeyModifiers, MouseButton, MouseEventKind, TerminalKeyEvent, TerminalMouseEvent,
};

use super::{ClientResult, Color, TerminalEvent, TerminalPos, TerminalSize};

pub trait TerminalBackend {
    type Writer: Write;
    fn writer(&mut self) -> &mut Self::Writer;

    fn submit_set_foreground_color(&mut self, color: Color) -> ClientResult<()>;
    fn submit_set_background_color(&mut self, color: Color) -> ClientResult<()>;
    fn submit_clear(&mut self) -> ClientResult<()>;
    fn submit_goto(&mut self, pos: TerminalPos) -> ClientResult<()>;

    fn flush(&mut self) -> ClientResult<()>;

    fn size(&self) -> ClientResult<TerminalSize>;
}

pub type DynamicFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>;

pub type EventPollResult = std::io::Result<Option<TerminalEvent>>;

pub trait EventsBackend: Send + Sync + 'static {
    fn poll_event(&mut self) -> DynamicFuture<EventPollResult>;
}

pub struct CrosstermEventsBackend {
    event_stream: EventStream,
}

impl CrosstermEventsBackend {
    pub fn new() -> Self {
        Self {
            event_stream: EventStream::new(),
        }
    }
}

fn translate_crossterm_event(event: Event) -> TerminalEvent {
    fn translate_modifiers(modifiers: crossterm::event::KeyModifiers) -> KeyModifiers {
        KeyModifiers {
            control: modifiers.contains(crossterm::event::KeyModifiers::CONTROL),
            shift: modifiers.contains(crossterm::event::KeyModifiers::SHIFT),
            alt: modifiers.contains(crossterm::event::KeyModifiers::ALT),
        }
    }

    fn translate_key_event(event: crossterm::event::KeyEvent) -> TerminalKeyEvent {
        TerminalKeyEvent {
            kind: match event.code {
                crossterm::event::KeyCode::Backspace => KeyEventKind::Backspace,
                crossterm::event::KeyCode::Enter => KeyEventKind::Enter,
                crossterm::event::KeyCode::Left => KeyEventKind::Left,
                crossterm::event::KeyCode::Right => KeyEventKind::Right,
                crossterm::event::KeyCode::Up => KeyEventKind::Up,
                crossterm::event::KeyCode::Down => KeyEventKind::Down,
                crossterm::event::KeyCode::Home => KeyEventKind::Home,
                crossterm::event::KeyCode::End => KeyEventKind::End,
                crossterm::event::KeyCode::PageUp => KeyEventKind::PageUp,
                crossterm::event::KeyCode::PageDown => KeyEventKind::PageDown,
                crossterm::event::KeyCode::Tab => KeyEventKind::Tab,
                crossterm::event::KeyCode::BackTab => KeyEventKind::BackTab,
                crossterm::event::KeyCode::Delete => KeyEventKind::Delete,
                crossterm::event::KeyCode::Insert => KeyEventKind::Insert,
                crossterm::event::KeyCode::F(ch) => KeyEventKind::Function(ch),
                crossterm::event::KeyCode::Char(ch) => KeyEventKind::Char(ch),
                crossterm::event::KeyCode::Null => KeyEventKind::Null,
                crossterm::event::KeyCode::Esc => KeyEventKind::Esc,
            },
            modifiers: translate_modifiers(event.modifiers),
        }
    }

    fn translate_mouse_button(button: crossterm::event::MouseButton) -> MouseButton {
        match button {
            crossterm::event::MouseButton::Left => MouseButton::Left,
            crossterm::event::MouseButton::Right => MouseButton::Right,
            crossterm::event::MouseButton::Middle => MouseButton::Middle,
        }
    }

    fn translate_mouse_event(event: crossterm::event::MouseEvent) -> TerminalMouseEvent {
        TerminalMouseEvent {
            kind: match event.kind {
                crossterm::event::MouseEventKind::Down(button) => {
                    MouseEventKind::Down(translate_mouse_button(button))
                }
                crossterm::event::MouseEventKind::Up(button) => {
                    MouseEventKind::Up(translate_mouse_button(button))
                }
                crossterm::event::MouseEventKind::Drag(button) => {
                    MouseEventKind::Drag(translate_mouse_button(button))
                }
                crossterm::event::MouseEventKind::Moved => MouseEventKind::Moved,
                crossterm::event::MouseEventKind::ScrollDown => MouseEventKind::ScrollDown,
                crossterm::event::MouseEventKind::ScrollUp => MouseEventKind::ScrollUp,
            },
            location: TerminalPos {
                column: event.column,
                row: event.row,
            },
            modifiers: translate_modifiers(event.modifiers),
        }
    }

    match event {
        Event::Key(event) => TerminalEvent::Key(translate_key_event(event)),
        Event::Mouse(event) => TerminalEvent::Mouse(translate_mouse_event(event)),
        Event::Resize(rows, columns) => TerminalEvent::Resize(TerminalSize { rows, columns }),
    }
}

impl EventsBackend for CrosstermEventsBackend {
    fn poll_event(&mut self) -> DynamicFuture<EventPollResult> {
        Box::pin(async move {
            match self.event_stream.next().await {
                Some(Ok(event)) => Ok(Some(translate_crossterm_event(event))),
                Some(Err(err)) => Err(err),
                None => Ok(None),
            }
        })
    }
}

pub struct CrosstermBackend<W: Write + QueueableCommand> {
    writer: W,
}

impl<W: Write + QueueableCommand> CrosstermBackend<W> {
    pub fn new(mut writer: W) -> ClientResult<Self> {
        crossterm::terminal::enable_raw_mode()?;

        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crossterm::terminal::disable_raw_mode().unwrap();
            std::io::stdout()
                .queue(crossterm::terminal::LeaveAlternateScreen)
                .unwrap();
            prev_hook(info);
        }));

        writer.queue(crossterm::terminal::EnterAlternateScreen)?;
        Ok(Self { writer })
    }
}

impl<W: Write + QueueableCommand> Drop for CrosstermBackend<W> {
    fn drop(&mut self) {
        let _ = self.writer.queue(crossterm::terminal::LeaveAlternateScreen);
        // it's not a HUGE deal if we don't exit raw mode, its better to let
        // other things run their destructors and whatnot over screwing up the
        // terminal
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn translate_color(color: Color) -> crossterm::style::Color {
    match color {
        Color::Reset => crossterm::style::Color::Reset,
        Color::Black => crossterm::style::Color::Black,
        Color::DarkGrey => crossterm::style::Color::DarkGrey,
        Color::Red => crossterm::style::Color::Red,
        Color::DarkRed => crossterm::style::Color::DarkRed,
        Color::Green => crossterm::style::Color::Green,
        Color::DarkGreen => crossterm::style::Color::DarkGreen,
        Color::Yellow => crossterm::style::Color::Yellow,
        Color::DarkYellow => crossterm::style::Color::DarkYellow,
        Color::Blue => crossterm::style::Color::Blue,
        Color::DarkBlue => crossterm::style::Color::DarkBlue,
        Color::Magenta => crossterm::style::Color::Magenta,
        Color::DarkMagenta => crossterm::style::Color::DarkMagenta,
        Color::Cyan => crossterm::style::Color::Cyan,
        Color::DarkCyan => crossterm::style::Color::DarkCyan,
        Color::White => crossterm::style::Color::White,
        Color::Grey => crossterm::style::Color::Grey,
        Color::Rgb { r, g, b } => crossterm::style::Color::Rgb { r, g, b },
        Color::AnsiValue(val) => crossterm::style::Color::AnsiValue(val),
    }
}

impl<W: Write + QueueableCommand> TerminalBackend for CrosstermBackend<W> {
    type Writer = W;

    fn submit_set_foreground_color(&mut self, color: Color) -> ClientResult<()> {
        self.writer
            .queue(crossterm::style::SetForegroundColor(translate_color(color)))?;
        Ok(())
    }

    fn submit_set_background_color(&mut self, color: Color) -> ClientResult<()> {
        self.writer
            .queue(crossterm::style::SetForegroundColor(translate_color(color)))?;
        Ok(())
    }

    fn submit_clear(&mut self) -> ClientResult<()> {
        self.writer
            .queue(crossterm::terminal::Clear(ClearType::All))?;
        Ok(())
    }

    fn submit_goto(&mut self, pos: TerminalPos) -> ClientResult<()> {
        self.writer
            .queue(crossterm::cursor::MoveTo(pos.column, pos.row))?;
        Ok(())
    }

    fn flush(&mut self) -> ClientResult<()> {
        self.writer.flush()?;
        Ok(())
    }

    fn writer(&mut self) -> &mut Self::Writer {
        &mut self.writer
    }

    fn size(&self) -> ClientResult<TerminalSize> {
        let (columns, rows) = crossterm::terminal::size()?;
        Ok(TerminalSize { columns, rows })
    }
}
