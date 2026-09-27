use std::time::Duration;

use super::scroll_view::ScrollView;
use crate::event::{Action, AnimationValueEvent, Event};
use crate::message::ScrollbarAxis;
use crate::node_id::NodeId;
use crate::num::Cast;
use crate::style::{Overflow, Style};
use crate::widgets::scrollbar::{self, ScrollAnimationState, ScrollStep};

/// Whether a scroll changed the `(x, y)` offset. Exact on purpose: any
/// change, however small, must trigger a repaint.
#[allow(clippy::float_cmp)]
pub(crate) fn offset_moved(before: (f32, f32), after: (f32, f32)) -> bool {
    before.0 != after.0 || before.1 != after.1
}

/// Shared scroll math helpers used across scroll container wrappers.
///
/// Transitional extraction: this centralizes the line-based helpers previously
/// consumed ad-hoc via `ScrollView::*` associated functions.
pub struct ScrollCore;

impl ScrollCore {
    #[must_use]
    pub fn max_offset(content_len: usize, viewport_len: usize) -> usize {
        scrollbar::max_offset(content_len, viewport_len)
    }

    #[must_use]
    pub fn clamp_offset(offset: usize, content_len: usize, viewport_len: usize) -> usize {
        scrollbar::clamp_offset(offset, content_len, viewport_len)
    }

    #[must_use]
    pub fn scroll_by(offset: usize, delta: i32, content_len: usize, viewport_len: usize) -> usize {
        scrollbar::scroll_by(offset, delta, content_len, viewport_len)
    }

    #[must_use]
    pub fn scroll_end(content_len: usize, viewport_len: usize) -> usize {
        scrollbar::scroll_end(content_len, viewport_len)
    }

    #[must_use]
    pub fn thumb(
        track_len: usize,
        content_len: usize,
        viewport_len: usize,
        offset: usize,
    ) -> (usize, usize) {
        scrollbar::thumb_range(track_len, content_len, viewport_len, offset)
    }

    #[must_use]
    pub fn drag_offset(
        pointer: usize,
        grab_offset: usize,
        track_len: usize,
        content_len: usize,
        viewport_len: usize,
        current_offset: usize,
    ) -> usize {
        scrollbar::drag_to_offset(
            pointer,
            grab_offset,
            track_len,
            content_len,
            viewport_len,
            current_offset,
        )
    }

    #[must_use]
    pub fn scrollbar_styles() -> (rich_rs::Style, rich_rs::Style, rich_rs::Style) {
        ScrollView::line_scrollbar_styles()
    }
}

/// The animated attributes of a [`ScrollHost`]'s offsets.
const SCROLL_HOST_OFFSET_X_ATTR: &str = "scrollhost.offset_x";
const SCROLL_HOST_OFFSET_Y_ATTR: &str = "scrollhost.offset_y";

/// Scroll state of a node that scrolls its own children: a plain
/// `Container`, and the root of a pushed screen (Python `Screen` and
/// `ModalScreen` set `overflow-y: auto`, `screen.py:174-188, 2164-2172`).
///
/// The runtime's layout pass mounts the node's scrollbar lanes, reports the
/// virtual content size (`set_content_size`) and the viewport left after the
/// scrollbar gutter (`layout`). The host is active, and scrolls, only on an
/// axis whose resolved overflow is `auto` or `scroll`. Callers check
/// [`ScrollHost::is_active`] before reporting its offset or viewport, or
/// before scrolling it.
pub(crate) struct ScrollHost {
    offset_x: f32,
    offset_y: f32,
    /// The running scroll animations, per axis.
    anim_x: ScrollAnimationState,
    anim_y: ScrollAnimationState,
    step_x: usize,
    step_y: usize,
    content_width: usize,
    content_height: usize,
    viewport_width: usize,
    viewport_height: usize,
    overflow_x: Overflow,
    overflow_y: Overflow,
}

fn clamp_offset_f32(offset: f32, content_len: usize, viewport_len: usize) -> f32 {
    if !offset.is_finite() {
        return 0.0;
    }
    let max = scrollbar::max_offset(content_len.max(1), viewport_len.max(1)).to_f32_lossy();
    offset.clamp(0.0, max)
}

impl Default for ScrollHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollHost {
    /// An inactive host: no viewport yet, and `overflow: hidden hidden`
    /// (the Python `Container` default) until `layout` says otherwise.
    pub(crate) fn new() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            anim_x: ScrollAnimationState::default(),
            anim_y: ScrollAnimationState::default(),
            step_x: 2,
            step_y: 1,
            content_width: 0,
            content_height: 0,
            viewport_width: 0,
            viewport_height: 0,
            overflow_x: Overflow::Hidden,
            overflow_y: Overflow::Hidden,
        }
    }

    /// Record the viewport and the overflow `style` resolves per axis
    /// (`hidden` when unset), then clamp the offset to the new extent.
    pub(crate) fn layout(&mut self, width: u16, height: u16, style: &Style) {
        self.viewport_width = usize::from(width.max(1));
        self.viewport_height = usize::from(height.max(1));
        let fallback = style.overflow.unwrap_or(Overflow::Hidden);
        self.overflow_x = style.overflow_x.unwrap_or(fallback);
        self.overflow_y = style.overflow_y.unwrap_or(fallback);
        self.clamp();
    }

    /// Record the size of the content the host scrolls over.
    pub(crate) fn set_content_size(&mut self, width: usize, height: usize) {
        self.content_width = width.max(1);
        self.content_height = height.max(1);
    }

    fn scrollable_x(&self) -> bool {
        matches!(self.overflow_x, Overflow::Auto | Overflow::Scroll)
    }

    fn scrollable_y(&self) -> bool {
        matches!(self.overflow_y, Overflow::Auto | Overflow::Scroll)
    }

    /// Whether the host scrolls: an axis allows it and a viewport is known.
    pub(crate) fn is_active(&self) -> bool {
        (self.scrollable_x() || self.scrollable_y())
            && self.viewport_width > 0
            && self.viewport_height > 0
    }

    fn clamp(&mut self) {
        self.offset_x = clamp_offset_f32(self.offset_x, self.content_width, self.viewport_width);
        self.offset_y = clamp_offset_f32(self.offset_y, self.content_height, self.viewport_height);
    }

    /// The scroll offset, clamped to the content.
    pub(crate) fn offset(&self) -> (f32, f32) {
        (
            clamp_offset_f32(self.offset_x, self.content_width, self.viewport_width),
            clamp_offset_f32(self.offset_y, self.content_height, self.viewport_height),
        )
    }

    /// The viewport size.
    pub(crate) fn viewport(&self) -> (usize, usize) {
        (self.viewport_width.max(1), self.viewport_height.max(1))
    }

    /// The content size, once the layout pass has reported one.
    pub(crate) fn content_size(&self) -> Option<(usize, usize)> {
        if self.content_width == 0 || self.content_height == 0 {
            None
        } else {
            Some((self.content_width, self.content_height))
        }
    }

    /// Scroll by `delta_x` columns and `delta_y` lines, as a mouse wheel
    /// notch asks, on the axes that allow scrolling. Returns whether the
    /// offset moved. A running animation on an axis is finished first: the
    /// notch scrolls from where it was heading, as in Python.
    pub(crate) fn scroll_by(&mut self, delta_x: i32, delta_y: i32) -> bool {
        let before = (self.offset_x, self.offset_y);
        if delta_y != 0 && self.scrollable_y() {
            self.offset_y = self.anim_y.target(self.offset_y) + delta_y.to_f32_lossy();
            self.anim_y.interrupted();
        }
        if delta_x != 0 && self.scrollable_x() {
            self.offset_x = self.anim_x.target(self.offset_x) + delta_x.to_f32_lossy();
            self.anim_x.interrupted();
        }
        self.clamp();
        offset_moved(before, (self.offset_x, self.offset_y))
    }

    /// Ask for an animated scroll of one axis to `offset`, as a scrollbar
    /// asks: over `duration`, else at Python's scroll speed. `node` is the
    /// host's node, which receives the animation's steps (see
    /// [`ScrollHost::apply_animation`]). Returns whether the offset will
    /// change.
    pub(crate) fn animate_to(
        &mut self,
        node: NodeId,
        axis: ScrollbarAxis,
        offset: f32,
        duration: Option<Duration>,
        ctx: &mut crate::event::WidgetCtx,
    ) -> bool {
        let (from, content, viewport, attribute) = match axis {
            ScrollbarAxis::Horizontal => (
                self.offset_x,
                self.content_width,
                self.viewport_width,
                SCROLL_HOST_OFFSET_X_ATTR,
            ),
            ScrollbarAxis::Vertical => (
                self.offset_y,
                self.content_height,
                self.viewport_height,
                SCROLL_HOST_OFFSET_Y_ATTR,
            ),
        };
        let to = clamp_offset_f32(offset, content, viewport);
        if (to - from).abs() <= f32::EPSILON {
            return false;
        }
        ctx.request_animation(scrollbar::scroll_animation(
            node, attribute, from, to, duration,
        ));
        match axis {
            ScrollbarAxis::Horizontal => self.anim_x.started(from, to),
            ScrollbarAxis::Vertical => self.anim_y.started(from, to),
        }
        true
    }

    /// Apply a step of an animation [`ScrollHost::animate_to`] asked for, or
    /// stop the animation when another scroll moved the offset since.
    /// Returns whether `event` was one for `node`.
    pub(crate) fn apply_animation(
        &mut self,
        node: NodeId,
        event: &Event,
        ctx: &mut crate::event::WidgetCtx,
    ) -> bool {
        let Event::AnimationValue(AnimationValueEvent {
            target,
            attribute,
            value,
            done,
        }) = event
        else {
            return false;
        };
        if *target != node {
            return false;
        }
        let (offset, anim, attribute) = match attribute.as_str() {
            SCROLL_HOST_OFFSET_X_ATTR => (
                &mut self.offset_x,
                &mut self.anim_x,
                SCROLL_HOST_OFFSET_X_ATTR,
            ),
            SCROLL_HOST_OFFSET_Y_ATTR => (
                &mut self.offset_y,
                &mut self.anim_y,
                SCROLL_HOST_OFFSET_Y_ATTR,
            ),
            _ => return false,
        };
        match anim.step(*offset, *value, *done) {
            ScrollStep::Apply(value) => *offset = value,
            ScrollStep::Stop => {
                ctx.request_animation(scrollbar::stop_scroll_animation(node, attribute, *offset));
            }
        }
        self.clamp();
        true
    }

    /// Scroll as a wheel notch of `delta_x` columns and `delta_y` lines
    /// asks: a horizontal notch with an animation, as in Python, the rest at
    /// once. Returns whether the offset moved or will move.
    pub(crate) fn wheel(
        &mut self,
        node: NodeId,
        delta_x: i32,
        delta_y: i32,
        ctx: &mut crate::event::WidgetCtx,
    ) -> bool {
        if delta_x != 0
            && self.scrollable_x()
            && crate::runtime::dispatch_ctx::wheel_notch_animates()
        {
            // From where a running animation is heading, so quick notches add up.
            let to = self.anim_x.target(self.offset_x) + delta_x.to_f32_lossy();
            return self.animate_to(node, ScrollbarAxis::Horizontal, to, None, ctx);
        }
        self.scroll_by(delta_x, delta_y)
    }

    /// Scroll one axis to `offset`, as a scrollbar drag asks. Returns whether
    /// the offset moved.
    pub(crate) fn scroll_to(&mut self, axis: ScrollbarAxis, offset: f32) -> bool {
        let before = (self.offset_x, self.offset_y);
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
        self.clamp();
        offset_moved(before, (self.offset_x, self.offset_y))
    }

    /// Apply a scroll key action. Returns whether the offset moved, or
    /// `None` when `action` is not a scroll action.
    pub(crate) fn scroll_action(&mut self, action: Action) -> Option<bool> {
        let before = (self.offset_x, self.offset_y);
        if scrollbar::is_scroll_action(action) {
            // A key scrolls from where a running animation is heading.
            self.offset_x = self.anim_x.target(self.offset_x);
            self.offset_y = self.anim_y.target(self.offset_y);
            self.anim_x.interrupted();
            self.anim_y.interrupted();
        }
        let (step_x, step_y) = (self.step_x.to_f32_lossy(), self.step_y.to_f32_lossy());
        let page_x = self.viewport_width.max(1).to_f32_lossy();
        let page_y = self.viewport_height.max(1).to_f32_lossy();
        match action {
            Action::ScrollHome => self.offset_y = 0.0,
            Action::ScrollEnd => {
                self.offset_y =
                    scrollbar::max_offset(self.content_height.max(1), self.viewport_height.max(1))
                        .to_f32_lossy();
            }
            Action::ScrollUp => self.offset_y = (self.offset_y - step_y).max(0.0),
            Action::ScrollDown => self.offset_y += step_y,
            Action::ScrollPageUp => self.offset_y = (self.offset_y - page_y).max(0.0),
            Action::ScrollPageDown => self.offset_y += page_y,
            Action::ScrollLeft => self.offset_x = (self.offset_x - step_x).max(0.0),
            Action::ScrollRight => self.offset_x += step_x,
            Action::ScrollPageLeft => self.offset_x = (self.offset_x - page_x).max(0.0),
            Action::ScrollPageRight => self.offset_x += page_x,
            _ => return None,
        }
        self.clamp();
        Some(offset_moved(before, (self.offset_x, self.offset_y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventCtx;
    use crate::widgets::scrollbar::assert_python_scroll;

    /// A host over 100 columns and 60 lines, 10 by 10 at a time.
    fn host() -> ScrollHost {
        let mut host = ScrollHost::new();
        let style = Style {
            overflow: Some(Overflow::Auto),
            ..Style::default()
        };
        host.layout(10, 10, &style);
        host.set_content_size(100, 60);
        host
    }

    #[test]
    fn scl_002_a_scroll_host_animates_a_scrollbar_scroll_and_a_horizontal_notch() {
        let node = NodeId::default();
        let mut host = host();
        let mut ctx = EventCtx::default();
        {
            let mut w = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
            assert!(host.animate_to(node, ScrollbarAxis::Vertical, 10.0, None, &mut w));
        }
        let requests = ctx.take_animation_requests();
        assert_eq!(requests[0].attribute, SCROLL_HOST_OFFSET_Y_ATTR);
        assert_python_scroll(&requests[0], 10.0, None);
        assert_eq!(host.offset(), (0.0, 0.0), "it animates, it does not jump");
        let step = Event::AnimationValue(AnimationValueEvent {
            target: node,
            attribute: SCROLL_HOST_OFFSET_Y_ATTR.to_string(),
            value: 4.0,
            done: false,
        });
        let mut ctx = EventCtx::default();
        {
            let mut w = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
            assert!(host.apply_animation(node, &step, &mut w));
        }
        assert_eq!(host.offset(), (0.0, 4.0));

        let mut ctx = EventCtx::default();
        {
            let _animates = crate::runtime::dispatch_ctx::set_wheel_animates(true);
            let mut w = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
            assert!(host.wheel(node, 4, 0, &mut w));
        }
        let requests = ctx.take_animation_requests();
        assert_eq!(requests[0].attribute, SCROLL_HOST_OFFSET_X_ATTR);
        assert_python_scroll(&requests[0], 4.0, None);

        // A vertical notch scrolls at once, from where the running page was
        // heading (10).
        let mut ctx = EventCtx::default();
        {
            let mut w = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
            assert!(host.wheel(node, 0, 2, &mut w));
        }
        assert!(ctx.take_animation_requests().is_empty());
        assert_eq!(host.offset(), (0.0, 12.0));
    }

    /// A step of `host`'s vertical animation to `value`; returns the
    /// animation requests it made.
    fn step_y(host: &mut ScrollHost, value: f32) -> Vec<crate::event::AnimationRequest> {
        let node = NodeId::default();
        let step = Event::AnimationValue(AnimationValueEvent {
            target: node,
            attribute: SCROLL_HOST_OFFSET_Y_ATTR.to_string(),
            value,
            done: false,
        });
        let mut ctx = EventCtx::default();
        {
            let mut w = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
            assert!(host.apply_animation(node, &step, &mut w));
        }
        ctx.take_animation_requests()
    }

    #[test]
    fn scl_001_a_notch_during_a_page_scrolls_2_lines_past_it() {
        // Python stops the running animation at its end, then scrolls from
        // `scroll_target_y` (`Widget._scroll_to`, `_scroll_down_for_pointer`).
        let node = NodeId::default();
        let mut host = host();
        let mut ctx = EventCtx::default();
        {
            let mut w = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
            assert!(host.animate_to(node, ScrollbarAxis::Vertical, 10.0, None, &mut w));
            assert!(step_y(&mut host, 3.0).is_empty());
            assert!(host.wheel(node, 0, 2, &mut w), "the notch is used");
        }
        assert_eq!(host.offset(), (0.0, 12.0), "2 lines past the page");
        // The page's next step finds the offset moved and stops it there.
        let stop = step_y(&mut host, 5.0);
        assert_eq!(host.offset(), (0.0, 12.0));
        assert_eq!(stop.len(), 1);
        assert!(stop[0].duration.is_zero());
        assert!((stop[0].end - 12.0).abs() < f32::EPSILON);
    }

    #[test]
    fn scl_001_quick_horizontal_notches_add_up() {
        // Python aims each notch at `scroll_target_x + 4`.
        let node = NodeId::default();
        let mut host = host();
        let mut ctx = EventCtx::default();
        {
            let _animates = crate::runtime::dispatch_ctx::set_wheel_animates(true);
            let mut w = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
            assert!(host.wheel(node, 4, 0, &mut w));
            assert!(host.wheel(node, 4, 0, &mut w));
        }
        let requests = ctx.take_animation_requests();
        assert_eq!(requests.len(), 2);
        assert!(
            (requests[1].end - 8.0).abs() < f32::EPSILON,
            "end {}",
            requests[1].end
        );
    }
}
