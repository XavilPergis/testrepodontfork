use std::borrow::Cow;

use crate::client::{
    terminal::{TerminalPos, TerminalSize},
    widget::apply_constraints,
};

use super::{BoxConstraints, Frame, TerminalCell, TerminalCellStyle, Widget};

#[derive(Clone, Debug)]
pub struct StyledSegment<'a> {
    pub text: Cow<'a, str>,
    pub style: TerminalCellStyle,
}

impl<'a> From<&'a str> for StyledSegment<'a> {
    fn from(text: &'a str) -> Self {
        Self {
            text: text.into(),
            style: TerminalCellStyle::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StyledText<'a> {
    pub spans: Vec<StyledSegment<'a>>,
}

impl<'a> StyledText<'a> {
    pub fn new() -> Self {
        Self {
            spans: Vec::default(),
        }
    }

    pub fn add_span<I: Into<StyledSegment<'a>>>(&mut self, span: I) -> &mut Self {
        self.spans.push(span.into());
        self
    }

    pub fn with_span<I: Into<StyledSegment<'a>>>(mut self, span: I) -> Self {
        self.spans.push(span.into());
        self
    }
}

impl<'a> From<&'a str> for StyledText<'a> {
    fn from(text: &'a str) -> Self {
        StyledText {
            spans: vec![StyledSegment {
                text: text.into(),
                style: TerminalCellStyle::default(),
            }],
        }
    }
}

impl<'a> From<String> for StyledText<'a> {
    fn from(text: String) -> Self {
        StyledText {
            spans: vec![StyledSegment {
                text: text.into(),
                style: TerminalCellStyle::default(),
            }],
        }
    }
}

impl<'a> From<Vec<StyledSegment<'a>>> for StyledText<'a> {
    fn from(spans: Vec<StyledSegment<'a>>) -> Self {
        StyledText { spans }
    }
}

#[derive(Clone, Debug)]
pub struct TextWidget<'a> {
    text: StyledText<'a>,
    computed_size: Option<TerminalSize>,
    computed_line_lengths: Vec<u16>,
}

impl<'a> TextWidget<'a> {
    pub fn new<S: Into<StyledText<'a>>>(text: S) -> Self {
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

    fn render<'buf>(&self, mut frame: Frame<'buf>) {
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
