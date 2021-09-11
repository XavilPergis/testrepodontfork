use std::{io::Write, pin::Pin, sync::Arc};

use crossterm::{
    event::{Event, EventStream},
    terminal::ClearType,
    QueueableCommand,
};
use futures::{Future, StreamExt};
use std::sync::RwLock;

use crate::client::{
    KeyEventKind, KeyModifiers, MouseButton, MouseEventKind, TerminalKeyEvent, TerminalMouseEvent,
};

use super::{ClientResult, Color, TerminalCell, TerminalEvent, TerminalPos, TerminalSize};

pub trait TerminalBackend {
    fn redraw(&mut self, buffer: &[TerminalCell]) -> ClientResult<()>;
    fn size(&self) -> ClientResult<TerminalSize>;
}

pub type DynamicFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>;

pub type EventPollResult = std::io::Result<Option<TerminalEvent>>;

pub trait EventsBackend: Send + Sync + 'static {
    fn poll_event(&mut self) -> DynamicFuture<EventPollResult>;
}

pub struct CrosstermEventsBackend {
    event_stream: EventStream,
    current_size: Arc<RwLock<TerminalSize>>,
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
        Event::Resize(rows, columns) => TerminalEvent::Resize(TerminalSize {
            height: rows,
            width: columns,
        }),
    }
}

impl EventsBackend for CrosstermEventsBackend {
    fn poll_event(&mut self) -> DynamicFuture<EventPollResult> {
        Box::pin(async move {
            match self.event_stream.next().await {
                Some(Ok(event)) => {
                    match event {
                        Event::Resize(width, height) => {
                            *self.current_size.write().unwrap() = TerminalSize { width, height };
                        }
                        _ => {}
                    }
                    Ok(Some(translate_crossterm_event(event)))
                }
                Some(Err(err)) => Err(err),
                None => Ok(None),
            }
        })
    }
}

pub struct CrosstermBackend<W: Write + QueueableCommand> {
    writer: W,
    current_size: Arc<RwLock<TerminalSize>>,
    prev_size: TerminalSize,
    prev_buffer: Box<[TerminalCell]>,
}

impl<W: Write + QueueableCommand> CrosstermBackend<W> {
    pub fn new(mut writer: W) -> ClientResult<(Self, CrosstermEventsBackend)> {
        crossterm::terminal::enable_raw_mode()?;

        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crossterm::terminal::disable_raw_mode().unwrap();
            std::io::stdout()
                .queue(crossterm::terminal::LeaveAlternateScreen)
                .unwrap();
            prev_hook(info);
        }));

        let terminal_size = Arc::new(RwLock::new(get_terminal_size()?));

        writer.queue(crossterm::terminal::EnterAlternateScreen)?;
        let term = Self {
            writer,
            current_size: Arc::clone(&terminal_size),
            prev_size: TerminalSize::default(),
            prev_buffer: Vec::default().into_boxed_slice(),
        };

        let events = CrosstermEventsBackend {
            current_size: terminal_size,
            event_stream: EventStream::new(),
        };

        Ok((term, events))
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

fn get_terminal_size() -> ClientResult<TerminalSize> {
    let (columns, rows) = crossterm::terminal::size()?;
    Ok(TerminalSize {
        width: columns,
        height: rows,
    })
}

impl<W: Write + QueueableCommand> TerminalBackend for CrosstermBackend<W> {
    fn size(&self) -> ClientResult<TerminalSize> {
        Ok(*self.current_size.read().unwrap())
    }

    fn redraw(&mut self, buffer: &[TerminalCell]) -> ClientResult<()> {
        let mut prev_fg_color = Color::default();
        let mut prev_bg_color = Color::default();

        let size = self.size()?;

        // full reset if the screen size changed
        if size != self.prev_size {
            self.prev_size = size;
            self.writer
                .queue(crossterm::terminal::Clear(ClearType::All))?;
            self.prev_buffer = vec![
                TerminalCell::default();
                self.prev_size.width as usize * self.prev_size.height as usize
            ]
            .into_boxed_slice();
        }

        self.writer.queue(crossterm::cursor::Hide)?;
        for row in 0..size.height {
            self.writer.queue(crossterm::cursor::MoveTo(0, row))?;
            for column in 0..size.width {
                let buf_index = row as usize * size.width as usize + column as usize;
                let cell = buffer[buf_index];
                // let prev_cell = self.prev_buffer[buf_index];

                // if cell == prev_cell {
                //     // continue;
                // } else {
                //     self.prev_buffer[buf_index] = cell;
                //     self.writer
                //         .queue(crossterm::style::SetBackgroundColor(translate_color(
                //             Color::DarkGrey,
                //         )))?;
                //     // prev_bg_color = Color::Yellow;
                // }

                if cell.style.background_color != prev_bg_color {
                    self.writer
                        .queue(crossterm::style::SetBackgroundColor(translate_color(
                            cell.style.background_color,
                        )))?;
                    prev_bg_color = cell.style.background_color;
                }

                if cell.style.foreground_color != prev_fg_color {
                    self.writer
                        .queue(crossterm::style::SetForegroundColor(translate_color(
                            cell.style.foreground_color,
                        )))?;
                    prev_fg_color = cell.style.foreground_color;
                }
                write!(self.writer, "{}", cell.character.unwrap_or(' '))?;
                self.writer
                    .queue(crossterm::style::SetBackgroundColor(translate_color(
                        Color::Reset,
                    )))?;
            }
        }
        self.writer.queue(crossterm::cursor::Show)?;

        self.writer.flush()?;

        Ok(())
    }
}
