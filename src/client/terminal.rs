#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalPos {
    pub column: u16,
    pub row: u16,
}

impl std::ops::Add for TerminalPos {
    type Output = TerminalPos;

    fn add(self, rhs: Self) -> Self::Output {
        TerminalPos {
            row: self.row + rhs.row,
            column: self.column + rhs.column,
        }
    }
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
