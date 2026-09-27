use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rich_rs::{Console, ConsoleOptions, Segment, Segments};
use textual_macros::widget;

use crate::compose::ComposeResult;
use crate::css;
use crate::debug::DebugLayout;
use crate::debug::debug_input;
use crate::event::{AnimationValueEvent, Event};
use crate::message::{MessageEvent, ScrollbarAxis, ScrollbarScrollTo};
use crate::node_id::NodeId;
use crate::num::Cast;
use crate::style::parse_color_like;
use crate::widgets::{NodeSeed, ScrollBar, ScrollBarCorner, Widget, scrollbar_max_offset};

#[widget(Interactive, Layout, Scrollable, StyleIdentity)]
pub struct AppRoot {
    children: Vec<Box<dyn Widget>>,
    children_extracted: bool,
    focused: Option<NodeId>,
    seed: NodeSeed,
    offset_x: f32,
    offset_y: f32,
    /// The running scroll animations, per axis.
    anim_x: crate::widgets::scrollbar::ScrollAnimationState,
    anim_y: crate::widgets::scrollbar::ScrollAnimationState,
    scroll_step_x: usize,
    scroll_step_y: usize,
    content_width: AtomicUsize,
    content_height: AtomicUsize,
    viewport_width: AtomicUsize,
    viewport_height: AtomicUsize,
    last_layout_height: u16,
    last_layout_width: u16,
    /// (index into `children`, sink) recorded by `with_child_handle` /
    /// `with_compose` (for decls bound via `HandleSlot::bind`).
    child_handle_sinks: Vec<(usize, crate::handle::HandleSink)>,
    /// (index into `children`, `css_id`, classes) recorded by `with_compose` so
    /// `.with_id()`/`.with_classes()` metadata on declared children reaches the
    /// mounted node.
    child_decl_meta: Vec<crate::widgets::ChildDeclMeta>,
}

#[cfg(test)]
use crate::event::Action;

const APP_ROOT_TYPE_ALIASES: &[&str] = &["AppRoot", "App"];
pub(crate) const APP_ROOT_VSCROLLBAR_ID: &str = "__app_root_vscrollbar";
pub(crate) const APP_ROOT_HSCROLLBAR_ID: &str = "__app_root_hscrollbar";
pub(crate) const APP_ROOT_SCROLLBAR_CORNER_ID: &str = "__app_root_scrollbar_corner";
const APP_ROOT_OFFSET_X_ATTR: &str = "approot.offset_x";
const APP_ROOT_OFFSET_Y_ATTR: &str = "approot.offset_y";

fn scrollbar_clamp_offset_f32(offset: f32, content_len: usize, viewport_len: usize) -> f32 {
    if !offset.is_finite() {
        return 0.0;
    }
    let max = scrollbar_max_offset(content_len.max(1), viewport_len.max(1)).to_f32_lossy();
    offset.clamp(0.0, max)
}

fn scrollbar_drag_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("TEXTUAL_DEBUG_SCROLLBAR_DRAG_TRACE").is_ok_and(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !(normalized.is_empty()
                || normalized == "0"
                || normalized == "false"
                || normalized == "off"
                || normalized == "no")
        })
    })
}

impl AppRoot {
    crate::seed_ident_methods!();

    #[must_use]
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            children_extracted: false,
            focused: None,
            seed: NodeSeed::default(),
            offset_x: 0.0,
            offset_y: 0.0,
            anim_x: crate::widgets::scrollbar::ScrollAnimationState::default(),
            anim_y: crate::widgets::scrollbar::ScrollAnimationState::default(),
            scroll_step_x: 2,
            scroll_step_y: 1,
            content_width: AtomicUsize::new(0),
            content_height: AtomicUsize::new(0),
            viewport_width: AtomicUsize::new(0),
            viewport_height: AtomicUsize::new(0),
            last_layout_height: 0,
            last_layout_width: 0,
            child_handle_sinks: Vec::new(),
            child_decl_meta: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_child(mut self, child: impl Widget + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    /// Add a child and bind `slot` to it; the slot is filled with the child's
    /// arena identity when the widget tree is built.
    #[must_use]
    pub fn with_child_handle<W: Widget + 'static>(
        mut self,
        child: W,
        slot: &crate::handle::HandleSlot<W>,
    ) -> Self {
        self.children.push(Box::new(child));
        self.child_handle_sinks
            .push((self.children.len() - 1, slot.make_sink()));
        self
    }

    /// Add multiple children from a `compose![]` result.
    ///
    /// Preserves each `ChildDecl`'s `id`/`classes` (so CSS id/class selectors
    /// match the mounted nodes) and any `handle_sink` bound via
    /// `HandleSlot::bind`, mirroring `App::mount_declarations`.
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

    /// Read-only access to the root's children.
    pub fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    /// Mutable access to the root's children.
    pub fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    pub fn focus_first(&mut self) {
        // Legacy stub calls removed (P1-14g): collect_focus_ids/set_focus_by_id
        // were no-ops. Tree-based focus management handles actual traversal.
        self.focused = None;
    }

    pub fn focus_next(&mut self) {
        // Legacy stub calls removed (P1-14g): collect_focus_ids/set_focus_by_id
        // were no-ops. Tree-based focus management handles actual traversal.
        // Keep self.focused field logic for compatibility.
    }

    pub fn focus_prev(&mut self) {
        // Legacy stub calls removed (P1-14g): collect_focus_ids/set_focus_by_id
        // were no-ops. Tree-based focus management handles actual traversal.
    }

    pub fn focus(&mut self, id: NodeId) -> bool {
        // Legacy stub calls removed (P1-14g): collect_focus_ids/set_focus_by_id
        // were no-ops. Update self.focused for compatibility; tree-based focus
        // management handles actual focus setting.
        self.focused = Some(id);
        true
    }

    pub fn set_virtual_content_size(&self, width: usize, height: usize) {
        self.content_width.store(width.max(1), Ordering::Relaxed);
        self.content_height.store(height.max(1), Ordering::Relaxed);
    }

    fn max_offset_y(&self) -> f32 {
        scrollbar_max_offset(
            self.content_height.load(Ordering::Relaxed).max(1),
            self.viewport_height.load(Ordering::Relaxed).max(1),
        )
        .to_f32_lossy()
    }

    fn clamp_offsets(&mut self) {
        self.offset_x = scrollbar_clamp_offset_f32(
            self.offset_x,
            self.content_width.load(Ordering::Relaxed).max(1),
            self.viewport_width.load(Ordering::Relaxed).max(1),
        );
        self.offset_y = scrollbar_clamp_offset_f32(
            self.offset_y,
            self.content_height.load(Ordering::Relaxed).max(1),
            self.viewport_height.load(Ordering::Relaxed).max(1),
        );
    }

    fn apply_scrollbar_offset(&mut self, axis: ScrollbarAxis, offset: f32) -> bool {
        let (before_x, before_y) = (self.offset_x, self.offset_y);
        match axis {
            ScrollbarAxis::Horizontal => {
                self.offset_x = offset;
                self.anim_x.interrupted();
            }
            ScrollbarAxis::Vertical => {
                self.offset_y = offset;
                self.anim_y.interrupted();
            }
        }
        self.clamp_offsets();
        super::scroll_core::offset_moved((before_x, before_y), (self.offset_x, self.offset_y))
    }

    fn clamped_axis_offset(&self, axis: ScrollbarAxis, offset: f32) -> f32 {
        match axis {
            ScrollbarAxis::Horizontal => scrollbar_clamp_offset_f32(
                offset,
                self.content_width.load(Ordering::Relaxed).max(1),
                self.viewport_width.load(Ordering::Relaxed).max(1),
            ),
            ScrollbarAxis::Vertical => scrollbar_clamp_offset_f32(
                offset,
                self.content_height.load(Ordering::Relaxed).max(1),
                self.viewport_height.load(Ordering::Relaxed).max(1),
            ),
        }
    }

    fn axis_offset(&self, axis: ScrollbarAxis) -> f32 {
        match axis {
            ScrollbarAxis::Horizontal => self.offset_x,
            ScrollbarAxis::Vertical => self.offset_y,
        }
    }

    /// Apply a step of a scroll animation this root asked for, or stop the
    /// animation when another scroll moved the offset since.
    fn apply_animation_step(
        &mut self,
        axis: ScrollbarAxis,
        value: f32,
        done: bool,
        ctx: &mut crate::event::WidgetCtx,
    ) {
        use crate::widgets::scrollbar::ScrollStep;
        let (offset, anim, attribute) = match axis {
            ScrollbarAxis::Horizontal => (self.offset_x, &mut self.anim_x, APP_ROOT_OFFSET_X_ATTR),
            ScrollbarAxis::Vertical => (self.offset_y, &mut self.anim_y, APP_ROOT_OFFSET_Y_ATTR),
        };
        match anim.step(offset, value, done) {
            ScrollStep::Apply(value) => {
                let next = self.clamped_axis_offset(axis, value);
                let current = self.axis_offset(axis);
                if (next - current).abs() > f32::EPSILON {
                    match axis {
                        ScrollbarAxis::Horizontal => self.offset_x = next,
                        ScrollbarAxis::Vertical => self.offset_y = next,
                    }
                    ctx.request_layout_invalidation();
                }
            }
            ScrollStep::Stop => ctx.request_animation(
                crate::widgets::scrollbar::stop_scroll_animation(self.node_id(), attribute, offset),
            ),
        }
    }

    /// Ask for an animated scroll of `axis` to `to`, over `duration` or at
    /// Python's scroll speed. Returns whether the offset will change.
    fn request_scroll_animation(
        &mut self,
        axis: ScrollbarAxis,
        to: f32,
        duration: Option<Duration>,
        ctx: &mut crate::event::WidgetCtx,
    ) -> bool {
        let from = self.axis_offset(axis);
        let to = self.clamped_axis_offset(axis, to);
        if (to - from).abs() <= f32::EPSILON {
            return false;
        }
        let attr = match axis {
            ScrollbarAxis::Horizontal => APP_ROOT_OFFSET_X_ATTR,
            ScrollbarAxis::Vertical => APP_ROOT_OFFSET_Y_ATTR,
        };
        ctx.request_animation(crate::widgets::scrollbar::scroll_animation(
            self.node_id(),
            attr,
            from,
            to,
            duration,
        ));
        match axis {
            ScrollbarAxis::Horizontal => self.anim_x.started(from, to),
            ScrollbarAxis::Vertical => self.anim_y.started(from, to),
        }
        true
    }
}

impl Default for AppRoot {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::widgets::Interactive for AppRoot {
    fn on_mount(&mut self, _ctx: &mut crate::event::WidgetCtx) {}

    fn on_unmount(&mut self) {}

    fn on_tick(&mut self, _tick: u64) {}

    fn on_resize(&mut self, width: u16, height: u16) {
        crate::widgets::Interactive::on_layout(self, width, height);
    }

    fn on_layout(&mut self, width: u16, height: u16) {
        self.last_layout_height = height.max(1);
        self.last_layout_width = width.max(1);
        self.viewport_width
            .store(self.last_layout_width as usize, Ordering::Relaxed);
        self.viewport_height
            .store(self.last_layout_height as usize, Ordering::Relaxed);
        if scrollbar_drag_trace_enabled() {
            debug_input(&format!(
                "[app-root-layout] self=0x{:x} node={} layout={}x{}",
                std::ptr::from_ref(self) as usize,
                crate::node_id::node_id_to_ffi(self.node_id()),
                self.last_layout_width,
                self.last_layout_height
            ));
        }
        self.clamp_offsets();
    }

    fn on_event_capture(&mut self, _event: &Event, _ctx: &mut crate::event::WidgetCtx) {}

    fn on_event(&mut self, event: &Event, ctx: &mut crate::event::WidgetCtx) {
        if let Event::AnimationValue(AnimationValueEvent {
            target,
            attribute,
            value,
            done,
        }) = event
        {
            let axis = match attribute.as_str() {
                APP_ROOT_OFFSET_Y_ATTR => Some(ScrollbarAxis::Vertical),
                APP_ROOT_OFFSET_X_ATTR => Some(ScrollbarAxis::Horizontal),
                _ => None,
            };
            if let Some(axis) = axis
                && *target == self.node_id()
            {
                self.apply_animation_step(axis, *value, *done, ctx);
                ctx.set_handled();
                return;
            }
        }

        let Event::Action(action) = event else {
            return;
        };
        if crate::widgets::scrollbar::is_scroll_action(*action) {
            // A key scrolls from where a running animation is heading.
            self.offset_x = self.anim_x.target(self.offset_x);
            self.offset_y = self.anim_y.target(self.offset_y);
            self.anim_x.interrupted();
            self.anim_y.interrupted();
        }

        let before_x = self.offset_x;
        let before_y = self.offset_y;
        match action {
            crate::event::Action::ScrollHome => self.offset_y = 0.0,
            crate::event::Action::ScrollEnd => self.offset_y = self.max_offset_y(),
            crate::event::Action::ScrollUp => {
                self.offset_y = (self.offset_y - self.scroll_step_y.to_f32_lossy()).max(0.0);
            }
            crate::event::Action::ScrollDown => {
                self.offset_y += self.scroll_step_y.to_f32_lossy();
            }
            crate::event::Action::ScrollPageUp => {
                let page = self.viewport_height.load(Ordering::Relaxed).max(1);
                self.offset_y = (self.offset_y - page.to_f32_lossy()).max(0.0);
            }
            crate::event::Action::ScrollPageDown => {
                let page = self.viewport_height.load(Ordering::Relaxed).max(1);
                self.offset_y += page.to_f32_lossy();
            }
            crate::event::Action::ScrollLeft => {
                self.offset_x = (self.offset_x - self.scroll_step_x.to_f32_lossy()).max(0.0);
            }
            crate::event::Action::ScrollRight => {
                self.offset_x += self.scroll_step_x.to_f32_lossy();
            }
            crate::event::Action::ScrollPageLeft => {
                let page = self.viewport_width.load(Ordering::Relaxed).max(1);
                self.offset_x = (self.offset_x - page.to_f32_lossy()).max(0.0);
            }
            crate::event::Action::ScrollPageRight => {
                let page = self.viewport_width.load(Ordering::Relaxed).max(1);
                self.offset_x += page.to_f32_lossy();
            }
            _ => return,
        }
        self.clamp_offsets();

        if super::scroll_core::offset_moved((before_x, before_y), (self.offset_x, self.offset_y)) {
            // Root scrolling can move large portions of the composed frame
            // (content + scrollbar thumbs + dock interactions). Request a
            // full-frame invalidation to avoid stale partial-region artifacts.
            ctx.request_layout_invalidation();
            ctx.set_handled();
        }
    }

    fn on_message(&mut self, msg: &MessageEvent, ctx: &mut crate::event::WidgetCtx) {
        let Some(ScrollbarScrollTo {
            axis,
            offset,
            animate,
            scroll_duration,
        }) = msg.downcast_ref::<ScrollbarScrollTo>()
        else {
            return;
        };
        let changed = if *animate {
            self.request_scroll_animation(*axis, *offset, *scroll_duration, ctx)
        } else {
            self.apply_scrollbar_offset(*axis, *offset)
        };
        if changed {
            ctx.request_layout_invalidation();
        }
        ctx.set_handled();
    }

    fn on_mouse_move(&mut self, x: u16, y: u16) -> bool {
        let _ = (x, y);
        false
    }
}

impl crate::widgets::Layout for AppRoot {
    fn set_virtual_content_size(&mut self, width: usize, height: usize) {
        AppRoot::set_virtual_content_size(self, width, height);
    }

    fn layout_height(&self) -> Option<usize> {
        None
    }

    fn content_width(&self) -> Option<usize> {
        None
    }
}

impl crate::widgets::Scrollable for AppRoot {
    fn on_mouse_scroll(&mut self, delta_x: i32, delta_y: i32, ctx: &mut crate::event::WidgetCtx) {
        let before_x = self.offset_x;
        let before_y = self.offset_y;

        // The deltas are lines and columns already; the scroll steps are
        // for keys. A horizontal notch animates, as in Python.
        // Each notch scrolls from where a running animation is heading, so
        // it is not lost, and quick horizontal notches add up.
        if delta_x != 0 && crate::runtime::dispatch_ctx::wheel_notch_animates() {
            let to = self.anim_x.target(self.offset_x) + delta_x.to_f32_lossy();
            if self.request_scroll_animation(ScrollbarAxis::Horizontal, to, None, ctx) {
                ctx.set_handled();
            }
            return;
        }
        if delta_y != 0 {
            self.offset_y = self.anim_y.target(self.offset_y) + delta_y.to_f32_lossy();
            self.anim_y.interrupted();
        }
        if delta_x != 0 {
            self.offset_x = self.anim_x.target(self.offset_x) + delta_x.to_f32_lossy();
            self.anim_x.interrupted();
        }
        self.clamp_offsets();

        if super::scroll_core::offset_moved((before_x, before_y), (self.offset_x, self.offset_y)) {
            // Root scrolling can move large portions of the composed frame
            // (content + scrollbar thumbs + dock interactions). Request a
            // full-frame invalidation to avoid stale partial-region artifacts.
            ctx.request_layout_invalidation();
            ctx.set_handled();
        }
    }

    fn scroll_offset(&self) -> (usize, usize) {
        (
            scrollbar_clamp_offset_f32(
                self.offset_x,
                self.content_width.load(Ordering::Relaxed).max(1),
                self.viewport_width.load(Ordering::Relaxed).max(1),
            )
            .round()
            .to_usize_sat(),
            scrollbar_clamp_offset_f32(
                self.offset_y,
                self.content_height.load(Ordering::Relaxed).max(1),
                self.viewport_height.load(Ordering::Relaxed).max(1),
            )
            .round()
            .to_usize_sat(),
        )
    }

    fn scroll_offset_f32(&self) -> (f32, f32) {
        (
            scrollbar_clamp_offset_f32(
                self.offset_x,
                self.content_width.load(Ordering::Relaxed).max(1),
                self.viewport_width.load(Ordering::Relaxed).max(1),
            ),
            scrollbar_clamp_offset_f32(
                self.offset_y,
                self.content_height.load(Ordering::Relaxed).max(1),
                self.viewport_height.load(Ordering::Relaxed).max(1),
            ),
        )
    }

    fn scroll_viewport_size(&self) -> Option<(usize, usize)> {
        let viewport_w = self.viewport_width.load(Ordering::Relaxed);
        let viewport_h = self.viewport_height.load(Ordering::Relaxed);
        if viewport_w == 0 || viewport_h == 0 {
            None
        } else {
            Some((viewport_w.max(1), viewport_h.max(1)))
        }
    }
}

impl crate::widgets::StyleIdentity for AppRoot {
    fn take_node_seed(&mut self) -> NodeSeed {
        std::mem::take(&mut self.seed)
    }

    fn set_inline_style(&mut self, style: crate::style::Style) {
        self.seed.styles.style = style;
    }

    crate::seed_style_identity_methods!();
}

impl crate::widgets::Render for AppRoot {
    fn compose(&mut self) -> ComposeResult {
        self.children_extracted = true;
        // User children first (with their with_compose id/class/handle-sink
        // metadata folded into each ChildDecl), then the dedicated scrollbar
        // lanes appended after — their positions never collide with the
        // user-children-keyed metadata.
        let mut decls = crate::compose::zip_child_decls(
            std::mem::take(&mut self.children),
            std::mem::take(&mut self.child_decl_meta),
            std::mem::take(&mut self.child_handle_sinks),
        );

        let mut vbar = ScrollBar::new(true, 2);
        vbar.seed.css_id = Some(APP_ROOT_VSCROLLBAR_ID.to_string());
        decls.push(crate::compose::ChildDecl::new(Box::new(vbar)));

        let mut hbar = ScrollBar::new(false, 1);
        hbar.seed.css_id = Some(APP_ROOT_HSCROLLBAR_ID.to_string());
        decls.push(crate::compose::ChildDecl::new(Box::new(hbar)));

        let mut corner = ScrollBarCorner::new();
        corner.seed.css_id = Some(APP_ROOT_SCROLLBAR_CORNER_ID.to_string());
        decls.push(crate::compose::ChildDecl::new(Box::new(corner)));

        // NOTE: the docked notification stack (`ToastRack`) is NOT injected
        // here. Python mounts a ToastRack on every Screen (`screen.py
        // `_extend_compose`), so the runtime mounts it as a per-tree system
        // child (`App::mount_system_toast_rack`) on the base tree and on every
        // pushed screen tree — injecting it from AppRoot::compose would give a
        // dead duplicate rack to every nested AppRoot used as a screen body.

        decls
    }

    fn render(&self, console: &Console, options: &ConsoleOptions) -> Segments {
        let _ = console;
        let width = options.size.0.max(1);
        let height = options.size.1.max(1);

        let meta = css::selector_meta_generic(self);
        let resolved = css::resolve_style(self, &meta);
        let raw_viewport_w = self.viewport_width.load(Ordering::Relaxed);
        let raw_viewport_h = self.viewport_height.load(Ordering::Relaxed);
        let viewport_w = if raw_viewport_w == 0 {
            width
        } else {
            raw_viewport_w
        }
        .max(1);
        let viewport_h = if raw_viewport_h == 0 {
            height
        } else {
            raw_viewport_h
        }
        .max(1);
        let content_w = self.content_width.load(Ordering::Relaxed).max(1);
        let content_h = self.content_height.load(Ordering::Relaxed).max(1);
        let clamped_offset_x = scrollbar_clamp_offset_f32(self.offset_x, content_w, viewport_w);
        let clamped_offset_y = scrollbar_clamp_offset_f32(self.offset_y, content_h, viewport_h);
        if scrollbar_drag_trace_enabled() {
            debug_input(&format!(
                "[app-root-geom] self=0x{:x} node={} widget={}x{} content={}x{} viewport={}x{} offsets=({:.3}, {:.3})",
                std::ptr::from_ref(self) as usize,
                crate::node_id::node_id_to_ffi(self.node_id()),
                width,
                height,
                content_w,
                content_h,
                viewport_w,
                viewport_h,
                clamped_offset_x,
                clamped_offset_y
            ));
        }

        // App/screen baseline surface is a concrete blank renderable using
        // the resolved background.
        let bg = resolved
            .bg
            .or_else(|| parse_color_like("$background"))
            .unwrap_or_else(|| crate::style::Color::rgb(0, 0, 0));
        let base_style = rich_rs::Style::new().with_bgcolor(bg.to_simple_opaque());

        let mut out = Segments::new();
        for row in 0..height {
            out.push(Segment::styled(" ".repeat(width), base_style));

            if row + 1 < height {
                out.push(Segment::line());
            }
        }

        out
    }

    fn render_with_debug(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        _debug: &DebugLayout,
    ) -> Segments {
        Widget::render(self, console, options)
    }

    fn style_type(&self) -> &'static str {
        "Screen"
    }

    fn style_type_aliases(&self) -> &[&'static str] {
        APP_ROOT_TYPE_ALIASES
    }
}
#[cfg(test)]
mod focus_tests {
    use super::*;
    use crate::css::{StyleSheet, set_style_context};
    use crate::event::EventCtx;
    use crate::widgets::containers::{Container, Panel, ScrollView};
    use crate::widgets::{Button, Horizontal, Input, ListView, VerticalScroll};
    use rich_rs::Console;

    #[test]
    fn focus_next_advances_after_set_focus_by_id() {
        use crate::widget_tree::WidgetTree;

        // Build a WidgetTree with two focusable Input widgets.
        let mut tree = WidgetTree::new();
        let root_id = tree.set_root(Box::new(AppRoot::new()));
        let container_id = tree.mount(root_id, Box::new(Container::new()));
        let first_id = tree.mount(
            container_id,
            Box::new(Input::new().with_placeholder("First")),
        );
        let second_id = tree.mount(
            container_id,
            Box::new(Input::new().with_placeholder("Second")),
        );

        // Collect focusable nodes via depth-first walk.
        let ids: Vec<_> = tree
            .walk_depth_first(root_id)
            .into_iter()
            .filter(|&id| tree.get(id).is_some_and(|n| n.widget.focusable()))
            .collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], first_id);
        assert_eq!(ids[1], second_id);

        // Set focus on the first input.
        tree.set_focus_state(first_id, true);
        assert!(tree.node_state(first_id).focused);

        // Advance focus: find current in chain, move to next.
        let current = ids.iter().position(|&id| id == first_id).unwrap();
        let next = ids[(current + 1) % ids.len()];
        tree.set_focus_state(first_id, false);
        tree.set_focus_state(next, true);

        assert_eq!(next, second_id);
        assert!(tree.node_state(second_id).focused);
        assert!(!tree.node_state(first_id).focused);
    }

    #[test]
    fn scroll_view_handles_mouse_scroll_without_focus() {
        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (12, 3);
        options.max_width = 12;
        options.max_height = 3;

        let list = ListView::new(vec![
            "item 1".to_string(),
            "item 2".to_string(),
            "item 3".to_string(),
            "item 4".to_string(),
            "item 5".to_string(),
        ]);
        let mut scroll = ScrollView::new(list).height(3);
        let _ = Widget::render(&scroll, &console, &options);

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            scroll.on_mouse_scroll(0, 1, &mut __w);
        }
        assert!(ctx.handled());
        assert_eq!(scroll.offset_y, 1);
    }

    #[test]
    fn scroll_view_action_emits_offset_animation_requests_when_transition_enabled() {
        let _guard = set_style_context(StyleSheet::parse(
            "ScrollView > .scrollview--content { transition: scrollview.offset 120ms ease-out; }",
        ));
        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (12, 3);
        options.max_width = 12;
        options.max_height = 3;

        let list = ListView::new(vec![
            "item 1".to_string(),
            "item 2".to_string(),
            "item 3".to_string(),
            "item 4".to_string(),
            "item 5".to_string(),
        ]);
        let mut scroll = ScrollView::new(list).height(3);
        let _ = Widget::render(&scroll, &console, &options);

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            scroll.on_event(&Event::Action(Action::ScrollDown), &mut __w);
        }
        let requests = ctx.take_animation_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].attribute, ScrollView::OFFSET_Y_ATTR);
        assert!(
            requests[0].start.abs() < f32::EPSILON,
            "start = {}",
            requests[0].start
        );
        assert!(
            (requests[0].end - 1.0).abs() < f32::EPSILON,
            "end = {}",
            requests[0].end
        );
    }

    #[test]
    fn panel_forwards_action_to_scrollview_child() {
        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (14, 6);
        options.max_width = 14;
        options.max_height = 6;

        let list = ListView::new(vec![
            "item 1".to_string(),
            "item 2".to_string(),
            "item 3".to_string(),
            "item 4".to_string(),
            "item 5".to_string(),
        ]);
        let mut panel = Panel::new(ScrollView::new(list).height(3)).padding(1);
        let _ = Widget::render(&panel, &console, &options);

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            panel.on_event(&Event::Action(Action::ScrollDown), &mut __w);
        }
        assert!(ctx.handled());
    }

    #[test]
    fn panel_forwards_mouse_scroll_to_scrollview_child() {
        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (14, 6);
        options.max_width = 14;
        options.max_height = 6;

        let list = ListView::new(vec![
            "item 1".to_string(),
            "item 2".to_string(),
            "item 3".to_string(),
            "item 4".to_string(),
            "item 5".to_string(),
        ]);
        let mut panel = Panel::new(ScrollView::new(list).height(3)).padding(1);
        let _ = Widget::render(&panel, &console, &options);

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            panel.on_mouse_scroll(0, 1, &mut __w);
        }
        assert!(ctx.handled());
    }

    #[test]
    fn scroll_view_ignores_trailing_blank_probe_lines_for_fill_layouts() {
        use std::sync::atomic::Ordering;
        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (48, 12);
        options.max_width = 48;
        options.max_height = 12;

        let columns =
            Horizontal::new().with_child(VerticalScroll::new().with_child(Button::new("One")));
        let scroll = ScrollView::new(columns);
        let _ = Widget::render(&scroll, &console, &options);

        assert_eq!(
            scroll.viewport_width.load(Ordering::Relaxed),
            48,
            "false vertical scrollbar shrank viewport width"
        );
    }

    #[test]
    fn app_root_tree_mode_render_returns_chrome() {
        let mut root = AppRoot::new().with_child(Button::new("ok"));
        let _ = root.compose();

        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (10, 4);
        options.max_width = 10;
        options.max_height = 4;
        let segments = Widget::render(&root, &console, &options);
        assert!(!segments.is_empty());
    }

    #[test]
    fn app_root_tree_mode_on_event_does_not_panic() {
        let mut root = AppRoot::new().with_child(Button::new("ok"));
        let _ = root.compose();

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            root.on_event(&Event::Action(Action::FocusNext), &mut __w);
        }
        // In tree mode, events are a no-op — not handled.
        assert!(!ctx.handled());
    }

    #[test]
    fn app_root_tree_mode_mouse_move_returns_false() {
        let mut root = AppRoot::new().with_child(Button::new("ok"));
        let _ = root.compose();
        root.on_layout(80, 24);
        assert!(!root.on_mouse_move(5, 5));
    }

    #[test]
    fn app_root_matches_screen_selector_type() {
        let root = AppRoot::new();
        assert_eq!(root.style_type(), "Screen");
        assert!(
            root.style_type_aliases().contains(&"AppRoot"),
            "AppRoot alias should remain available for compatibility selectors"
        );
        assert!(
            root.style_type_aliases().contains(&"App"),
            "App alias enables canonical App:* selectors from Python defaults"
        );
    }

    #[test]
    fn app_root_mouse_scroll_updates_root_offset() {
        let mut root = AppRoot::new();
        root.on_layout(40, 6);
        root.set_virtual_content_size(40, 60);

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            root.on_mouse_scroll(0, 1, &mut __w);
        }

        assert!(ctx.handled());
        assert_eq!(root.scroll_offset(), (0, 1));
    }

    #[test]
    fn app_root_scrollbar_message_updates_vertical_offset() {
        let mut root = AppRoot::new();
        root.on_layout(20, 10);
        root.set_virtual_content_size(20, 200);

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            root.on_message(
                &MessageEvent::new(
                    NodeId::default(),
                    ScrollbarScrollTo {
                        axis: ScrollbarAxis::Vertical,
                        offset: 24.0,
                        animate: false,
                        scroll_duration: None,
                    },
                )
                .with_control(NodeId::default()),
                &mut __w,
            );
        }

        assert!(
            ctx.handled(),
            "scrollbar message should be handled by app root"
        );
        assert!(
            ctx.invalidation().layout,
            "scrollbar message should request layout invalidation"
        );
        assert_eq!(
            root.scroll_offset(),
            (0, 24),
            "scrollbar message should set vertical scroll offset"
        );
    }

    #[test]
    fn app_root_scrollbar_message_requests_animation_when_enabled() {
        let mut root = AppRoot::new();
        root.on_layout(20, 10);
        root.set_virtual_content_size(20, 200);

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            root.on_message(
                &MessageEvent::new(
                    NodeId::default(),
                    ScrollbarScrollTo {
                        axis: ScrollbarAxis::Vertical,
                        offset: 24.5,
                        animate: true,
                        scroll_duration: None,
                    },
                )
                .with_control(NodeId::default()),
                &mut __w,
            );
        }

        assert!(ctx.handled());
        let offset_y = root.scroll_offset_f32().1;
        assert!(
            offset_y.abs() < f32::EPSILON,
            "animated message should not jump offset immediately (offset_y = {offset_y})"
        );
        let requests = ctx.take_animation_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].target, root.node_id());
        assert_eq!(requests[0].attribute, APP_ROOT_OFFSET_Y_ATTR);
        assert!(
            requests[0].start.abs() < f32::EPSILON,
            "start = {}",
            requests[0].start
        );
        assert!(
            (requests[0].end - 24.5).abs() < f32::EPSILON,
            "end = {}",
            requests[0].end
        );
    }

    #[test]
    fn app_root_scrollbar_message_clamps_to_bounds() {
        let mut root = AppRoot::new();
        root.on_layout(20, 10);
        root.set_virtual_content_size(20, 35);

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            root.on_message(
                &MessageEvent::new(
                    NodeId::default(),
                    ScrollbarScrollTo {
                        axis: ScrollbarAxis::Vertical,
                        offset: 999.0,
                        animate: false,
                        scroll_duration: None,
                    },
                )
                .with_control(NodeId::default()),
                &mut __w,
            );
        }

        assert!(ctx.handled());
        assert_eq!(
            root.scroll_offset().1,
            25,
            "offset should clamp to max(content - viewport)"
        );
    }

    #[test]
    fn app_root_scrollbar_message_preserves_fractional_offset() {
        let mut root = AppRoot::new();
        root.on_layout(20, 10);
        root.set_virtual_content_size(20, 200);

        let mut ctx = EventCtx::default();
        {
            let mut __w = crate::event::WidgetCtx::__from_dispatch(
                crate::node_id::NodeId::default(),
                &mut ctx,
            );
            root.on_message(
                &MessageEvent::new(
                    NodeId::default(),
                    ScrollbarScrollTo {
                        axis: ScrollbarAxis::Vertical,
                        offset: 24.5,
                        animate: false,
                        scroll_duration: None,
                    },
                )
                .with_control(NodeId::default()),
                &mut __w,
            );
        }

        assert!(ctx.handled());
        let offset_y = root.scroll_offset_f32().1;
        assert!(
            (offset_y - 24.5).abs() < f32::EPSILON,
            "offset_y = {offset_y}"
        );
        assert_eq!(root.scroll_offset().1, 25);
    }

    /// An animated scroll of `axis` to `offset` that a scrollbar asks for.
    fn scrollbar_scroll(axis: ScrollbarAxis, offset: f32) -> MessageEvent {
        MessageEvent::new(
            NodeId::default(),
            ScrollbarScrollTo {
                axis,
                offset,
                animate: true,
                scroll_duration: None,
            },
        )
        .with_control(NodeId::default())
    }

    #[test]
    fn scl_002_the_app_screen_pages_at_python_speed_and_animates_a_horizontal_notch() {
        let mut root = AppRoot::new();
        root.on_layout(20, 10);
        root.set_virtual_content_size(200, 200);
        let mut ctx = EventCtx::default();
        {
            let mut w = crate::event::WidgetCtx::__from_dispatch(NodeId::default(), &mut ctx);
            root.on_message(&scrollbar_scroll(ScrollbarAxis::Vertical, 24.0), &mut w);
        }
        let requests = ctx.take_animation_requests();
        crate::widgets::scrollbar::assert_python_scroll(&requests[0], 24.0, None);

        let mut ctx = EventCtx::default();
        {
            let _animates = crate::runtime::dispatch_ctx::set_wheel_animates(true);
            let mut w = crate::event::WidgetCtx::__from_dispatch(NodeId::default(), &mut ctx);
            crate::widgets::Scrollable::on_mouse_scroll(&mut root, 4, 0, &mut w);
        }
        assert!(ctx.handled());
        let requests = ctx.take_animation_requests();
        assert_eq!(requests[0].attribute, APP_ROOT_OFFSET_X_ATTR);
        crate::widgets::scrollbar::assert_python_scroll(&requests[0], 4.0, None);
        assert_eq!(
            root.scroll_offset(),
            (0, 0),
            "the notch animates, it does not jump"
        );
    }

    #[test]
    fn scl_002_a_scroll_view_pages_at_python_speed_and_animates_a_horizontal_notch() {
        let console = Console::new();
        let mut options = console.options().clone();
        options.size = (12, 5);
        options.max_width = 12;
        options.max_height = 5;
        let text: Vec<String> = (0..30)
            .map(|n| format!("line {n} {}", "x".repeat(40)))
            .collect();
        let mut scroll = ScrollView::new(crate::widgets::Static::new(text.join("\n"))).height(5);
        let _ = Widget::render(&scroll, &console, &options);
        let mut ctx = EventCtx::default();
        {
            let mut w = crate::event::WidgetCtx::__from_dispatch(NodeId::default(), &mut ctx);
            scroll.on_message(&scrollbar_scroll(ScrollbarAxis::Vertical, 5.0), &mut w);
        }
        let requests = ctx.take_animation_requests();
        assert_eq!(requests[0].attribute, ScrollView::OFFSET_Y_ATTR);
        crate::widgets::scrollbar::assert_python_scroll(&requests[0], 5.0, None);

        // Wider content than the view, as a line that does not wrap makes it.
        scroll.content_width.store(100, Ordering::Relaxed);
        scroll.viewport_width.store(12, Ordering::Relaxed);
        let mut ctx = EventCtx::default();
        {
            let _animates = crate::runtime::dispatch_ctx::set_wheel_animates(true);
            let mut w = crate::event::WidgetCtx::__from_dispatch(NodeId::default(), &mut ctx);
            crate::widgets::Scrollable::on_mouse_scroll(&mut scroll, 4, 0, &mut w);
        }
        let requests = ctx.take_animation_requests();
        assert_eq!(requests.len(), 1, "one animated horizontal scroll");
        crate::widgets::scrollbar::assert_python_scroll(&requests[0], 4.0, None);
    }
}
