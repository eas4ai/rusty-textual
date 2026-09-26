use super::scroll_view::ScrollView;
use crate::event::Action;
use crate::message::ScrollbarAxis;
use crate::num::Cast;
use crate::style::{Overflow, Style};
use crate::widgets::scrollbar;

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

    /// Scroll by mouse wheel notches, on the axes that allow scrolling.
    /// Returns whether the offset moved.
    pub(crate) fn scroll_by(&mut self, delta_x: i32, delta_y: i32) -> bool {
        let before = (self.offset_x, self.offset_y);
        if delta_y != 0 && self.scrollable_y() {
            self.offset_y += delta_y
                .saturating_mul(self.step_y.to_i32_sat())
                .to_f32_lossy();
        }
        if delta_x != 0 && self.scrollable_x() {
            self.offset_x += delta_x
                .saturating_mul(self.step_x.to_i32_sat())
                .to_f32_lossy();
        }
        self.clamp();
        offset_moved(before, (self.offset_x, self.offset_y))
    }

    /// Scroll one axis to `offset`, as a scrollbar drag asks. Returns whether
    /// the offset moved.
    pub(crate) fn scroll_to(&mut self, axis: ScrollbarAxis, offset: f32) -> bool {
        let before = (self.offset_x, self.offset_y);
        match axis {
            ScrollbarAxis::Horizontal => self.offset_x = offset,
            ScrollbarAxis::Vertical => self.offset_y = offset,
        }
        self.clamp();
        offset_moved(before, (self.offset_x, self.offset_y))
    }

    /// Apply a scroll key action. Returns whether the offset moved, or
    /// `None` when `action` is not a scroll action.
    pub(crate) fn scroll_action(&mut self, action: Action) -> Option<bool> {
        let before = (self.offset_x, self.offset_y);
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
