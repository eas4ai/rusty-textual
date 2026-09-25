//! Inline render mode (Python `App.run(inline=True)`): the app draws below
//! the shell prompt on the terminal's main screen instead of the alternate
//! screen.
//!
//! Each frame is the whole app, redrawn from its top-left cell (the origin)
//! with relative cursor moves only, the way Python's `InlineUpdate` writes
//! it (`_compositor.py`), so the shell content above the app stays put.
//! After a frame the cursor is back at the origin, and the runtime asks the
//! terminal where that is (a cursor position report) so mouse coordinates can
//! be made relative to the app.

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
    /// After a cursor position query went unanswered, the earliest time to
    /// ask again; `None` asks after every frame. A reply that was only late
    /// is still queued and answers the next query at once.
    pub(crate) origin_retry_at: Option<Instant>,
    /// The wait before asking again after an unanswered query; doubles while
    /// the terminal stays silent (see [`next_origin_backoff`]).
    pub(crate) origin_backoff: Duration,
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
            origin_retry_at: None,
            origin_backoff: Duration::ZERO,
        }
    }
}

/// The first wait before asking a silent terminal for the cursor position
/// again.
pub(crate) const ORIGIN_RETRY_MIN: Duration = Duration::from_secs(1);
/// The longest wait between such attempts.
pub(crate) const ORIGIN_RETRY_MAX: Duration = Duration::from_secs(32);

/// The wait after another unanswered cursor position query: 1 s, then
/// doubling up to 32 s. Each attempt at a silent terminal blocks for
/// crossterm's 2 s timeout, so the backoff bounds that cost.
pub(crate) fn next_origin_backoff(current: Duration) -> Duration {
    current
        .saturating_mul(2)
        .clamp(ORIGIN_RETRY_MIN, ORIGIN_RETRY_MAX)
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
    fn origin_retries_back_off_up_to_the_maximum() {
        let mut wait = Duration::ZERO;
        let mut waits = Vec::new();
        for _ in 0..8 {
            wait = next_origin_backoff(wait);
            waits.push(wait.as_secs());
        }
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 32, 32]);
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
