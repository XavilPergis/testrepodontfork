use crate::client::{
    last_of,
    terminal::{TerminalPos, TerminalRect, TerminalScalar, TerminalSize},
};

use super::{
    apply_constraints, apply_max_constraint, AxisConstraint, BoxConstraints, Frame, Widget,
};

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

    fn render<'buf>(&self, mut frame: Frame<'buf>) {
        let mut current_pos = TerminalPos::default();
        let mut max_row_height = 0;

        for (child, size) in self.children.iter().zip(self.computed_child_sizes.iter()) {
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

/// Widget that keeps the tail end of its widget list in view.
#[derive(Clone, Debug)]
pub struct VerticalListWidget<W> {
    children: Vec<W>,
    computed_base_height: TerminalScalar,
    computed_child_sizes: Vec<TerminalSize>,
    computed_clamped_size: TerminalSize,
}

impl<W> VerticalListWidget<W> {
    pub fn new(children: Vec<W>) -> Self {
        Self {
            children,
            computed_base_height: 0,
            computed_child_sizes: Vec::default(),
            computed_clamped_size: TerminalSize::default(),
        }
    }
}

impl<W: Widget> Widget for VerticalListWidget<W> {
    fn layout(&mut self, constraints: BoxConstraints) -> TerminalSize {
        let mut total_height = 0;
        let mut max_width = 0;

        for child in self.children.iter_mut().rev() {
            let child_size = child.layout(BoxConstraints {
                height: AxisConstraint::unconstrained(),
                ..constraints
            });

            total_height += child_size.height;
            max_width = max_width.max(child_size.width);
            self.computed_child_sizes.push(child_size);

            if constraints.height.overflows(total_height) {
                break;
            }
        }

        self.computed_base_height = -constraints.height.overflows_by(total_height);
        apply_constraints(constraints, max_width, total_height)
    }

    fn render<'buf>(&self, mut frame: Frame<'buf>) {
        let mut current_height = self.computed_base_height;
        let children = last_of(self.computed_child_sizes.len(), &self.children);

        for (child, &size) in children.iter().zip(self.computed_child_sizes.iter().rev()) {
            frame.render_widget(
                TerminalRect::from_size(size).offset_rows(current_height),
                child,
            );
            current_height += size.height;
        }
    }
}
