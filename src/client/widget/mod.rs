use super::terminal::*;

mod list;
mod text;

pub mod widgets {
    pub use super::list::{VerticalListWidget, WrappedListWidget};
    pub use super::text::TextWidget;
}

pub mod style {
    pub use super::text::{StyledSegment, StyledText};
}

pub trait Widget {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize;
    fn render<'buf>(&self, frame: Frame<'buf>);
}

impl<'a> Widget for Box<dyn Widget + 'a> {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize {
        Widget::layout(&mut **self, constraints)
    }

    fn render<'buf>(&self, frame: Frame<'buf>) {
        Widget::render(&**self, frame)
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
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

pub fn apply_axis_constraint(constraint: AxisConstraint, value: u16) -> u16 {
    let value = constraint.max.map(|max| value.min(max)).unwrap_or(value);
    let value = constraint.min.map(|min| value.max(min)).unwrap_or(value);
    value
}

pub fn apply_constraints(constraints: BoxConstraints, width: u16, height: u16) -> TerminalSize {
    TerminalSize {
        height: apply_axis_constraint(constraints.height, height),
        width: apply_axis_constraint(constraints.width, width),
    }
}
