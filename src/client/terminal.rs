#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalPos {
    pub column: u16,
    pub row: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct TerminalRect {
    pub start: TerminalPos,
    pub end: TerminalPos,
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
