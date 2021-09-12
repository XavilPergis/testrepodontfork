use super::widget::Widget;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalPos {
    pub column: u16,
    pub row: u16,
}

macro_rules! gen_pos_add_impls {
    ($([$impl_type:ty, $rhs_type:ty, $row:ident, $column:ident])+) => {
        $(impl std::ops::Add<$rhs_type> for $impl_type {
            type Output = TerminalPos;

            fn add(self, rhs: $rhs_type) -> Self::Output {
                TerminalPos {
                    row: self.row + rhs.$row,
                    column: self.column + rhs.$column,
                }
            }
        })*
    };
}

gen_pos_add_impls! {
    [TerminalPos, TerminalPos, row, column]
    [TerminalPos, &TerminalPos, row, column]
    [&TerminalPos, TerminalPos, row, column]
    [&TerminalPos, &TerminalPos, row, column]
    [TerminalPos, TerminalSize, height, width]
    [TerminalPos, &TerminalSize, height, width]
    [&TerminalPos, TerminalSize, height, width]
    [&TerminalPos, &TerminalSize, height, width]
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalRect {
    pub start: TerminalPos,
    pub end: TerminalPos,
}

impl TerminalRect {
    pub fn width(&self) -> u16 {
        self.end.column - self.start.column
    }

    pub fn height(&self) -> u16 {
        self.end.row - self.start.row
    }
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

impl Default for Color {
    fn default() -> Self {
        Color::Reset
    }
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

    pub fn render_widget<W: Widget>(&mut self, area: TerminalRect, widget: &W) {
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
