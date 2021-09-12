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
    pub min: TerminalScalar,
    pub max: Option<TerminalScalar>,
}

impl AxisConstraint {
    pub fn unconstrained() -> AxisConstraint {
        Self { min: 0, max: None }
    }

    pub fn exactly(value: TerminalScalar) -> AxisConstraint {
        Self {
            min: value,
            max: Some(value),
        }
    }

    pub fn bounded_maximum(value: TerminalScalar) -> AxisConstraint {
        Self {
            min: 0,
            max: Some(value),
        }
    }

    pub fn bounded_minimum(value: TerminalScalar) -> AxisConstraint {
        Self {
            min: value,
            max: None,
        }
    }

    pub fn bounded(min: TerminalScalar, max: TerminalScalar) -> AxisConstraint {
        Self {
            min,
            max: Some(max),
        }
    }
}

impl AxisConstraint {
    pub fn overflows(&self, value: TerminalScalar) -> bool {
        match self.max {
            Some(max) => value > max,
            None => false,
        }
    }

    pub fn underflows(&self, value: TerminalScalar) -> bool {
        value < self.min
    }

    pub fn overflows_by(&self, value: TerminalScalar) -> TerminalScalar {
        match self.max {
            Some(max) if value > max => value - max,
            _ => 0,
        }
    }

    pub fn unbind_minimum(&self) -> AxisConstraint {
        AxisConstraint { min: 0, ..*self }
    }

    pub fn unbind_maximum(&self) -> AxisConstraint {
        AxisConstraint { max: None, ..*self }
    }
}

pub fn apply_max_constraint(constraint: AxisConstraint, value: TerminalScalar) -> TerminalScalar {
    constraint.max.map(|max| value.min(max)).unwrap_or(value)
}

pub fn apply_min_constraint(constraint: AxisConstraint, value: TerminalScalar) -> TerminalScalar {
    constraint.min.max(value)
}

pub fn apply_axis_constraint(constraint: AxisConstraint, value: TerminalScalar) -> TerminalScalar {
    apply_min_constraint(constraint, apply_max_constraint(constraint, value))
}

pub fn apply_constraints(
    constraints: BoxConstraints,
    width: TerminalScalar,
    height: TerminalScalar,
) -> TerminalSize {
    TerminalSize {
        height: apply_axis_constraint(constraints.height, height),
        width: apply_axis_constraint(constraints.width, width),
    }
}

pub struct VerticalSplitView<A, B> {
    elastic: A,
    fill: B,

    computed_elastic_size: TerminalSize,
    computed_fill_size: TerminalSize,
    computed_should_draw_fill: bool,
}

impl<A, B> VerticalSplitView<A, B> {
    pub fn new(elastic: A, fill: B) -> Self {
        Self {
            elastic,
            fill,
            computed_elastic_size: TerminalSize::default(),
            computed_fill_size: TerminalSize::default(),
            computed_should_draw_fill: true,
        }
    }
}

impl<A: Widget, B: Widget> Widget for VerticalSplitView<A, B> {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize {
        // we unbind the minimum for the elastic widget, so that it can lay out
        // at its leisure, because we can fill whatever space would be lost in
        // this unbind with space from the filler widget later.
        let elastic_size = self.elastic.layout(BoxConstraints {
            height: constraints.height.unbind_minimum(),
            ..constraints
        });
        self.computed_elastic_size = elastic_size;

        let fill_min_bound = constraints.height.min.saturating_sub(elastic_size.height);
        let fill_max_bound = match constraints
            .height
            .max
            .map(|max| max.saturating_sub(elastic_size.height))
        {
            None => Some(None),
            Some(0) => None,
            Some(max) => Some(Some(max)),
        };

        let fill_size = if let Some(fill_max_bound) = fill_max_bound {
            self.fill.layout(BoxConstraints {
                height: AxisConstraint {
                    min: fill_min_bound,
                    max: fill_max_bound,
                },
                ..constraints
            })
        } else {
            self.computed_should_draw_fill = false;
            TerminalSize::default()
        };
        self.computed_fill_size = fill_size;

        apply_constraints(
            constraints,
            elastic_size.width.max(fill_size.width),
            elastic_size.height + fill_size.height,
        )
    }

    fn render<'buf>(&self, mut frame: Frame<'buf>) {
        frame.render_widget(
            TerminalRect::from_size(self.computed_elastic_size)
                .offset_rows(self.computed_fill_size.height),
            &self.elastic,
        );

        if self.computed_should_draw_fill {
            frame.render_widget(
                TerminalRect::from_size(self.computed_elastic_size),
                &self.fill,
            );
        }
    }
}
