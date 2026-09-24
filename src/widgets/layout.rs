use rich_rs::{Console, ConsoleOptions, Segment, Segments};
use std::sync::atomic::{AtomicUsize, Ordering};
use textual_macros::widget;

use crate::compose::ComposeResult;
use crate::css;
use crate::debug::{DebugLayout, debug_input, debug_layout};
use crate::event::{
    Action, BlurEvent, Event, EventCtx, FocusEvent, MouseEnterEvent, MouseLeaveEvent,
};
use crate::node_id::NodeId;
use crate::num::Cast;

use super::{
    LayoutConstraints, NodeSeed, Widget,
    helpers::{
        adjust_line_length_no_bg, apply_debug_box, apply_margin, clamp_with_constraints,
        constraints_from_style, margin_from_style, pad_lines_to_width,
    },
};
use crate::style::{BoxSizing, Dock as StyleDock, Margin, Scalar};

#[widget(Interactive, StyleIdentity)]
pub struct Row {
    children: Vec<Box<dyn Widget>>,
    children_extracted: bool,
    align: RowAlign,
    last_layout_width: u16,
    seed: NodeSeed,
    /// Index of the currently focused child (non-tree mode only).
    focused_child: Option<usize>,
    /// Index of the currently hovered child (non-tree mode only).
    hovered_child: Option<usize>,
    /// (index into `children`, `css_id`, classes) recorded by `with_compose` so
    /// `.with_id()`/`.with_classes()` metadata on declared children reaches the
    /// mounted node (mirrors `Container::with_compose`).
    child_decl_meta: Vec<crate::widgets::ChildDeclMeta>,
    /// (index into `children`, sink) recorded by `with_compose` for decls bound
    /// via `HandleSlot::bind`.
    child_handle_sinks: Vec<(usize, crate::handle::HandleSink)>,
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}

impl Row {
    crate::seed_ident_methods!();

    #[must_use]
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            children_extracted: false,
            align: RowAlign::Top,
            last_layout_width: 0,
            seed: NodeSeed::default(),
            focused_child: None,
            hovered_child: None,
            child_decl_meta: Vec::new(),
            child_handle_sinks: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_child(mut self, child: impl Widget + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    /// Add multiple children from a `compose![]` result.
    ///
    /// Preserves each `ChildDecl`'s `id`/`classes` (so CSS id/class selectors
    /// match the mounted nodes) and any `handle_sink` bound via `HandleSlot::bind`,
    /// mirroring `Container::with_compose`.
    #[must_use]
    pub fn with_compose(mut self, children: ComposeResult) -> Self {
        for decl in children {
            let crate::compose::ChildDecl {
                builder,
                id,
                classes,
                handle_sink,
                ..
            } = decl;
            let crate::compose::WidgetBuilder::Ready(widget) = builder;
            let index = self.children.len();
            self.children.push(widget);
            if id.is_some() || !classes.is_empty() {
                self.child_decl_meta.push((index, id, classes));
            }
            if let Some(sink) = handle_sink {
                self.child_handle_sinks.push((index, sink));
            }
        }
        self
    }

    pub fn push(&mut self, child: impl Widget + 'static) {
        self.children.push(Box::new(child));
    }

    #[must_use]
    pub fn align(mut self, align: RowAlign) -> Self {
        self.align = align;
        self
    }

    /// Read-only access to the row's children.
    #[must_use]
    pub fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    /// Mutable access to the row's children.
    pub fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn is_tree_mode(&self) -> bool {
        self.children_extracted
    }

    fn child_at_x(&self, x: u16) -> Option<(usize, u16)> {
        let count = self.children.len();
        if count == 0 {
            return None;
        }
        let total_width = self.last_layout_width.max(x.saturating_add(1)).max(1) as usize;
        let base = total_width / count;
        let remainder = total_width % count;

        let mut cursor = 0usize;
        for idx in 0..count {
            let width = (base + usize::from(idx < remainder)).max(1);
            let end = cursor + width;
            let xu = x as usize;
            if xu < end {
                return Some((idx, (xu - cursor).to_u16_sat()));
            }
            cursor = end;
        }
        Some((count - 1, 0))
    }

    /// Cycle focus to the next/prev focusable child (non-tree mode).
    ///
    /// Dispatches `Event::Blur` to the previously focused child and
    /// `Event::Focus` to the next one, updating `self.focused_child`.
    fn cycle_focus(&mut self, action: Action) -> bool {
        let mut focusable: Vec<usize> = Vec::new();
        let mut current_pos: Option<usize> = None;
        for (idx, child) in self.children.iter().enumerate() {
            if child.focusable() {
                if self.focused_child == Some(idx) {
                    current_pos = Some(focusable.len());
                }
                focusable.push(idx);
            }
        }
        if focusable.is_empty() {
            return false;
        }
        let next_pos = match (action, current_pos) {
            (Action::FocusNext, Some(pos)) => (pos + 1) % focusable.len(),
            (Action::FocusPrev, Some(0) | None) => focusable.len() - 1,
            (Action::FocusPrev, Some(pos)) => pos - 1,
            (Action::FocusNext, None) => 0,
            _ => return false,
        };
        let next_idx = focusable[next_pos];
        if self.focused_child == Some(next_idx) {
            return false;
        }
        // Blur the previously focused child.
        if let Some(prev_idx) = self.focused_child {
            let blur = Event::Blur(BlurEvent {
                node: NodeId::default(),
            });
            let mut ectx = EventCtx::default();
            if let Some(child) = self.children.get_mut(prev_idx) {
                let mut ctx = crate::event::WidgetCtx::__from_dispatch(
                    crate::node_id::NodeId::default(),
                    &mut ectx,
                );
                child.on_event(&blur, &mut ctx);
            }
        }
        self.focused_child = Some(next_idx);
        // Focus the new child.
        let focus = Event::Focus(FocusEvent {
            node: NodeId::default(),
        });
        let mut ectx = EventCtx::default();
        if let Some(child) = self.children.get_mut(next_idx) {
            let mut ctx = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ectx,
            );
            child.on_event(&focus, &mut ctx);
        }
        true
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RowAlign {
    Top,
    Center,
    Bottom,
}

impl crate::widgets::Interactive for Row {
    fn on_mount(&mut self, ctx: &mut crate::event::WidgetCtx) {
        if !self.is_tree_mode() {
            for child in &mut self.children {
                child.on_mount(ctx);
            }
        }
    }

    fn on_unmount(&mut self) {
        if !self.is_tree_mode() {
            for child in &mut self.children {
                child.on_unmount();
            }
        }
    }

    fn on_tick(&mut self, tick: u64) {
        if !self.is_tree_mode() {
            for child in &mut self.children {
                child.on_tick(tick);
            }
        }
    }

    fn on_resize(&mut self, width: u16, height: u16) {
        self.last_layout_width = width;
        if !self.is_tree_mode() {
            for child in &mut self.children {
                child.on_resize(width, height);
            }
        }
    }

    fn on_layout(&mut self, width: u16, height: u16) {
        self.last_layout_width = width;
        if !self.is_tree_mode() {
            for child in &mut self.children {
                child.on_layout(width, height);
            }
        }
    }

    fn on_event_capture(&mut self, event: &Event, ctx: &mut crate::event::WidgetCtx) {
        if self.is_tree_mode() {
            return;
        }
        for child in &mut self.children {
            child.on_event_capture(event, ctx);
            if ctx.handled() {
                break;
            }
        }
    }

    fn on_event(&mut self, event: &Event, ctx: &mut crate::event::WidgetCtx) {
        if self.is_tree_mode() {
            return;
        }
        match event {
            Event::Action(Action::FocusNext | Action::FocusPrev) => {
                if let Event::Action(action) = event {
                    if self.cycle_focus(*action) {
                        ctx.request_repaint();
                        ctx.set_handled();
                    }
                }
                return;
            }
            Event::MouseDown(mouse) => {
                if let Some((idx, local_x)) = self.child_at_x(mouse.x) {
                    let child_event = Event::MouseDown(crate::event::MouseDownEvent {
                        target: NodeId::default(),
                        screen_x: mouse.screen_x,
                        screen_y: mouse.screen_y,
                        x: local_x,
                        y: mouse.y,
                    });
                    if let Some(child) = self.children.get_mut(idx) {
                        child.on_event(&child_event, ctx);
                    }
                }
                return;
            }
            Event::MouseUp(mouse) => {
                if let Some((idx, local_x)) = self.child_at_x(mouse.x) {
                    let child_event = Event::MouseUp(crate::event::MouseUpEvent {
                        target: Some(NodeId::default()),
                        screen_x: mouse.screen_x,
                        screen_y: mouse.screen_y,
                        x: local_x,
                        y: mouse.y,
                    });
                    if let Some(child) = self.children.get_mut(idx) {
                        child.on_event(&child_event, ctx);
                    }
                }
                return;
            }
            Event::MouseScroll(mouse) => {
                if let Some((idx, local_x)) = self.child_at_x(mouse.x) {
                    let child_event = Event::MouseScroll(crate::event::MouseScrollEvent {
                        target: Some(NodeId::default()),
                        screen_x: mouse.screen_x,
                        screen_y: mouse.screen_y,
                        x: local_x,
                        y: mouse.y,
                        delta_x: mouse.delta_x,
                        delta_y: mouse.delta_y,
                        modifiers: mouse.modifiers,
                    });
                    if let Some(child) = self.children.get_mut(idx) {
                        child.on_event(&child_event, ctx);
                    }
                }
                return;
            }
            _ => {}
        }
        for child in &mut self.children {
            child.on_event(event, ctx);
            if ctx.handled() {
                break;
            }
        }
    }

    fn on_mouse_move(&mut self, x: u16, y: u16) -> bool {
        if self.is_tree_mode() {
            return false;
        }
        let hit = self.child_at_x(x);
        let new_hovered = hit.map(|(idx, _)| idx);
        let mut changed = false;
        debug_input(&format!("[hover][row] x={x} y={y} hit={hit:?}"));

        // Dispatch Enter/Leave events when the hovered child changes.
        if new_hovered != self.hovered_child {
            if let Some(prev_idx) = self.hovered_child {
                let leave = Event::Leave(MouseLeaveEvent {
                    screen_x: x,
                    screen_y: y,
                    x,
                    y,
                });
                let mut ectx = EventCtx::default();
                if let Some(child) = self.children.get_mut(prev_idx) {
                    let mut ctx = crate::event::WidgetCtx::__from_dispatch(
                        crate::node_id::NodeId::default(),
                        &mut ectx,
                    );
                    child.on_event(&leave, &mut ctx);
                }
                changed = true;
            }
            self.hovered_child = new_hovered;
            if let Some(new_idx) = new_hovered {
                let enter = Event::Enter(MouseEnterEvent {
                    screen_x: x,
                    screen_y: y,
                    x,
                    y,
                });
                let mut ectx = EventCtx::default();
                if let Some(child) = self.children.get_mut(new_idx) {
                    let mut ctx = crate::event::WidgetCtx::__from_dispatch(
                        crate::node_id::NodeId::default(),
                        &mut ectx,
                    );
                    child.on_event(&enter, &mut ctx);
                }
                changed = true;
            }
        }

        if let Some((idx, local_x)) = hit {
            if let Some(child) = self.children.get_mut(idx) {
                changed |= child.on_mouse_move(local_x, y);
            }
        }

        changed
    }
}

/// Per-child layout inputs of a [`Row`]: fixed widths, margins, size
/// constraints and resolved styles.
struct RowChildMetrics {
    fixed_widths: Vec<Option<usize>>,
    margins: Vec<Margin>,
    constraints_list: Vec<LayoutConstraints>,
    resolved_list: Vec<crate::style::Style>,
}

impl RowChildMetrics {
    /// Total width of the fixed children (with margins) and the number of
    /// flexible children.
    fn fixed_total_and_flex_count(&self) -> (usize, usize) {
        let fixed_widths = &self.fixed_widths;
        let margins = &self.margins;
        let mut fixed_total = 0usize;
        let mut flex_count = 0usize;
        for (idx, fixed) in fixed_widths.iter().enumerate() {
            if let Some(width) = fixed {
                let margin = margins[idx];
                fixed_total = fixed_total
                    .saturating_add(width + margin.left as usize + margin.right as usize);
            } else {
                flex_count += 1;
            }
        }

        (fixed_total, flex_count)
    }

    /// Log the row's child metrics on the layout debug channel.
    fn log(&self, width: usize, height_limit: usize, count: usize, fixed_total: usize) {
        let fixed_widths = &self.fixed_widths;
        let margins = &self.margins;
        let constraints_list = &self.constraints_list;
        let resolved_list = &self.resolved_list;
        debug_layout(&format!(
            "[row] id={} viewport=({}, {}) children={} fixed_total={}",
            0u64, width, height_limit, count, fixed_total
        ));
        for (idx, fixed) in fixed_widths.iter().enumerate() {
            debug_layout(&format!(
                "[row] child={} fixed={:?} margin=({}, {}) constraints=({:?},{:?}) width={:?}",
                idx,
                fixed,
                margins[idx].left,
                margins[idx].right,
                constraints_list[idx].min_width,
                constraints_list[idx].max_width,
                resolved_list[idx].width
            ));
        }
    }

    /// Final child widths: fixed children keep their width plus margins,
    /// flexible children share the rest (`base`, plus one for the first
    /// `remainder` of them).
    fn widths(&self, count: usize, base: usize, remainder: usize) -> Vec<usize> {
        let fixed_widths = &self.fixed_widths;
        let margins = &self.margins;
        let mut flex_seen = 0usize;
        let widths: Vec<usize> = (0..count)
            .map(|idx| {
                if let Some(fixed) = fixed_widths[idx] {
                    let margin = margins[idx];
                    (fixed + margin.left as usize + margin.right as usize).max(1)
                } else {
                    let extra = usize::from(flex_seen < remainder);
                    flex_seen += 1;
                    (base + extra).max(1)
                }
            })
            .collect();
        widths
    }
}

impl Row {
    /// Resolve each child's style into row layout inputs.
    fn child_metrics(&self) -> RowChildMetrics {
        let count = self.children.len().max(1);
        let mut fixed_widths: Vec<Option<usize>> = vec![None; count];
        let mut margins: Vec<Margin> = vec![Margin::default(); count];
        let mut constraints_list: Vec<LayoutConstraints> = Vec::with_capacity(count);
        let mut resolved_list: Vec<crate::style::Style> = Vec::with_capacity(count);

        for (idx, child) in self.children.iter().enumerate() {
            let meta = css::selector_meta_generic(child.as_ref());
            let resolved = css::resolve_style(child.as_ref(), &meta);
            let margin = margin_from_style(&resolved);
            let style_constraints = constraints_from_style(&resolved);
            let constraints = style_constraints;

            let fixed =
                if let (Some(min), Some(max)) = (constraints.min_width, constraints.max_width) {
                    if min == max { Some(min) } else { None }
                } else if matches!(resolved.width, Some(Scalar::Auto)) {
                    let pad = resolved
                        .padding
                        .map_or(0, |s| s.left as usize)
                        .saturating_mul(2);
                    let (_, _, border_left, border_right) =
                        super::helpers::border_spacing_from_style(&resolved);
                    child
                        .content_width()
                        .map(|w| w.saturating_add(pad + border_left + border_right).max(1))
                } else {
                    None
                };

            fixed_widths[idx] = fixed;
            margins[idx] = margin;
            constraints_list.push(constraints);
            resolved_list.push(resolved);
        }
        RowChildMetrics {
            fixed_widths,
            margins,
            constraints_list,
            resolved_list,
        }
    }

    /// Render each child into its column.
    fn render_child_lines(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        metrics: &RowChildMetrics,
        widths: &[usize],
        height_limit: usize,
    ) -> Vec<Vec<Vec<Segment>>> {
        let mut child_lines: Vec<Vec<Vec<Segment>>> = Vec::new();

        for (idx, child) in self.children.iter().enumerate() {
            let margin = metrics.margins[idx];
            let child_width = widths[idx].max(1);
            let constraints = metrics.constraints_list[idx];
            let render_width = clamp_with_constraints(
                child_width
                    .saturating_sub(margin.left as usize + margin.right as usize)
                    .max(1),
                constraints.min_width,
                constraints.max_width,
                child_width
                    .saturating_sub(margin.left as usize + margin.right as usize)
                    .max(1),
            );
            let render_height = clamp_with_constraints(
                height_limit
                    .saturating_sub(margin.top as usize + margin.bottom as usize)
                    .max(1),
                constraints.min_height,
                constraints.max_height,
                height_limit
                    .saturating_sub(margin.top as usize + margin.bottom as usize)
                    .max(1),
            );
            let render_height = if let Some(fixed_total) = child.layout_height() {
                render_height.min(fixed_total.max(1))
            } else {
                render_height
            };
            let mut child_options = options.clone();
            child_options.size = (render_width, render_height);
            child_options.max_width = render_width;
            child_options.max_height = render_height;

            let segments = child.render_styled(console, &child_options);
            let mut lines =
                Segment::split_and_crop_lines(segments, render_width, None, true, false);
            let mut target_height = child.layout_height().unwrap_or(lines.len().max(1));
            target_height = clamp_with_constraints(
                target_height,
                constraints.min_height,
                constraints.max_height,
                height_limit,
            );
            lines = Segment::set_shape(&lines, render_width, Some(target_height), None, false);
            lines = pad_lines_to_width(lines, render_width);
            lines = apply_margin(lines, child_width, margin);
            child_lines.push(lines);
        }
        child_lines
    }

    /// Render each child into its column, wrapped in a layout debug box.
    fn render_debug_child_lines(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        debug: &DebugLayout,
        metrics: &RowChildMetrics,
        widths: &[usize],
        height_limit: usize,
    ) -> Vec<Vec<Vec<Segment>>> {
        let mut child_lines: Vec<Vec<Vec<Segment>>> = Vec::new();

        for (idx, child) in self.children.iter().enumerate() {
            let child_width = widths[idx].max(1);
            let constraints = metrics.constraints_list[idx];
            let margin = metrics.margins[idx];
            let render_width = clamp_with_constraints(
                child_width
                    .saturating_sub(margin.left as usize + margin.right as usize)
                    .max(1),
                constraints.min_width,
                constraints.max_width,
                child_width
                    .saturating_sub(margin.left as usize + margin.right as usize)
                    .max(1),
            );
            let render_height = clamp_with_constraints(
                height_limit,
                constraints.min_height,
                constraints.max_height,
                height_limit,
            );
            let mut child_options = options.clone();
            child_options.size = (render_width, render_height);
            child_options.max_width = render_width;
            child_options.max_height = render_height;

            let segments = child.render_styled(console, &child_options);
            let mut lines =
                Segment::split_and_crop_lines(segments, render_width, None, true, false);
            let mut target_height = child.layout_height().unwrap_or(lines.len().max(1));
            target_height = clamp_with_constraints(
                target_height,
                constraints.min_height,
                constraints.max_height,
                height_limit,
            );
            lines = Segment::set_shape(&lines, render_width, Some(target_height), None, false);
            lines = pad_lines_to_width(lines, render_width);
            lines = apply_margin(lines, child_width, margin);
            let child_height = lines.len().max(1);
            let debug_height = (child_height + 2).max(3);
            let label = if debug.show_sizes {
                Some(format!("{child_width}x{debug_height}"))
            } else {
                None
            };
            let wrapped = apply_debug_box(
                lines,
                child_width,
                debug_height,
                label.as_deref(),
                debug.style_for(idx),
            );
            child_lines.push(wrapped);
        }
        child_lines
    }

    /// Align the child columns vertically and join them into rows.
    fn join_child_columns(
        &self,
        child_lines: Vec<Vec<Vec<Segment>>>,
        widths: &[usize],
        width: usize,
        height_limit: usize,
    ) -> Segments {
        let max_child_height = child_lines
            .iter()
            .map(std::vec::Vec::len)
            .max()
            .unwrap_or(1)
            .max(1)
            .min(height_limit);

        let mut normalized_lines: Vec<Vec<Vec<Segment>>> = Vec::new();
        for lines in child_lines {
            let height = lines.len().max(1);
            let (pad_top, pad_bottom) = match self.align {
                RowAlign::Top => (0, max_child_height.saturating_sub(height)),
                RowAlign::Center => {
                    let total = max_child_height.saturating_sub(height);
                    (total / 2, total - total / 2)
                }
                RowAlign::Bottom => (max_child_height.saturating_sub(height), 0),
            };
            let mut padded = Vec::new();
            for _ in 0..pad_top {
                padded.push(Vec::new());
            }
            padded.extend(lines);
            for _ in 0..pad_bottom {
                padded.push(Vec::new());
            }
            normalized_lines.push(padded);
        }

        let mut out_lines: Vec<Vec<Segment>> = Vec::new();
        for row in 0..max_child_height {
            let mut line: Vec<Segment> = Vec::new();
            for (idx, lines) in normalized_lines.iter().enumerate() {
                let child_width = widths.get(idx).copied().unwrap_or(1).max(1);
                let child_line = lines
                    .get(row)
                    .cloned()
                    .unwrap_or_else(|| vec![Segment::new(" ".repeat(child_width))]);
                let adjusted = adjust_line_length_no_bg(&child_line, child_width);
                line.extend(adjusted);
            }
            out_lines.push(line);
        }

        out_lines.truncate(max_child_height);
        while out_lines.len() < max_child_height {
            out_lines.push(Vec::new());
        }
        let out_lines = pad_lines_to_width(out_lines, width);
        let line_count = out_lines.len();
        let mut out = Segments::new();
        for (idx, line) in out_lines.into_iter().enumerate() {
            out.extend(line);
            if idx + 1 < line_count {
                out.push(Segment::line());
            }
        }
        out
    }
}

impl crate::widgets::Render for Row {
    fn compose(&mut self) -> ComposeResult {
        self.children_extracted = true;
        crate::compose::zip_child_decls(
            std::mem::take(&mut self.children),
            std::mem::take(&mut self.child_decl_meta),
            std::mem::take(&mut self.child_handle_sinks),
        )
    }

    fn render(&self, console: &Console, options: &ConsoleOptions) -> Segments {
        let width = options.size.0.max(1);
        let height_limit = options.size.1.max(1);

        if self.is_tree_mode() {
            return blank_block(width, height_limit);
        }

        let count = self.children.len().max(1);
        let metrics = self.child_metrics();
        let (fixed_total, flex_count) = metrics.fixed_total_and_flex_count();

        if crate::debug::channel_enabled(crate::debug::DebugChannel::Layout) {
            metrics.log(width, height_limit, count, fixed_total);
        }

        let remaining = width.saturating_sub(fixed_total);
        let base = remaining.checked_div(flex_count).unwrap_or(0);
        let remainder = remaining.checked_rem(flex_count).unwrap_or(0);
        let widths = metrics.widths(count, base, remainder);

        if crate::debug::channel_enabled(crate::debug::DebugChannel::Layout) {
            debug_layout(&format!(
                "[row] id={} widths={:?} remaining={} flex_count={} base={} remainder={}",
                0u64, widths, remaining, flex_count, base, remainder
            ));
        }

        let child_lines =
            self.render_child_lines(console, options, &metrics, &widths, height_limit);
        self.join_child_columns(child_lines, &widths, width, height_limit)
    }

    fn render_with_debug(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        debug: &DebugLayout,
    ) -> Segments {
        if self.is_tree_mode() {
            return Widget::render(self, console, options);
        }

        let width = options.size.0.max(1);
        let height_limit = options.size.1.max(1);

        let count = self.children.len().max(1);
        let metrics = self.child_metrics();
        let (fixed_total, flex_count) = metrics.fixed_total_and_flex_count();

        let remaining = width.saturating_sub(fixed_total);
        let base = remaining.checked_div(flex_count).unwrap_or(0);
        let remainder = remaining.checked_rem(flex_count).unwrap_or(0);
        let widths = metrics.widths(count, base, remainder);

        let child_lines =
            self.render_debug_child_lines(console, options, debug, &metrics, &widths, height_limit);
        self.join_child_columns(child_lines, &widths, width, height_limit)
    }
}

impl crate::widgets::StyleIdentity for Row {
    crate::seed_style_identity_methods!();

    fn set_inline_style(&mut self, style: crate::style::Style) {
        self.seed.styles.style = style;
    }

    fn take_node_seed(&mut self) -> NodeSeed {
        std::mem::take(&mut self.seed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockKind {
    Top,
    Bottom,
    Left,
    Right,
    Fill,
}

pub struct DockItem {
    kind: DockKind,
    size: Option<usize>,
    child: Box<dyn Widget>,
}

#[widget(Focus, Interactive, Layout, StyleIdentity)]
pub struct Dock {
    items: Vec<DockItem>,
    items_extracted: bool,
    fixed_height: Option<usize>,
    last_layout_width: AtomicUsize,
    last_layout_height: AtomicUsize,
    seed: NodeSeed,
}

impl Default for Dock {
    fn default() -> Self {
        Self::new()
    }
}

impl Dock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            items_extracted: false,
            fixed_height: None,
            last_layout_width: AtomicUsize::new(1),
            last_layout_height: AtomicUsize::new(1),
            seed: NodeSeed::default(),
        }
    }

    #[must_use]
    pub fn height(mut self, height: usize) -> Self {
        self.fixed_height = Some(height.max(1));
        self
    }

    #[must_use]
    pub fn push_top(mut self, height: Option<usize>, child: impl Widget + 'static) -> Self {
        self.items.push(DockItem {
            kind: DockKind::Top,
            size: height,
            child: Box::new(child),
        });
        self
    }

    #[must_use]
    pub fn push_bottom(mut self, height: Option<usize>, child: impl Widget + 'static) -> Self {
        self.items.push(DockItem {
            kind: DockKind::Bottom,
            size: height,
            child: Box::new(child),
        });
        self
    }

    #[must_use]
    pub fn push_left(mut self, width: usize, child: impl Widget + 'static) -> Self {
        self.items.push(DockItem {
            kind: DockKind::Left,
            size: Some(width),
            child: Box::new(child),
        });
        self
    }

    #[must_use]
    pub fn push_right(mut self, width: usize, child: impl Widget + 'static) -> Self {
        self.items.push(DockItem {
            kind: DockKind::Right,
            size: Some(width),
            child: Box::new(child),
        });
        self
    }

    #[must_use]
    pub fn push_fill(mut self, child: impl Widget + 'static) -> Self {
        self.items.push(DockItem {
            kind: DockKind::Fill,
            size: None,
            child: Box::new(child),
        });
        self
    }

    fn is_tree_mode(&self) -> bool {
        self.items_extracted
    }

    fn apply_item_layout_hints(item: &mut DockItem) {
        let mut style = crate::style::Style::default();
        match item.kind {
            DockKind::Top => {
                style.dock = Some(StyleDock::Top);
                if let Some(height) = item.size {
                    style.height = Some(Scalar::Cells(height.to_u16_sat()));
                    // Dock API sizes are absolute band sizes (including chrome).
                    style.box_sizing = Some(BoxSizing::BorderBox);
                }
            }
            DockKind::Bottom => {
                style.dock = Some(StyleDock::Bottom);
                if let Some(height) = item.size {
                    style.height = Some(Scalar::Cells(height.to_u16_sat()));
                    // Dock API sizes are absolute band sizes (including chrome).
                    style.box_sizing = Some(BoxSizing::BorderBox);
                }
            }
            DockKind::Left => {
                style.dock = Some(StyleDock::Left);
                if let Some(width) = item.size {
                    style.width = Some(Scalar::Cells(width.to_u16_sat()));
                    // Dock API sizes are absolute band sizes (including chrome).
                    style.box_sizing = Some(BoxSizing::BorderBox);
                }
            }
            DockKind::Right => {
                style.dock = Some(StyleDock::Right);
                if let Some(width) = item.size {
                    style.width = Some(Scalar::Cells(width.to_u16_sat()));
                    // Dock API sizes are absolute band sizes (including chrome).
                    style.box_sizing = Some(BoxSizing::BorderBox);
                }
            }
            DockKind::Fill => {
                // No dock style for fill items.
            }
        }
        if style != crate::style::Style::default() {
            item.child.set_inline_style(style);
        }
    }

    fn child_at_xy(&self, x: u16, y: u16) -> Option<(usize, u16, u16, u16, u16)> {
        let mut x0 = 0u16;
        let mut y0 = 0u16;
        let mut width = self
            .last_layout_width
            .load(Ordering::Relaxed)
            .max(1)
            .to_u16_sat();
        let mut height = self
            .last_layout_height
            .load(Ordering::Relaxed)
            .max(1)
            .to_u16_sat();
        let mut fill_idx: Option<usize> = None;
        let mut fill_rect: Option<(u16, u16, u16, u16)> = None;

        for (idx, item) in self.items.iter().enumerate() {
            match item.kind {
                DockKind::Top => {
                    let h = dock_item_height(item, height);
                    if let Some((lx, ly)) = local_in_rect(x, y, (x0, y0, width, h)) {
                        return Some((idx, lx, ly, width, h));
                    }
                    y0 = y0.saturating_add(h);
                    height = height.saturating_sub(h);
                }
                DockKind::Bottom => {
                    let h = dock_item_height(item, height);
                    let by = y0.saturating_add(height.saturating_sub(h));
                    if let Some((lx, ly)) = local_in_rect(x, y, (x0, by, width, h)) {
                        return Some((idx, lx, ly, width, h));
                    }
                    height = height.saturating_sub(h);
                }
                DockKind::Left => {
                    let w = dock_item_width(item, width);
                    if let Some((lx, ly)) = local_in_rect(x, y, (x0, y0, w, height)) {
                        return Some((idx, lx, ly, w, height));
                    }
                    x0 = x0.saturating_add(w);
                    width = width.saturating_sub(w);
                }
                DockKind::Right => {
                    let w = dock_item_width(item, width);
                    let bx = x0.saturating_add(width.saturating_sub(w));
                    if let Some((lx, ly)) = local_in_rect(x, y, (bx, y0, w, height)) {
                        return Some((idx, lx, ly, w, height));
                    }
                    width = width.saturating_sub(w);
                }
                DockKind::Fill => {
                    fill_idx = Some(idx);
                    fill_rect = Some((x0, y0, width.max(1), height.max(1)));
                }
            }
        }

        if let (Some(idx), Some(rect)) = (fill_idx, fill_rect)
            && let Some((lx, ly)) = local_in_rect(x, y, rect)
        {
            return Some((idx, lx, ly, rect.2, rect.3));
        }
        None
    }

    /// Lay out and render the dock items: top and bottom bars, left and right
    /// columns, and the fill item in the middle. With `debug`, each item is
    /// wrapped in a layout debug box.
    fn render_items(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        debug: Option<&DebugLayout>,
    ) -> Segments {
        let mut remaining_width = options.size.0.max(1);
        let mut remaining_height = self.fixed_height.unwrap_or_else(|| options.size.1.max(1));

        let mut top_lines: Vec<Vec<Segment>> = Vec::new();
        let mut bottom_lines: Vec<Vec<Segment>> = Vec::new();

        let mut left_columns: Vec<(usize, Vec<Vec<Segment>>)> = Vec::new();
        let mut right_columns: Vec<(usize, Vec<Vec<Segment>>)> = Vec::new();
        let mut fill_index: Option<usize> = None;

        for (idx, item) in self.items.iter().enumerate() {
            let child = item.child.as_ref();
            match item.kind {
                DockKind::Top | DockKind::Bottom => {
                    let height = item
                        .size
                        .or_else(|| item.child.layout_height())
                        .unwrap_or(1)
                        .min(remaining_height);
                    let lines = dock_item_lines(
                        child,
                        console,
                        options,
                        (remaining_width, height),
                        debug,
                        idx,
                    );
                    if matches!(item.kind, DockKind::Top) {
                        top_lines.extend(lines);
                    } else {
                        bottom_lines.extend(lines);
                    }
                    remaining_height = remaining_height.saturating_sub(height);
                }
                DockKind::Left | DockKind::Right => {
                    let width = item.size.unwrap_or(1).min(remaining_width);
                    let lines = dock_item_lines(
                        child,
                        console,
                        options,
                        (width, remaining_height),
                        debug,
                        idx,
                    );
                    if matches!(item.kind, DockKind::Left) {
                        left_columns.push((width, lines));
                    } else {
                        right_columns.push((width, lines));
                    }
                    remaining_width = remaining_width.saturating_sub(width);
                }
                DockKind::Fill => {
                    fill_index = Some(idx);
                }
            }
        }

        let fill_lines = fill_index.map(|idx| {
            let child = self.items[idx].child.as_ref();
            dock_item_lines(
                child,
                console,
                options,
                (remaining_width, remaining_height),
                debug,
                idx,
            )
        });

        let middle_lines = dock_middle_lines(
            &left_columns,
            fill_lines.as_ref(),
            &right_columns,
            remaining_width,
            remaining_height,
        );

        join_lines(
            top_lines
                .into_iter()
                .chain(middle_lines)
                .chain(bottom_lines)
                .collect(),
        )
    }
}

impl crate::widgets::Focus for Dock {
    fn focusable(&self) -> bool {
        if self.is_tree_mode() {
            return false;
        }
        self.items.iter().any(|item| item.child.focusable())
    }
}

impl crate::widgets::Interactive for Dock {
    fn on_event(&mut self, event: &Event, ctx: &mut crate::event::WidgetCtx) {
        if self.is_tree_mode() {
            return;
        }
        match event {
            Event::Action(Action::FocusNext | Action::FocusPrev) => {
                return;
            }
            Event::MouseDown(mouse) => {
                if let Some((idx, local_x, local_y, w, h)) = self.child_at_xy(mouse.x, mouse.y) {
                    if let Some(item) = self.items.get_mut(idx) {
                        item.child.on_layout(w, h);
                    }
                    let child_event = Event::MouseDown(crate::event::MouseDownEvent {
                        target: NodeId::default(),
                        screen_x: mouse.screen_x,
                        screen_y: mouse.screen_y,
                        x: local_x,
                        y: local_y,
                    });
                    if let Some(item) = self.items.get_mut(idx) {
                        item.child.on_event(&child_event, ctx);
                        if ctx.handled() {
                            return;
                        }
                    }
                }
            }
            Event::MouseUp(mouse) => {
                if let Some((idx, local_x, local_y, w, h)) = self.child_at_xy(mouse.x, mouse.y) {
                    if let Some(item) = self.items.get_mut(idx) {
                        item.child.on_layout(w, h);
                    }
                    let child_event = Event::MouseUp(crate::event::MouseUpEvent {
                        target: Some(NodeId::default()),
                        screen_x: mouse.screen_x,
                        screen_y: mouse.screen_y,
                        x: local_x,
                        y: local_y,
                    });
                    if let Some(item) = self.items.get_mut(idx) {
                        item.child.on_event(&child_event, ctx);
                        if ctx.handled() {
                            return;
                        }
                    }
                }
            }
            Event::MouseScroll(mouse) => {
                if let Some((idx, local_x, local_y, w, h)) = self.child_at_xy(mouse.x, mouse.y) {
                    if let Some(item) = self.items.get_mut(idx) {
                        item.child.on_layout(w, h);
                    }
                    let child_event = Event::MouseScroll(crate::event::MouseScrollEvent {
                        target: Some(NodeId::default()),
                        screen_x: mouse.screen_x,
                        screen_y: mouse.screen_y,
                        x: local_x,
                        y: local_y,
                        delta_x: mouse.delta_x,
                        delta_y: mouse.delta_y,
                        modifiers: mouse.modifiers,
                    });
                    if let Some(item) = self.items.get_mut(idx) {
                        item.child.on_event(&child_event, ctx);
                        if ctx.handled() {
                            return;
                        }
                    }
                }
            }
            _ => {}
        }
        for item in &mut self.items {
            item.child.on_event(event, ctx);
            if ctx.handled() {
                break;
            }
        }
    }

    fn on_mouse_move(&mut self, x: u16, y: u16) -> bool {
        if self.is_tree_mode() {
            return false;
        }
        let mut changed = false;
        let hit = self.child_at_xy(x, y);
        if let Some((idx, local_x, local_y, w, h)) = hit
            && let Some(item) = self.items.get_mut(idx)
        {
            item.child.on_layout(w, h);
            changed |= item.child.on_mouse_move(local_x, local_y);
        }
        changed
    }

    fn on_mount(&mut self, ctx: &mut crate::event::WidgetCtx) {
        if !self.is_tree_mode() {
            for item in &mut self.items {
                item.child.on_mount(ctx);
            }
        }
    }

    fn on_unmount(&mut self) {
        if !self.is_tree_mode() {
            for item in &mut self.items {
                item.child.on_unmount();
            }
        }
    }

    fn on_tick(&mut self, tick: u64) {
        if !self.is_tree_mode() {
            for item in &mut self.items {
                item.child.on_tick(tick);
            }
        }
    }

    fn on_resize(&mut self, width: u16, height: u16) {
        if !self.is_tree_mode() {
            for item in &mut self.items {
                item.child.on_resize(width, height);
            }
        }
    }

    fn on_event_capture(&mut self, event: &Event, ctx: &mut crate::event::WidgetCtx) {
        if self.is_tree_mode() {
            return;
        }
        for item in &mut self.items {
            item.child.on_event_capture(event, ctx);
            if ctx.handled() {
                break;
            }
        }
    }
}

impl crate::widgets::Layout for Dock {
    fn layout_height(&self) -> Option<usize> {
        self.fixed_height
    }
}

impl crate::widgets::Render for Dock {
    fn compose(&mut self) -> ComposeResult {
        self.items_extracted = true;
        let mut children: ComposeResult = Vec::with_capacity(self.items.len());
        for mut item in std::mem::take(&mut self.items) {
            Self::apply_item_layout_hints(&mut item);
            children.push(crate::compose::ChildDecl::new(item.child));
        }
        children
    }

    fn render(&self, console: &Console, options: &ConsoleOptions) -> Segments {
        self.last_layout_width
            .store(options.size.0.max(1), Ordering::Relaxed);
        self.last_layout_height.store(
            self.fixed_height.unwrap_or_else(|| options.size.1.max(1)),
            Ordering::Relaxed,
        );

        if self.is_tree_mode() {
            let width = options.size.0.max(1);
            let height = self.fixed_height.unwrap_or_else(|| options.size.1.max(1));
            return blank_block(width, height);
        }

        self.render_items(console, options, None)
    }

    fn render_with_debug(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        debug: &DebugLayout,
    ) -> Segments {
        if self.is_tree_mode() {
            return Widget::render(self, console, options);
        }

        self.render_items(console, options, Some(debug))
    }
}

impl crate::widgets::StyleIdentity for Dock {
    crate::seed_style_identity_methods!();

    fn set_inline_style(&mut self, style: crate::style::Style) {
        self.seed.styles.style = style;
    }

    fn take_node_seed(&mut self) -> NodeSeed {
        std::mem::take(&mut self.seed)
    }
}

#[widget(Interactive, StyleIdentity)]
pub struct Grid {
    rows: usize,
    cols: usize,
    cells: Vec<Option<Box<dyn Widget>>>,
    cells_extracted: bool,
    row_gaps: usize,
    col_gaps: usize,
    row_sizes: Option<Vec<usize>>,
    col_sizes: Option<Vec<usize>>,
    seed: NodeSeed,
    /// (index into the `compose()` extraction order, `css_id`,
    /// classes) recorded by `with_compose` so `.with_id()`/`.with_classes()`
    /// metadata on declared children reaches the mounted node.
    child_decl_meta: Vec<crate::widgets::ChildDeclMeta>,
    /// (index into the extraction order, sink) recorded by `with_compose` for
    /// decls bound via `HandleSlot::bind`.
    child_handle_sinks: Vec<(usize, crate::handle::HandleSink)>,
}

impl Grid {
    #[must_use]
    pub fn new(rows: usize, cols: usize) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        Self {
            rows,
            cols,
            cells: (0..rows * cols).map(|_| None).collect(),
            cells_extracted: false,
            row_gaps: 0,
            col_gaps: 0,
            row_sizes: None,
            col_sizes: None,
            seed: NodeSeed::default(),
            child_decl_meta: Vec::new(),
            child_handle_sinks: Vec::new(),
        }
    }

    pub fn set(&mut self, row: usize, col: usize, child: impl Widget + 'static) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        let idx = row * self.cols + col;
        self.cells[idx] = Some(Box::new(child));
    }

    #[must_use]
    pub fn with_cell(mut self, row: usize, col: usize, child: impl Widget + 'static) -> Self {
        self.set(row, col, child);
        self
    }

    #[must_use]
    pub fn with_child(mut self, child: impl Widget + 'static) -> Self {
        self.push(child);
        self
    }

    #[must_use]
    pub fn with_compose(mut self, children: ComposeResult) -> Self {
        for decl in children {
            let crate::compose::ChildDecl {
                builder,
                id,
                classes,
                handle_sink,
                ..
            } = decl;
            let crate::compose::WidgetBuilder::Ready(widget) = builder;
            // Index within the `compose()` extraction order, which
            // filters out empty cells. `push_boxed` fills the first empty slot, so
            // the count of occupied cells before insertion equals this child's
            // position in the extracted sequence.
            let index = self.cells.iter().filter(|c| c.is_some()).count();
            self.push_boxed(widget);
            if id.is_some() || !classes.is_empty() {
                self.child_decl_meta.push((index, id, classes));
            }
            if let Some(sink) = handle_sink {
                self.child_handle_sinks.push((index, sink));
            }
        }
        self
    }

    pub fn push(&mut self, child: impl Widget + 'static) {
        self.push_boxed(Box::new(child));
    }

    fn push_boxed(&mut self, child: Box<dyn Widget>) {
        if let Some(idx) = self.cells.iter().position(std::option::Option::is_none) {
            self.cells[idx] = Some(child);
        } else {
            // Allow overflow so tree-mode grid auto-placement can flow into extra rows.
            self.cells.push(Some(child));
        }
    }

    #[must_use]
    pub fn id(mut self, value: impl Into<String>) -> Self {
        self.seed.css_id = Some(value.into());
        self
    }

    #[must_use]
    pub fn class(mut self, value: impl Into<String>) -> Self {
        self.seed.classes.push(value.into());
        self
    }

    #[must_use]
    pub fn classes(mut self, values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for value in values {
            self.seed.classes.push(value.into());
        }
        self
    }

    #[must_use]
    pub fn row_gap(mut self, gap: usize) -> Self {
        self.row_gaps = gap;
        self
    }

    #[must_use]
    pub fn col_gap(mut self, gap: usize) -> Self {
        self.col_gaps = gap;
        self
    }

    #[must_use]
    pub fn row_sizes(mut self, sizes: Vec<usize>) -> Self {
        if sizes.len() == self.rows {
            self.row_sizes = Some(sizes);
        }
        self
    }

    #[must_use]
    pub fn col_sizes(mut self, sizes: Vec<usize>) -> Self {
        if sizes.len() == self.cols {
            self.col_sizes = Some(sizes);
        }
        self
    }

    fn is_tree_mode(&self) -> bool {
        self.cells_extracted
    }
}

impl crate::widgets::Interactive for Grid {
    fn on_mount(&mut self, ctx: &mut crate::event::WidgetCtx) {
        if !self.is_tree_mode() {
            for child in self.cells.iter_mut().flatten() {
                child.on_mount(ctx);
            }
        }
    }

    fn on_unmount(&mut self) {
        if !self.is_tree_mode() {
            for child in self.cells.iter_mut().flatten() {
                child.on_unmount();
            }
        }
    }

    fn on_tick(&mut self, tick: u64) {
        if !self.is_tree_mode() {
            for child in self.cells.iter_mut().flatten() {
                child.on_tick(tick);
            }
        }
    }

    fn on_resize(&mut self, width: u16, height: u16) {
        if !self.is_tree_mode() {
            for child in self.cells.iter_mut().flatten() {
                child.on_resize(width, height);
            }
        }
    }

    fn on_event_capture(&mut self, event: &Event, ctx: &mut crate::event::WidgetCtx) {
        if self.is_tree_mode() {
            return;
        }
        for child in self.cells.iter_mut().flatten() {
            child.on_event_capture(event, ctx);
            if ctx.handled() {
                break;
            }
        }
    }

    fn on_event(&mut self, event: &Event, ctx: &mut crate::event::WidgetCtx) {
        if self.is_tree_mode() {
            return;
        }
        for child in self.cells.iter_mut().flatten() {
            child.on_event(event, ctx);
            if ctx.handled() {
                break;
            }
        }
    }
}

impl Grid {
    /// Lay out and render the cells. With `debug`, each occupied cell is
    /// wrapped in a layout debug box.
    #[allow(clippy::needless_range_loop)] // r/c used as 2D indices into row_heights[r]/col_widths[c]
    fn render_cells(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        debug: Option<&DebugLayout>,
    ) -> Segments {
        let width = options.size.0.max(1);
        let height = options.size.1.max(1);

        let total_col_gaps = self.col_gaps.saturating_mul(self.cols.saturating_sub(1));
        let total_row_gaps = self.row_gaps.saturating_mul(self.rows.saturating_sub(1));
        let inner_width = width.saturating_sub(total_col_gaps).max(1);
        let inner_height = height.saturating_sub(total_row_gaps).max(1);

        let col_widths: Vec<usize> = if let Some(sizes) = &self.col_sizes {
            sizes.clone()
        } else {
            let base_w = inner_width / self.cols;
            let rem_w = inner_width % self.cols;
            (0..self.cols)
                .map(|c| base_w + usize::from(c < rem_w))
                .collect()
        };

        let row_heights: Vec<usize> = if let Some(sizes) = &self.row_sizes {
            sizes.clone()
        } else {
            let base_h = inner_height / self.rows;
            let rem_h = inner_height % self.rows;
            (0..self.rows)
                .map(|r| base_h + usize::from(r < rem_h))
                .collect()
        };

        let mut cell_lines: Vec<Vec<Vec<Vec<Segment>>>> = Vec::new();
        for r in 0..self.rows {
            let mut row_cells = Vec::new();
            for c in 0..self.cols {
                let idx = r * self.cols + c;
                let size = (col_widths[c].max(1), row_heights[r].max(1));
                row_cells.push(self.render_cell(console, options, debug, idx, size));
            }
            cell_lines.push(row_cells);
        }

        join_lines(self.join_cell_rows(&cell_lines, &col_widths, &row_heights, width))
    }

    /// Render one cell's child (or blank) at `(cell_width, cell_height)`.
    fn render_cell(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        debug: Option<&DebugLayout>,
        idx: usize,
        (cell_width, cell_height): (usize, usize),
    ) -> Vec<Vec<Segment>> {
        let (margin, constraints) = if let Some(child) = &self.cells[idx] {
            let meta = css::selector_meta_generic(child.as_ref());
            let resolved = css::resolve_style(child.as_ref(), &meta);
            let style_constraints = constraints_from_style(&resolved);
            (margin_from_style(&resolved), style_constraints)
        } else {
            (Margin::default(), LayoutConstraints::default())
        };
        let render_width = clamp_with_constraints(
            cell_width
                .saturating_sub(margin.left as usize + margin.right as usize)
                .max(1),
            constraints.min_width,
            constraints.max_width,
            cell_width
                .saturating_sub(margin.left as usize + margin.right as usize)
                .max(1),
        );
        let render_height = clamp_with_constraints(
            cell_height
                .saturating_sub(margin.top as usize + margin.bottom as usize)
                .max(1),
            constraints.min_height,
            constraints.max_height,
            cell_height
                .saturating_sub(margin.top as usize + margin.bottom as usize)
                .max(1),
        );
        let mut child_options = options.clone();
        child_options.size = (render_width, render_height);
        child_options.max_width = render_width;
        child_options.max_height = render_height;
        if let Some(child) = &self.cells[idx] {
            let segments = child.render_styled(console, &child_options);
            let mut lines =
                Segment::split_and_crop_lines(segments, render_width, None, true, false);
            lines = Segment::set_shape(&lines, render_width, Some(render_height), None, false);
            lines = pad_lines_to_width(lines, render_width);
            lines = apply_margin(lines, cell_width, margin);
            debug_boxed(
                lines,
                debug,
                idx,
                cell_width,
                (cell_height + 2).max(3),
                (cell_width, cell_height),
            )
        } else {
            Segment::set_shape(&[], cell_width, Some(cell_height), None, false)
        }
    }

    /// Join the rendered cells row by row, with the column and row gaps.
    #[allow(clippy::needless_range_loop)] // r/c used as 2D indices into row_heights[r]/col_widths[c]
    fn join_cell_rows(
        &self,
        cell_lines: &[Vec<Vec<Vec<Segment>>>],
        col_widths: &[usize],
        row_heights: &[usize],
        width: usize,
    ) -> Vec<Vec<Segment>> {
        let mut out_lines: Vec<Vec<Segment>> = Vec::new();
        for r in 0..self.rows {
            let cell_height = row_heights[r].max(1);
            for row in 0..cell_height {
                let mut line: Vec<Segment> = Vec::new();
                for c in 0..self.cols {
                    let cell_width = col_widths[c].max(1);
                    let lines = &cell_lines[r][c];
                    let cell_line = lines
                        .get(row)
                        .cloned()
                        .unwrap_or_else(|| vec![Segment::new(" ".repeat(cell_width))]);
                    let adjusted = Segment::adjust_line_length(&cell_line, cell_width, None, true);
                    line.extend(adjusted);
                    if c + 1 < self.cols && self.col_gaps > 0 {
                        line.push(Segment::new(" ".repeat(self.col_gaps)));
                    }
                }
                out_lines.push(line);
            }
            if r + 1 < self.rows && self.row_gaps > 0 {
                let gap_line = vec![Segment::new(" ".repeat(width))];
                for _ in 0..self.row_gaps {
                    out_lines.push(gap_line.clone());
                }
            }
        }
        out_lines
    }
}

impl crate::widgets::Render for Grid {
    fn compose(&mut self) -> ComposeResult {
        self.cells_extracted = true;
        let children: Vec<Box<dyn Widget>> = self
            .cells
            .iter_mut()
            .filter_map(std::option::Option::take)
            .collect();
        crate::compose::zip_child_decls(
            children,
            std::mem::take(&mut self.child_decl_meta),
            std::mem::take(&mut self.child_handle_sinks),
        )
    }

    fn render(&self, console: &Console, options: &ConsoleOptions) -> Segments {
        let width = options.size.0.max(1);
        let height = options.size.1.max(1);

        if self.is_tree_mode() {
            return blank_block(width, height);
        }

        self.render_cells(console, options, None)
    }

    fn render_with_debug(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        debug: &DebugLayout,
    ) -> Segments {
        if self.is_tree_mode() {
            return Widget::render(self, console, options);
        }

        self.render_cells(console, options, Some(debug))
    }
}

impl crate::widgets::StyleIdentity for Grid {
    crate::seed_style_identity_methods!();

    fn set_inline_style(&mut self, style: crate::style::Style) {
        self.seed.styles.style = style;
    }

    fn take_node_seed(&mut self) -> NodeSeed {
        std::mem::take(&mut self.seed)
    }
}

/// A blank block `width` cells wide and `height` lines tall.
fn blank_block(width: usize, height: usize) -> Segments {
    let blank = vec![Segment::new(" ".repeat(width))];
    let mut out = Segments::new();
    for row in 0..height {
        out.extend(blank.clone());
        if row + 1 < height {
            out.push(Segment::line());
        }
    }
    out
}

/// Join lines into segments with a line break between them.
fn join_lines(out_lines: Vec<Vec<Segment>>) -> Segments {
    let line_count = out_lines.len();
    let mut out = Segments::new();
    for (idx, line) in out_lines.into_iter().enumerate() {
        out.extend(line);
        if idx + 1 < line_count {
            out.push(Segment::line());
        }
    }
    out
}

/// Wrap `lines` in a layout debug box `width` by `box_height` when `debug`
/// is set, labelled with `label_size` when `show_sizes` is on.
fn debug_boxed(
    lines: Vec<Vec<Segment>>,
    debug: Option<&DebugLayout>,
    idx: usize,
    width: usize,
    box_height: usize,
    label_size: (usize, usize),
) -> Vec<Vec<Segment>> {
    let Some(debug) = debug else {
        return lines;
    };
    let label = if debug.show_sizes {
        Some(format!("{}x{}", label_size.0, label_size.1))
    } else {
        None
    };
    apply_debug_box(
        lines,
        width,
        box_height,
        label.as_deref(),
        debug.style_for(idx),
    )
}

/// `(x, y)` relative to the rect `(rx, ry, rw, rh)`, when it lies inside.
fn local_in_rect(x: u16, y: u16, (rx, ry, rw, rh): (u16, u16, u16, u16)) -> Option<(u16, u16)> {
    (x >= rx && x < rx.saturating_add(rw) && y >= ry && y < ry.saturating_add(rh))
        .then(|| (x.saturating_sub(rx), y.saturating_sub(ry)))
}

/// Height of a top/bottom dock item within `available` rows.
fn dock_item_height(item: &DockItem, available: u16) -> u16 {
    item.size
        .or_else(|| item.child.layout_height())
        .unwrap_or(1)
        .max(1)
        .min(available as usize)
        .to_u16_sat()
}

/// Width of a left/right dock item within `available` columns.
fn dock_item_width(item: &DockItem, available: u16) -> u16 {
    item.size
        .unwrap_or(1)
        .max(1)
        .min(available as usize)
        .to_u16_sat()
}

/// Render a dock item's child into `(width, height)`, clamped by its CSS
/// size constraints, and pad its lines to `pad_width`.
fn render_dock_child(
    child: &dyn Widget,
    console: &Console,
    options: &ConsoleOptions,
    (width, height): (usize, usize),
    pad_width: usize,
) -> Vec<Vec<Segment>> {
    let constraints = {
        let meta = css::selector_meta_generic(child);
        let resolved = css::resolve_style(child, &meta);
        constraints_from_style(&resolved)
    };
    let render_height = clamp_with_constraints(
        height,
        constraints.min_height,
        constraints.max_height,
        height,
    );
    let render_width =
        clamp_with_constraints(width, constraints.min_width, constraints.max_width, width);
    let mut child_options = options.clone();
    child_options.size = (render_width, render_height);
    child_options.max_width = render_width;
    child_options.max_height = render_height;
    let segments = child.render_styled(console, &child_options);
    let mut lines = Segment::split_and_crop_lines(segments, render_width, None, true, false);
    lines = Segment::set_shape(&lines, render_width, Some(render_height), None, false);
    pad_lines_to_width(lines, pad_width)
}

/// Render a dock item's child at `(width, height)`, wrapped in a layout
/// debug box (`height + 2` tall, at least 3) when `debug` is set.
fn dock_item_lines(
    child: &dyn Widget,
    console: &Console,
    options: &ConsoleOptions,
    (width, height): (usize, usize),
    debug: Option<&DebugLayout>,
    idx: usize,
) -> Vec<Vec<Segment>> {
    let lines = render_dock_child(child, console, options, (width, height), width);
    let debug_height = (height + 2).max(3);
    debug_boxed(
        lines,
        debug,
        idx,
        width,
        debug_height,
        (width, debug_height),
    )
}

/// The dock's middle band: left columns, the fill item (or blank), then
/// right columns, `height` lines tall.
fn dock_middle_lines(
    left_columns: &[(usize, Vec<Vec<Segment>>)],
    fill_lines: Option<&Vec<Vec<Segment>>>,
    right_columns: &[(usize, Vec<Vec<Segment>>)],
    remaining_width: usize,
    remaining_height: usize,
) -> Vec<Vec<Segment>> {
    let mut middle_lines: Vec<Vec<Segment>> = Vec::new();
    for row in 0..remaining_height {
        let mut line: Vec<Segment> = Vec::new();

        for (col_width, column) in left_columns {
            let col_line = column
                .get(row)
                .cloned()
                .unwrap_or_else(|| vec![Segment::new(" ".repeat(*col_width))]);
            let adjusted = Segment::adjust_line_length(&col_line, *col_width, None, true);
            line.extend(adjusted);
        }

        let remaining_mid_width = remaining_width;
        if let Some(lines) = fill_lines {
            let fill_line = lines
                .get(row)
                .cloned()
                .unwrap_or_else(|| vec![Segment::new(" ".repeat(remaining_mid_width))]);
            let adjusted = Segment::adjust_line_length(&fill_line, remaining_mid_width, None, true);
            line.extend(adjusted);
        } else {
            line.extend(vec![Segment::new(" ".repeat(remaining_mid_width))]);
        }

        for (col_width, column) in right_columns {
            let col_line = column
                .get(row)
                .cloned()
                .unwrap_or_else(|| vec![Segment::new(" ".repeat(*col_width))]);
            let adjusted = Segment::adjust_line_length(&col_line, *col_width, None, true);
            line.extend(adjusted);
        }

        middle_lines.push(line);
    }
    middle_lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::Label;

    #[test]
    fn row_compose_drains_declared_child() {
        let mut r = Row::new().with_child(Label::new("a"));
        // compose() is the single child path — it yields the declared child.
        assert_eq!(r.compose().len(), 1);
    }

    #[test]
    fn row_compose_extracts_all() {
        let mut r = Row::new()
            .with_child(Label::new("a"))
            .with_child(Label::new("b"));
        let children = r.compose();
        assert_eq!(children.len(), 2);
        assert!(r.children().is_empty());
    }

    #[test]
    fn row_with_compose_preserves_child_decl_meta() {
        use crate::compose::ChildDecl;
        let decls: Vec<ChildDecl> = vec![
            ChildDecl::from(Label::new("a")).with_id("disp-0"),
            ChildDecl::from(Label::new("b")).with_classes(&["highlight"]),
            ChildDecl::from(Label::new("c")),
        ];
        let mut r = Row::new().with_compose(decls);
        // compose() now bundles each child's id/classes into its ChildDecl.
        let out = r.compose();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].id.as_deref(), Some("disp-0"));
        assert!(out[0].classes.is_empty());
        assert_eq!(out[1].id, None);
        assert_eq!(out[1].classes, vec!["highlight".to_string()]);
        assert_eq!(out[2].id, None);
        assert!(out[2].classes.is_empty());
    }

    #[test]
    fn grid_with_compose_preserves_child_decl_meta() {
        use crate::compose::ChildDecl;
        let decls: Vec<ChildDecl> = vec![
            ChildDecl::from(Label::new("a")),
            ChildDecl::from(Label::new("b")).with_id("cell-1"),
        ];
        let mut g = Grid::new(2, 2).with_compose(decls);
        // The id rides on the ChildDecl at its (filtered) extraction position.
        let out = g.compose();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, None);
        assert_eq!(out[1].id.as_deref(), Some("cell-1"));
    }

    // ── Row tree-mode regression tests ──────────────────────────────

    #[test]
    fn row_tree_mode_flag_set_after_extraction() {
        let mut r = Row::new()
            .with_child(Label::new("a"))
            .with_child(Label::new("b"));
        assert!(!r.is_tree_mode());
        let _ = r.compose();
        assert!(r.is_tree_mode());
    }

    #[test]
    fn row_tree_mode_render_returns_chrome() {
        let mut r = Row::new().with_child(Label::new("hello"));
        let _ = r.compose();

        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (10, 3);
        options.max_width = 10;
        options.max_height = 3;
        let segments = Widget::render(&r, &console, &options);
        assert!(!segments.is_empty());
    }

    #[test]
    fn row_tree_mode_on_event_does_not_panic() {
        let mut r = Row::new().with_child(Label::new("a"));
        let _ = r.compose();

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            r.on_event(&Event::Action(Action::FocusNext), &mut __w);
        }
        assert!(!ctx.handled());
    }

    #[test]
    fn row_tree_mode_mouse_move_returns_false() {
        let mut r = Row::new().with_child(Label::new("a"));
        let _ = r.compose();
        assert!(!r.on_mouse_move(0, 0));
    }

    // ── Dock tree-mode regression tests ─────────────────────────────

    #[test]
    fn dock_tree_mode_flag_set_after_extraction() {
        let mut d = Dock::new()
            .push_top(Some(1), Label::new("header"))
            .push_fill(Label::new("body"));
        assert!(!d.is_tree_mode());
        let children = d.compose();
        assert_eq!(children.len(), 2);
        assert!(d.is_tree_mode());
    }

    #[test]
    fn dock_explicit_size_hints_use_border_box_in_tree_mode() {
        let mut d = Dock::new()
            .push_bottom(Some(3), Label::new("footer"))
            .push_right(8, Label::new("side"));
        let children = d.compose();
        assert_eq!(children.len(), 2);

        let footer_style = children[0]
            .widget()
            .style()
            .expect("footer child should expose style after dock hinting");
        assert_eq!(footer_style.height, Some(Scalar::Cells(3)));
        assert_eq!(footer_style.box_sizing, Some(BoxSizing::BorderBox));

        let side_style = children[1]
            .widget()
            .style()
            .expect("side child should expose style after dock hinting");
        assert_eq!(side_style.width, Some(Scalar::Cells(8)));
        assert_eq!(side_style.box_sizing, Some(BoxSizing::BorderBox));
    }

    #[test]
    fn dock_tree_mode_render_returns_chrome() {
        let mut d = Dock::new()
            .push_top(Some(1), Label::new("header"))
            .push_fill(Label::new("body"));
        let _ = d.compose();

        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (20, 10);
        options.max_width = 20;
        options.max_height = 10;
        let segments = Widget::render(&d, &console, &options);
        assert!(!segments.is_empty());
    }

    #[test]
    fn dock_tree_mode_on_event_does_not_panic() {
        let mut d = Dock::new().push_fill(Label::new("body"));
        let _ = d.compose();

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            d.on_event(&Event::Action(Action::FocusNext), &mut __w);
        }
        assert!(!ctx.handled());
    }

    #[test]
    fn dock_tree_mode_mouse_move_returns_false() {
        let mut d = Dock::new().push_fill(Label::new("body"));
        let _ = d.compose();
        assert!(!d.on_mouse_move(0, 0));
    }

    // ── Grid tree-mode regression tests ─────────────────────────────

    #[test]
    fn grid_tree_mode_flag_set_after_extraction() {
        let mut g = Grid::new(2, 2)
            .with_cell(0, 0, Label::new("a"))
            .with_cell(0, 1, Label::new("b"))
            .with_cell(1, 0, Label::new("c"));
        assert!(!g.is_tree_mode());
        let children = g.compose();
        assert_eq!(children.len(), 3);
        assert!(g.is_tree_mode());
    }

    #[test]
    fn grid_tree_mode_render_returns_chrome() {
        let mut g =
            Grid::new(1, 2)
                .with_cell(0, 0, Label::new("a"))
                .with_cell(0, 1, Label::new("b"));
        let _ = g.compose();

        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (10, 3);
        options.max_width = 10;
        options.max_height = 3;
        let segments = Widget::render(&g, &console, &options);
        assert!(!segments.is_empty());
    }

    #[test]
    fn grid_tree_mode_on_event_does_not_panic() {
        let mut g = Grid::new(1, 1).with_cell(0, 0, Label::new("a"));
        let _ = g.compose();

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            g.on_event(&Event::Action(Action::FocusNext), &mut __w);
        }
        assert!(!ctx.handled());
    }

    #[test]
    fn row_id_and_class_are_carried_in_seed() {
        let mut r = Row::new().id("row-ident").class("my-row");
        let seed = r.take_node_seed();
        assert_eq!(seed.css_id.as_deref(), Some("row-ident"));
        assert!(seed.classes.iter().any(|c| c == "my-row"));
    }

    #[test]
    fn grid_id_and_class_already_present() {
        let mut g = Grid::new(2, 2).id("grid-ident").class("my-grid");
        let seed = g.take_node_seed();
        assert_eq!(seed.css_id.as_deref(), Some("grid-ident"));
        assert!(seed.classes.iter().any(|c| c == "my-grid"));
    }
}
