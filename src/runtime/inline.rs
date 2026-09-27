//! Inline render mode (Python `App.run(inline=True)`): the app draws below
//! the shell prompt on the terminal's main screen instead of the alternate
//! screen.
//!
//! Each frame is the whole app, redrawn from its top-left cell (the origin)
//! with relative cursor moves only, the way Python's `InlineUpdate` writes
//! it (`_compositor.py`), so the shell content above the app stays put.
//! After a frame the cursor is back at the origin, and when the origin can
//! have moved the runtime asks the terminal where that is (a cursor position
//! report) so mouse coordinates can be made relative to the app.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// Inline-mode state for a running app.
#[derive(Debug, Clone)]
pub(crate) struct InlineState {
    /// Blank lines written below the cursor before the first frame
    /// (Python `App.INLINE_PADDING`).
    pub(crate) padding: usize,
    /// The terminal's size; the app's frame is at most this tall.
    pub(crate) terminal: (u16, u16),
    /// Rows the previous frame used, if one was drawn.
    pub(crate) previous_height: Option<u16>,
    /// The app's top-left cell `(column, row)` from the last cursor position
    /// report, both 0-based.
    pub(crate) origin: Option<(u16, u16)>,
    /// The terminal was resized since the last frame.
    pub(crate) resized: bool,
    /// The app started: the padding was written and frames may be on screen.
    /// An inline run that stopped earlier has nothing to erase on exit.
    pub(crate) started: bool,
    /// When to ask the terminal for the origin again.
    pub(crate) origin_query: OriginQuery,
    /// Which cursor position reports answer the runtime's requests.
    pub(crate) origin_marks: OriginMarks,
}

impl InlineState {
    pub(crate) fn new(padding: usize, terminal: (u16, u16)) -> Self {
        Self {
            padding,
            terminal,
            previous_height: None,
            origin: None,
            resized: false,
            started: false,
            origin_query: OriginQuery::WhenMoved,
            origin_marks: OriginMarks::default(),
        }
    }
}

/// The wait before retrying an unanswered cursor position query.
pub(crate) const ORIGIN_RETRY: Duration = Duration::from_secs(1);

/// When the runtime asks the terminal where the origin is. crossterm hands
/// the report only to a caller that waits for it, so the runtime asks after
/// every frame that can have moved the origin and at no other frame
/// (INL-007; Python asks after every frame without waiting). Each
/// unanswered query blocks for crossterm's 2 s timeout, so a query that goes
/// unanswered is retried once, and a retry that goes unanswered is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OriginQuery {
    /// Ask after a frame that can have moved the origin: the last query was
    /// answered, none was sent, or the retry went unanswered too.
    WhenMoved,
    /// The last query went unanswered: ask once more with the first frame at
    /// or after this time, or sooner with a frame that can have moved the
    /// origin. A reply that was only late is queued by then and answers at
    /// once.
    RetryAt(Instant),
}

impl OriginQuery {
    /// Whether to send a query at `now`, after a frame that `moved` the
    /// origin or may have (see [`origin_can_move`]).
    pub(crate) fn due(self, moved: bool, now: Instant) -> bool {
        moved || matches!(self, Self::RetryAt(at) if now >= at)
    }

    /// The state after a query that returned at `now`, `answered` or not.
    pub(crate) fn after(self, answered: bool, now: Instant) -> Self {
        match (answered, self) {
            (false, Self::WhenMoved) => Self::RetryAt(now + ORIGIN_RETRY),
            (true, _) | (false, Self::RetryAt(_)) => Self::WhenMoved,
        }
    }
}

/// The first column a request marks. A legacy F3 key with modifiers
/// (`CSI 1 ; m R`, m from 2 to 16) reads as a cursor position report in
/// column m - 1 of the first row, so the marks start past those columns.
const FIRST_MARK: u16 = 16;

/// How many reports that answer none of the current requests one request
/// skips before it counts as unanswered.
pub(crate) const MAX_SKIPPED_REPORTS: usize = 16;

/// Which cursor position reports answer the runtime's requests (INL-007).
/// crossterm answers a request with the oldest report it holds, which can
/// be the late reply to an earlier request or a key that reads as a report,
/// so each request first moves the cursor from the origin to its own mark
/// column. A report in the mark column of a request made since the origin
/// last moved gives the origin's row; any other report is skipped.
#[derive(Debug, Clone, Default)]
pub(crate) struct OriginMarks {
    /// Requests made so far; picks the next mark.
    sent: u32,
    /// The marks of the requests made since the origin last moved.
    current: Vec<u16>,
}

impl OriginMarks {
    /// The mark for a request made after a frame that `moved` the origin or
    /// may have, in a terminal `width` columns wide. `None` when the terminal
    /// is too narrow for two different marks; then any report answers.
    pub(crate) fn next(&mut self, moved: bool, width: u16) -> Option<u16> {
        if moved {
            self.current.clear();
        }
        let span = width.saturating_sub(FIRST_MARK);
        if span < 2 {
            return None;
        }
        let offset = u16::try_from(self.sent % u32::from(span)).unwrap_or_default();
        self.sent = self.sent.wrapping_add(1);
        let mark = FIRST_MARK + offset;
        self.current.push(mark);
        Some(mark)
    }

    /// Whether a report in `column` answers a request made since the origin
    /// last moved.
    pub(crate) fn answer(&self, column: u16) -> bool {
        self.current.contains(&column)
    }
}

/// Whether a frame of `rows` rows can have moved the origin: it is the first
/// frame, it is taller than the `previous` one (a frame that runs past the
/// terminal's last row scrolls the terminal), or it is the first frame after
/// a resize. A frame of the same height or shorter leaves the origin where
/// it was.
pub(crate) fn origin_can_move(previous: Option<u16>, rows: usize, resized: bool) -> bool {
    resized || previous.is_none_or(|previous| usize::from(previous) < rows)
}

/// Whether a request to run inline takes effect: Python picks its inline
/// driver only when not on Windows, so inline requests run full-screen there.
pub(crate) fn effective_inline(requested: bool, windows: bool) -> bool {
    requested && !windows
}

/// Row separator inside a frame. Raw mode turns off the terminal's
/// newline-to-CRLF translation, so the carriage return is written out.
pub(crate) const ROW_BREAK: &str = "\r\n";

/// Erase the whole display (Python writes it on SIGWINCH in inline mode).
pub(crate) const ERASE_DISPLAY: &str = "\x1b[2J";

/// The bytes that follow a frame's `rows` rows: erase below the frame when
/// it got shorter (`clear`), then return to the origin. Python
/// `InlineUpdate.render_segments`, minus its cursor position query, which
/// the runtime sends separately.
pub(crate) fn frame_tail(rows: usize, clear: bool) -> String {
    let mut out = String::new();
    if clear {
        if rows > 1 {
            out.push_str(ROW_BREAK);
        }
        out.push_str("\x1b[J");
    }
    if rows > 1 {
        let back = if clear { rows } else { rows - 1 };
        let _ = write!(out, "\x1b[{back}A\r");
    } else {
        out.push('\r');
    }
    out
}

/// The bytes that end an inline run, written with the cursor at the origin.
///
/// `keep_frame` (the no-clear exit) leaves the last frame on screen and puts
/// the cursor on the line below it. Otherwise the app's rows and its padding
/// are erased and the cursor returns to where the padding began. Python
/// `App._process_messages` exit path and `LinuxInlineDriver.stop_application_mode`.
pub(crate) fn exit_sequence(keep_frame: bool, height: u16, padding: usize) -> String {
    let mut out = String::new();
    if keep_frame {
        if height > 1 {
            let _ = write!(out, "\x1b[{}B", height - 1);
        }
        out.push_str(ROW_BREAK);
    } else {
        if padding > 0 {
            let _ = write!(out, "\x1b[{padding}A");
        }
        out.push_str("\r\x1b[J");
    }
    out
}

/// Translate a terminal cell to app coordinates, or `None` when it lies
/// above or left of the app's origin.
pub(crate) fn app_relative(cell: (u16, u16), origin: Option<(u16, u16)>) -> Option<(u16, u16)> {
    let Some((ox, oy)) = origin else {
        return Some(cell);
    };
    Some((cell.0.checked_sub(ox)?, cell.1.checked_sub(oy)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inl_015_windows_runs_inline_requests_full_screen() {
        assert!(
            !effective_inline(true, true),
            "Windows falls back to full-screen"
        );
        assert!(effective_inline(true, false));
        assert!(!effective_inline(false, false));
        assert!(!effective_inline(false, true));
    }

    #[test]
    fn frame_tail_returns_to_the_origin() {
        // Python: N rows joined by newlines, then up N-1 lines and a CR.
        assert_eq!(frame_tail(5, false), "\x1b[4A\r");
        assert_eq!(frame_tail(1, false), "\r");
    }

    #[test]
    fn frame_tail_erases_below_a_shrunk_frame() {
        // Python: an extra newline, clear down, then up N lines.
        assert_eq!(frame_tail(3, true), "\r\n\x1b[J\x1b[3A\r");
        assert_eq!(frame_tail(1, true), "\x1b[J\r");
    }

    #[test]
    fn exit_erases_the_app_and_its_padding() {
        assert_eq!(exit_sequence(false, 5, 1), "\x1b[1A\r\x1b[J");
        assert_eq!(exit_sequence(false, 5, 0), "\r\x1b[J");
    }

    #[test]
    fn no_clear_exit_moves_below_the_last_frame() {
        assert_eq!(exit_sequence(true, 5, 1), "\x1b[4B\r\n");
        assert_eq!(exit_sequence(true, 1, 1), "\r\n");
    }

    #[test]
    fn inl_007_an_unanswered_origin_query_is_retried_once() {
        let now = Instant::now();
        let retry = OriginQuery::WhenMoved.after(false, now);
        assert_eq!(retry, OriginQuery::RetryAt(now + ORIGIN_RETRY));
        assert!(!retry.due(false, now), "the retry waits");
        assert!(
            retry.due(false, now + ORIGIN_RETRY),
            "the retry comes with any frame"
        );
        assert_eq!(
            retry.after(true, now),
            OriginQuery::WhenMoved,
            "a late reply answers the retry"
        );
        let retried = retry.after(false, now);
        assert_eq!(
            retried,
            OriginQuery::WhenMoved,
            "an unanswered retry is not retried"
        );
        assert!(!retried.due(false, now + Duration::from_secs(3600)));
    }

    #[test]
    fn inl_007_a_frame_that_can_move_the_origin_always_asks() {
        let now = Instant::now();
        assert!(OriginQuery::WhenMoved.due(true, now));
        assert!(
            OriginQuery::RetryAt(now + ORIGIN_RETRY).due(true, now),
            "a pending retry does not hold back a frame that can move the origin"
        );
        let retried = OriginQuery::WhenMoved.after(false, now).after(false, now);
        assert!(retried.due(true, now), "nor does an unanswered retry");
        assert_eq!(
            retried.after(false, now),
            OriginQuery::RetryAt(now + ORIGIN_RETRY),
            "that query, unanswered, is retried once too"
        );
    }

    #[test]
    fn inl_007_only_a_report_in_a_current_mark_answers() {
        let mut marks = OriginMarks::default();
        let first = marks.next(true, 80).expect("room for marks");
        assert!((FIRST_MARK..80).contains(&first));
        assert!(marks.answer(first));
        for bogus in 1..FIRST_MARK {
            assert!(
                !marks.answer(bogus),
                "F3 with modifiers reads as column {bogus}"
            );
        }
        let retry = marks.next(false, 80).expect("room for marks");
        assert_ne!(retry, first);
        assert!(marks.answer(first) && marks.answer(retry), "no move since");
        let after_move = marks.next(true, 80).expect("room for marks");
        assert!(marks.answer(after_move));
        assert!(
            !marks.answer(first) && !marks.answer(retry),
            "from before the move"
        );
        assert_eq!(marks.next(true, FIRST_MARK + 1), None, "too narrow");
    }

    #[test]
    fn inl_007_only_a_frame_that_can_move_the_origin_asks_for_it() {
        let now = Instant::now();
        assert!(origin_can_move(None, 5, false), "the first frame");
        assert!(origin_can_move(Some(5), 8, false), "a taller frame");
        assert!(
            origin_can_move(Some(5), 5, true),
            "the first frame after a resize"
        );
        assert!(
            !origin_can_move(Some(5), 5, false),
            "a frame of the same height"
        );
        assert!(!origin_can_move(Some(5), 3, false), "a shorter frame");
        assert!(!OriginQuery::WhenMoved.due(false, now));
    }

    #[test]
    fn coordinates_become_relative_to_the_origin() {
        assert_eq!(app_relative((10, 7), Some((0, 3))), Some((10, 4)));
        assert_eq!(app_relative((10, 2), Some((0, 3))), None, "above the app");
        assert_eq!(
            app_relative((10, 2), None),
            Some((10, 2)),
            "origin not known yet"
        );
    }
}
