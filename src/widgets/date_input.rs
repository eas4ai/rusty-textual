//! Segmented date input with an inline spinner strip.
//!
//! A `DateInput` edits one calendar date through day/month/year segments in
//! locale order ([`DateOrder::Dmy`], [`DateOrder::Mdy`], [`DateOrder::Ymd`]);
//! the default is year / month / day ([`DateOrder::Ymd`]).
//! There are two entry paths, as designed:
//!
//! - **Direct entry**: typing digits fills an edit buffer for the focused
//!   segment (2 digits for day/month, 4 for year; a digit that would
//!   overflow the range restarts the segment). The buffer commits when it
//!   is full or when moving to another segment — partial input never touches
//!   committed state, so typing a year while Feb 29 is selected keeps day
//!   29 until the year actually commits. Leaving the widget (blur), picking
//!   the strip, or Escape discards partial input; Backspace pops one digit.
//! - **Spinner strip**: focusing the widget opens a strip below the input,
//!   the same width as the input, showing `prev | current | next` for the
//!   focused segment with `<` / `>` ends (`[< 29 | 30 | 31 >]`). Up/Down
//!   (or clicking the ends and values) steps with wrap; the current value is
//!   underlined, never hyphen-decorated.
//!
//! Left/Right moves between segments, Escape clears in-progress typing, and
//! every committed change posts [`DateChanged`]. Day ranges follow the
//! month and leap year (Feb 30 clamps to Feb 28/29 when the month changes).
use rich_rs::{Console, ConsoleOptions, Segment, Segments, Style};

use super::core::{NodeSeed, Widget};
use crate::event::{Event, WidgetCtx};
use crate::message::DateChanged;

/// Minimum selectable year.
pub const YEAR_MIN: i32 = 1900;
/// Maximum selectable year.
pub const YEAR_MAX: i32 = 2100;

/// Segment order (locale layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateOrder {
    /// Day / month / year.
    Dmy,
    /// Month / day / year.
    Mdy,
    /// Year / month / day (default).
    #[default]
    Ymd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentKind {
    Day,
    Month,
    Year,
}

/// Days in `month` of `year` (Gregorian leap-year rule).
#[must_use]
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        2 if is_leap_year(year) => 29,
        2 => 28,
        // April, June, September and November (and any out-of-range month).
        _ => 30,
    }
}

/// Gregorian leap-year rule.
#[must_use]
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Wrap `value` into `[min, max]` (both inclusive).
fn wrap_range(value: i32, min: i32, max: i32) -> i32 {
    let len = max - min + 1;
    ((value - min) % len + len) % len + min
}

/// A segmented date input. See the module docs for the interaction design.
pub struct DateInput {
    seed: NodeSeed,
    order: DateOrder,
    year: i32,
    month: u8,
    day: u8,
    /// Position index into the ordered segment list.
    seg: usize,
    /// In-progress direct-entry digits for the focused segment.
    typing: String,
    /// Whether the spinner strip is open (tracks focus).
    open: bool,
}

impl DateInput {
    #[must_use]
    pub fn new(year: i32, month: u8, day: u8) -> Self {
        let mut input = Self {
            seed: NodeSeed::default(),
            order: DateOrder::default(),
            year: year.clamp(YEAR_MIN, YEAR_MAX),
            month: month.clamp(1, 12),
            day: day.clamp(1, 31),
            seg: 0,
            typing: String::new(),
            open: false,
        };
        input.clamp_day();
        input
    }

    crate::seed_ident_methods!();

    /// Segment order (locale layout).
    #[must_use]
    pub fn order(mut self, order: DateOrder) -> Self {
        self.order = order;
        self
    }

    #[must_use]
    pub fn order_of(&self) -> DateOrder {
        self.order
    }

    /// Current date as `(year, month, day)`.
    #[must_use]
    pub fn date(&self) -> (i32, u8, u8) {
        (self.year, self.month, self.day)
    }

    /// Strip open state. Real focus transitions post Focus/Blur and flip
    /// this immediately; a silent mount auto-focus (live loop) opens the
    /// strip on first input via the focus-state sync in `on_event`.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Index of the focused segment (position in display order).
    #[must_use]
    pub fn focused_segment(&self) -> usize {
        self.seg
    }

    fn segments(&self) -> [SegmentKind; 3] {
        use SegmentKind as S;
        match self.order {
            DateOrder::Dmy => [S::Day, S::Month, S::Year],
            DateOrder::Mdy => [S::Month, S::Day, S::Year],
            DateOrder::Ymd => [S::Year, S::Month, S::Day],
        }
    }

    fn focused_kind(&self) -> SegmentKind {
        self.segments()[self.seg]
    }

    fn seg_value(&self, kind: SegmentKind) -> i32 {
        match kind {
            SegmentKind::Day => i32::from(self.day),
            SegmentKind::Month => i32::from(self.month),
            SegmentKind::Year => self.year,
        }
    }

    fn seg_range(&self, kind: SegmentKind) -> (i32, i32) {
        match kind {
            SegmentKind::Day => (1, i32::from(days_in_month(self.year, self.month))),
            SegmentKind::Month => (1, 12),
            SegmentKind::Year => (YEAR_MIN, YEAR_MAX),
        }
    }

    fn seg_width(kind: SegmentKind) -> usize {
        match kind {
            SegmentKind::Year => 4,
            _ => 2,
        }
    }

    fn fmt_seg(kind: SegmentKind, value: i32) -> String {
        match kind {
            SegmentKind::Year => format!("{value:04}"),
            _ => format!("{value:02}"),
        }
    }

    /// Clamp the day to its month (call after month/year writes).
    fn clamp_day(&mut self) {
        let max = days_in_month(self.year, self.month);
        if self.day > max {
            self.day = max;
        }
    }

    /// Write a segment value with wrap; returns whether anything changed.
    fn write_seg(&mut self, kind: SegmentKind, value: i32) -> bool {
        let (min, max) = self.seg_range(kind);
        let wrapped = wrap_range(value, min, max);
        let changed = self.seg_value(kind) != wrapped;
        match kind {
            SegmentKind::Day => self.day = wrapped as u8,
            SegmentKind::Month => self.month = wrapped as u8,
            SegmentKind::Year => self.year = wrapped,
        }
        self.clamp_day();
        changed
    }

    fn post_change(&self, ctx: &mut WidgetCtx) {
        ctx.post_message(DateChanged {
            year: self.year,
            month: self.month,
            day: self.day,
        });
    }

    fn changed(&mut self, ctx: &mut WidgetCtx) {
        self.post_change(ctx);
        ctx.request_repaint();
    }

    /// Step the focused segment by `dir` with wrap.
    fn step_focused(&mut self, ctx: &mut WidgetCtx, dir: i32) {
        let kind = self.focused_kind();
        self.typing.clear();
        if self.write_seg(kind, self.seg_value(kind) + dir) {
            self.changed(ctx);
        } else {
            ctx.request_repaint();
        }
    }

    /// Type one digit into the focused segment (direct entry).
    ///
    /// Digits accumulate in an edit buffer that is separate from the
    /// committed date: the segment commits when the buffer is full, or when
    /// focus leaves the segment ([`Self::commit_typing`]). Partial input
    /// never touches state, so typing a year while Feb 29 is selected keeps
    /// day 29 until the year actually commits (then [`Self::clamp_day`]
    /// revalidates once).
    fn type_digit(&mut self, ctx: &mut WidgetCtx, digit: char) {
        let kind = self.focused_kind();
        let width = Self::seg_width(kind);
        let (_, max) = self.seg_range(kind);
        if self.typing.len() >= width {
            self.typing.clear();
        }
        self.typing.push(digit);
        let mut parsed: i32 = self.typing.parse().unwrap_or(0);
        if parsed > max {
            // Overflow restarts the segment with just this digit.
            self.typing = digit.to_string();
            parsed = self.typing.parse().unwrap_or(0);
        }
        if self.typing.len() >= width {
            let (min, _) = self.seg_range(kind);
            if self.write_seg(kind, parsed.max(min)) {
                self.changed(ctx);
                return;
            }
        }
        ctx.request_repaint();
    }

    /// Commit a partial edit buffer (called when focus leaves the segment).
    /// A no-op when the buffer is empty.
    fn commit_typing(&mut self, ctx: &mut WidgetCtx) {
        if self.typing.is_empty() {
            return;
        }
        let kind = self.focused_kind();
        let (min, _) = self.seg_range(kind);
        let parsed: i32 = self.typing.parse().unwrap_or(min);
        self.typing.clear();
        if self.write_seg(kind, parsed.max(min)) {
            self.changed(ctx);
        } else {
            ctx.request_repaint();
        }
    }

    /// Move segment focus by `dir` with wrap, committing in-progress typing.
    fn move_seg(&mut self, ctx: &mut WidgetCtx, dir: i32) {
        self.commit_typing(ctx);
        self.seg = wrap_range(self.seg as i32 + dir, 0, 2) as usize;
        ctx.request_repaint();
    }

    /// Display row (`DD / MM / YYYY` in locale order) plus segment spans.
    fn display_row(&self) -> (String, Vec<(usize, usize)>) {
        let mut row = String::new();
        let mut spans = Vec::new();
        for (i, kind) in self.segments().iter().enumerate() {
            if i > 0 {
                row.push_str(" / ");
            }
            let start = row.len();
            let mut text = Self::fmt_seg(*kind, self.seg_value(*kind));
            if i == self.seg && !self.typing.is_empty() {
                // Show in-progress typing padded to full width.
                let width = Self::seg_width(*kind);
                let padded = format!("{:>width$}", self.typing, width = width);
                text = padded;
            }
            row.push_str(&text);
            spans.push((start, row.len()));
        }
        (row, spans)
    }

    /// Spinner strip row for the focused segment, padded to `width`, plus
    /// click zones measured from the row start: decrement / set-prev /
    /// current (noop) / set-next / increment.
    fn strip_row(&self, width: usize) -> (String, StripZones) {
        let kind = self.focused_kind();
        let (min, max) = self.seg_range(kind);
        let cur = self.seg_value(kind);
        let prev = wrap_range(cur - 1, min, max);
        let next = wrap_range(cur + 1, min, max);
        let prev_s = Self::fmt_seg(kind, prev);
        let cur_s = Self::fmt_seg(kind, cur);
        let next_s = Self::fmt_seg(kind, next);
        // [< PP | CC | NN >] with the arrows as the step ends.
        let mut row = format!("< {prev_s} | {cur_s} | {next_s} >");
        if row.len() < width {
            let pad = width - row.len();
            let left = pad / 2;
            row = format!("{}{row}{}", " ".repeat(left), " ".repeat(pad - left));
        }
        // Zones index into the unpadded core; padding only shifts right.
        let shift = row.find('<').unwrap_or(0);
        let core = |i: usize| shift + i;
        let pw = prev_s.len();
        let cw = cur_s.len();
        // "< PP | CC | NN >": 0:'<', 2..2+pw:prev, then " | ", cur, " | ", next, " >".
        let prev_start = core(2);
        let cur_start = prev_start + pw + 3;
        let next_start = cur_start + cw + 3;
        let zones = StripZones {
            dec_end: prev_start,
            prev: (prev_start, prev_start + pw, prev),
            cur: (cur_start, cur_start + cw, cur),
            next: (next_start, next_start + next_s.len(), next),
            inc_start: next_start + next_s.len() + 1,
        };
        (row, zones)
    }

    fn set_open(&mut self, ctx: &mut WidgetCtx, open: bool) {
        if self.open != open {
            // Partial input is scratch: moving between segments commits it,
            // but leaving the widget (blur) or picking the strip discards
            // it — the strip is its own entry path.
            self.typing.clear();
            self.open = open;
        }
        ctx.request_repaint();
    }
}

/// Click zones of the spinner strip row (offsets from the row start).
struct StripZones {
    /// x < `dec_end` (the `<` end): step down.
    dec_end: usize,
    /// Previous value zone (click sets it).
    prev: (usize, usize, i32),
    /// Current value zone (noop).
    cur: (usize, usize, i32),
    /// Next value zone (click sets it).
    next: (usize, usize, i32),
    /// x >= `inc_start` (the `>` end): step up.
    inc_start: usize,
}

impl StripZones {
    fn click(&self, x: usize) -> StripAction {
        if x < self.dec_end {
            StripAction::Step(-1)
        } else if x < self.prev.1 && x >= self.prev.0 {
            StripAction::Set(self.prev.2)
        } else if x < self.cur.1 && x >= self.cur.0 {
            StripAction::Noop
        } else if x < self.next.1 && x >= self.next.0 {
            StripAction::Set(self.next.2)
        } else if x >= self.inc_start {
            StripAction::Step(1)
        } else {
            StripAction::Noop
        }
    }
}

enum StripAction {
    Noop,
    Step(i32),
    Set(i32),
}

fn underline() -> Style {
    Style::parse("underline").unwrap_or_default()
}

fn underline_bold() -> Style {
    Style::parse("bold underline").unwrap_or_default()
}

impl Widget for DateInput {
    fn style_type(&self) -> &'static str {
        "DateInput"
    }

    fn focusable(&self) -> bool {
        true
    }

    fn layout_height(&self) -> Option<usize> {
        // Constant: the strip row renders blank when closed, so focus
        // changes never need a relayout.
        Some(2)
    }

    fn take_node_seed(&mut self) -> NodeSeed {
        std::mem::take(&mut self.seed)
    }

    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        let open = self.open;
        let (row, spans) = self.display_row();
        let mut line: Vec<Segment> = Vec::new();
        let mut cursor = 0;
        for (i, (start, end)) in spans.iter().enumerate() {
            if *start > cursor {
                line.push(Segment::new(row[cursor..*start].to_string()));
            }
            let focused = i == self.seg && open;
            if focused {
                line.push(Segment::styled(row[*start..*end].to_string(), underline()));
            } else {
                line.push(Segment::new(row[*start..*end].to_string()));
            }
            cursor = *end;
        }
        if cursor < row.len() {
            line.push(Segment::new(row[cursor..].to_string()));
        }
        if !open {
            return Segments::from(line);
        }
        let (strip, _) = self.strip_row(row.len());
        // Underline the current value cell inside the strip.
        let kind = self.focused_kind();
        let cur_s = Self::fmt_seg(kind, self.seg_value(kind));
        let mut strip_line: Vec<Segment> = Vec::new();
        if let Some(pos) = strip.find(&cur_s) {
            // Match the `| CC |` cell, not a coincidental digit run: the
            // current cell is the middle `X | CC | Y` group.
            let cell_start = strip[..pos].rfind("| ").map_or(pos, |p| p + 2);
            let cell_end = pos + cur_s.len();
            if cell_start > 0 {
                strip_line.push(Segment::new(strip[..cell_start].to_string()));
            }
            strip_line.push(Segment::styled(
                strip[cell_start..cell_end].to_string(),
                underline_bold(),
            ));
            if cell_end < strip.len() {
                strip_line.push(Segment::new(strip[cell_end..].to_string()));
            }
        } else {
            strip_line.push(Segment::new(strip));
        }
        let mut lines = line;
        lines.push(Segment::new("\n".to_string()));
        lines.extend(strip_line);
        Segments::from(lines)
    }

    fn on_event(&mut self, event: &Event, ctx: &mut WidgetCtx) {
        // The strip tracks real focus state, not just Focus/Blur events:
        // mount auto-focus flips state without posting events.
        let focused = crate::widgets::Widget::node_state(self).focused;
        if focused != self.open {
            self.set_open(ctx, focused);
            if matches!(event, Event::Focus(_) | Event::Blur(_)) {
                ctx.set_handled();
                return;
            }
        }
        match event {
            Event::Focus(focus) if focus.node == ctx.node_id() => {
                // Segment position survives refocus; a fresh widget starts
                // at segment 0 via `new`.
                self.set_open(ctx, true);
                ctx.set_handled();
            }
            Event::Blur(blur) if blur.node == ctx.node_id() => {
                self.set_open(ctx, false);
                ctx.set_handled();
            }
            Event::Key(key) if focused => {
                let name = key.name();
                match name {
                    "up" => self.step_focused(ctx, 1),
                    "down" => self.step_focused(ctx, -1),
                    "left" => self.move_seg(ctx, -1),
                    "right" => self.move_seg(ctx, 1),
                    "escape" => {
                        self.typing.clear();
                        ctx.request_repaint();
                    }
                    "backspace" => {
                        // Pop one digit off the edit buffer; committed
                        // state is untouched.
                        if self.typing.pop().is_some() {
                            ctx.request_repaint();
                        }
                    }
                    _ => {
                        if let Some(ch) = key.character {
                            if ch.is_ascii_digit() {
                                self.type_digit(ctx, ch);
                            } else {
                                return;
                            }
                        } else {
                            return;
                        }
                    }
                }
                ctx.set_handled();
            }
            Event::MouseDown(down) => {
                let (row, spans) = self.display_row();
                if down.y == 0 {
                    let x = down.x as usize;
                    for (i, (start, end)) in spans.iter().enumerate() {
                        if x >= *start && x < *end {
                            // Clicking another segment commits partial input.
                            self.commit_typing(ctx);
                            self.seg = i;
                            ctx.request_repaint();
                            ctx.set_handled();
                            return;
                        }
                    }
                } else if down.y == 1 && self.open {
                    let (_, zones) = self.strip_row(row.len());
                    match zones.click(down.x as usize) {
                        StripAction::Noop => {}
                        StripAction::Step(dir) => self.step_focused(ctx, dir),
                        StripAction::Set(v) => {
                            let kind = self.focused_kind();
                            self.typing.clear();
                            if self.write_seg(kind, v) {
                                self.changed(ctx);
                            } else {
                                ctx.request_repaint();
                            }
                        }
                    }
                    ctx.set_handled();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leap_year_rule() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2028));
        assert!(!is_leap_year(2023));
        assert!(!is_leap_year(2100));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2000));
    }

    #[test]
    fn february_length_follows_leap_year() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 1), 31);
    }

    #[test]
    fn wrap_range_wraps_both_ends() {
        assert_eq!(wrap_range(12 + 1, 1, 12), 1);
        assert_eq!(wrap_range(1 - 1, 1, 12), 12);
        assert_eq!(wrap_range(2100 + 1, YEAR_MIN, YEAR_MAX), YEAR_MIN);
        assert_eq!(wrap_range(0, 0, 2), 0);
    }

    #[test]
    fn constructor_clamps_impossible_dates() {
        let input = DateInput::new(2023, 2, 31);
        assert_eq!(input.date(), (2023, 2, 28));
        let input = DateInput::new(2024, 2, 31);
        assert_eq!(input.date(), (2024, 2, 29));
    }

    #[test]
    fn segment_orders_cover_locales() {
        // Year / month / day is the default order.
        let mut input = DateInput::new(2024, 3, 5);
        assert_eq!(input.order_of(), DateOrder::Ymd);
        let (row, _) = input.display_row();
        assert_eq!(row, "2024 / 03 / 05");
        input.order = DateOrder::Mdy;
        let (row, _) = input.display_row();
        assert_eq!(row, "03 / 05 / 2024");
        input.order = DateOrder::Dmy;
        let (row, _) = input.display_row();
        assert_eq!(row, "05 / 03 / 2024");
    }

    /// Manual entry scrolls the strip to the typed number, ignoring
    /// leading zeros: typing `0` then `2` on the month segment centers the
    /// strip on February.
    #[test]
    fn manual_entry_scrolls_strip_ignoring_zero() {
        use crate::event::EventCtx;
        use crate::node_id::node_id_from_ffi;
        let mut input = DateInput::new(2024, 1, 15);
        input.seg = 1; // month segment
        input.open = true;
        let mut ctx = EventCtx::default();
        let node = node_id_from_ffi(1);
        ctx.set_node_id(node);
        let mut wctx = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
        input.type_digit(&mut wctx, '0');
        assert_eq!(input.date().1, 1, "lone zero clamps to segment min");
        input.type_digit(&mut wctx, '2');
        assert_eq!(input.date().1, 2);
        let (strip, _) = input.strip_row(14);
        assert!(
            strip.contains("01 | 02 | 03"),
            "strip must follow manual entry (got {strip:?})"
        );
    }

    /// The user changed the year, so the dependent day adapts:
    /// 2024/02/29 -> year 2025 -> 2025/02/28 (never rejects the change).
    #[test]
    fn changing_year_clamps_dependent_day() {
        let mut input = DateInput::new(2024, 2, 29);
        assert!(input.write_seg(SegmentKind::Year, 2025));
        assert_eq!(input.date(), (2025, 2, 28));
    }

    /// Same hierarchical rule for months: 2026/01/31 -> April -> 2026/04/30.
    #[test]
    fn changing_month_clamps_dependent_day() {
        let mut input = DateInput::new(2026, 1, 31);
        assert!(input.write_seg(SegmentKind::Month, 4));
        assert_eq!(input.date(), (2026, 4, 30));
    }

    /// The day scroller is dynamic: Feb 2025 centers on 28 with next
    /// wrapping to 01, while Feb 2024 offers 29.
    #[test]
    fn feb_strip_follows_leap_year() {
        let mut leap = DateInput::new(2024, 2, 29);
        leap.seg = 2; // day strip (position 2 in YMD)
        let (strip, _) = leap.strip_row(14);
        assert!(strip.contains("28 | 29 | 01"), "got {strip:?}");
        let mut flat = DateInput::new(2024, 2, 29);
        assert!(flat.write_seg(SegmentKind::Year, 2025));
        flat.seg = 2;
        let (strip, _) = flat.strip_row(14);
        assert!(strip.contains("27 | 28 | 01"), "got {strip:?}");
    }

    /// Edit buffer vs committed state: typing `202` over the year while Feb
    /// 29 is selected must leave the committed date untouched; only the
    /// full `2028` commits (leap, so day 29 survives).
    #[test]
    fn partial_year_typing_preserves_feb_29_until_commit() {
        use crate::event::EventCtx;
        use crate::node_id::node_id_from_ffi;
        let mut input = DateInput::new(2024, 2, 29);
        input.seg = 0; // year is position 0 in the default YMD order
        input.open = true;
        let mut ctx = EventCtx::default();
        let node = node_id_from_ffi(1);
        ctx.set_node_id(node);
        let mut wctx = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
        for ch in ['2', '0', '2'] {
            input.type_digit(&mut wctx, ch);
            assert_eq!(
                input.date(),
                (2024, 2, 29),
                "partial year {ch} must not revalidate the day"
            );
        }
        input.type_digit(&mut wctx, '8');
        assert_eq!(input.date(), (2028, 2, 29));
    }

    /// Committing a non-leap year revalidates once: full `2025` clamps the
    /// day 29 -> 28.
    #[test]
    fn full_year_typing_revalidates_day_on_commit() {
        use crate::event::EventCtx;
        use crate::node_id::node_id_from_ffi;
        let mut input = DateInput::new(2024, 2, 29);
        input.seg = 0; // year is position 0 in the default YMD order
        input.open = true;
        let mut ctx = EventCtx::default();
        let node = node_id_from_ffi(1);
        ctx.set_node_id(node);
        let mut wctx = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
        for ch in ['2', '0', '2', '5'] {
            input.type_digit(&mut wctx, ch);
        }
        assert_eq!(input.date(), (2025, 2, 28));
    }

    /// Leaving the segment commits the partial buffer: `202` clamps to the
    /// minimum year and revalidates the day exactly once.
    #[test]
    fn leaving_segment_commits_partial_year() {
        use crate::event::EventCtx;
        use crate::node_id::node_id_from_ffi;
        let mut input = DateInput::new(2024, 2, 29);
        input.seg = 0; // year is position 0 in the default YMD order
        input.open = true;
        let mut ctx = EventCtx::default();
        let node = node_id_from_ffi(1);
        ctx.set_node_id(node);
        let mut wctx = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
        for ch in ['2', '0', '2'] {
            input.type_digit(&mut wctx, ch);
        }
        input.move_seg(&mut wctx, -1);
        assert_eq!(input.date(), (YEAR_MIN, 2, 28));
        assert_eq!(input.focused_segment(), 2);
    }

    /// Leaving the widget discards scratch input: `202` + blur leaves the
    /// committed leap-day date untouched.
    #[test]
    fn blur_discards_partial_year() {
        use crate::event::EventCtx;
        use crate::node_id::node_id_from_ffi;
        let mut input = DateInput::new(2024, 2, 29);
        input.seg = 0;
        input.open = true;
        let mut ctx = EventCtx::default();
        let node = node_id_from_ffi(1);
        ctx.set_node_id(node);
        let mut wctx = crate::event::WidgetCtx::__from_dispatch(node, &mut ctx);
        for ch in ['2', '0', '2'] {
            input.type_digit(&mut wctx, ch);
        }
        input.set_open(&mut wctx, false);
        assert_eq!(input.date(), (2024, 2, 29));
        assert!(input.typing.is_empty());
    }

    #[test]
    fn strip_row_marks_three_values() {
        let mut input = DateInput::new(2024, 3, 30);
        input.seg = 2; // day is position 2 in the default YMD order.
        let (strip, zones) = input.strip_row(14);
        assert!(strip.starts_with('<'));
        assert!(strip.ends_with('>'));
        assert!(strip.contains("29 | 30 | 31"));
        assert!(matches!(zones.click(0), StripAction::Step(-1)));
        assert!(matches!(zones.click(100), StripAction::Step(1)));
    }
}
