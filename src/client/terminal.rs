use super::widget::Widget;

pub type TerminalScalar = i32;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalPos {
    pub column: TerminalScalar,
    pub row: TerminalScalar,
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
    pub width: TerminalScalar,
    pub height: TerminalScalar,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalRect {
    pub start: TerminalPos,
    pub end: TerminalPos,
}

impl TerminalRect {
    pub fn from_size(size: TerminalSize) -> Self {
        Self {
            start: TerminalPos::default(),
            end: TerminalPos {
                column: size.width,
                row: size.height,
            },
        }
    }

    pub fn offset_rows(&self, rows: TerminalScalar) -> Self {
        Self {
            start: self.start
                + TerminalSize {
                    width: 0,
                    height: rows,
                },
            end: self.end
                + TerminalSize {
                    width: 0,
                    height: rows,
                },
        }
    }

    pub fn offset_columns(&self, columns: TerminalScalar) -> Self {
        Self {
            start: self.start
                + TerminalSize {
                    width: columns,
                    height: 0,
                },
            end: self.end
                + TerminalSize {
                    width: columns,
                    height: 0,
                },
        }
    }

    pub fn width(&self) -> TerminalScalar {
        self.end.column - self.start.column
    }

    pub fn height(&self) -> TerminalScalar {
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

#[derive(Debug)]
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

    fn make_buffer_index(&self, row: usize, col: usize) -> usize {
        row * (self.root_size.width as usize) + col
    }

    fn buffer_index(&self, offset: TerminalPos) -> Option<usize> {
        let pos_root = self.frame_bounds.start + offset;

        if offset.column < 0
            || offset.column > self.width()
            || offset.row < 0
            || offset.row > self.height()
        {
            return None;
        }

        if pos_root.column < 0
            || pos_root.column >= self.root_size.width
            || pos_root.row < 0
            || pos_root.row >= self.root_size.height
        {
            return None;
        }

        Some(self.make_buffer_index(pos_root.row as usize, pos_root.column as usize))
    }

    pub fn put(&mut self, offset: TerminalPos, cell: TerminalCell) {
        if let Some(index) = self.buffer_index(offset) {
            self.root_buffer[index] = cell;
        }
    }

    pub fn get(&self, offset: TerminalPos) -> Option<&TerminalCell> {
        self.buffer_index(offset)
            .map(|index| &self.root_buffer[index])
    }

    pub fn width(&self) -> TerminalScalar {
        self.frame_bounds.width()
    }

    pub fn height(&self) -> TerminalScalar {
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
