//! PTY checks for pushed screens (`docs/spec/screens.md`).
//!
//! This file is the `screen-pty` Sudus mechanism, run through
//! `scripts/screen_mechanism.py`, which maps each `scr_NNN_` test to its
//! requirement. Each test runs the probe app of `tests/fixtures/inline_probe`
//! in the pseudo terminal of `tests/support/pty.rs` and presses `p`, which
//! pushes a screen of 60 lines (`pushed 1` to `pushed 60`, then
//! `pushed-end`) onto the 30-row terminal. Every case runs for a `Screen`
//! and a `ModalScreen`, in full-screen and in inline mode. Run idle and
//! single-threaded, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, SHELL_THEN_EXEC, Term, dump, has_text, lines, painted_rows, probe};

/// (`PROBE_MODE`, `PROBE_PUSH`): each kind of pushed screen in each mode.
const CASES: [(&str, &str); 4] = [
    ("full", "screen"),
    ("full", "modal"),
    ("inline", "screen"),
    ("inline", "modal"),
];

/// The pushed screen's last line.
const LAST_LINE: &str = "pushed-end";

/// Start the probe in `mode`, push a `kind` screen, and return the terminal
/// once the screen shows, after checking its last line is not yet visible.
fn pushed(mode: &str, kind: &str) -> (Term, vt100::Screen) {
    let env = [("PROBE_MODE", mode), ("PROBE_PUSH", kind)];
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), &env, Answers::TERMINAL);
    term.wait_for("the probe", has_text("keys:0"));
    term.settle();
    term.send(b"p");
    term.wait_for("the pushed screen", has_text("pushed 1"));
    let screen = term.settle();
    assert!(
        !has_text(LAST_LINE)(&screen),
        "{mode} {kind}: the pushed screen's last line shows before scrolling:\n{}",
        dump(&screen)
    );
    (term, screen)
}

/// Rows of the vertical scrollbar in the last column: the painted rows
/// other than the inline screen's top and bottom border rows.
fn scrollbar_rows(screen: &vt100::Screen) -> Vec<u16> {
    let text = lines(screen);
    painted_rows(screen)
        .into_iter()
        .filter(|&row| {
            !text[usize::from(row)]
                .chars()
                .last()
                .is_some_and(|c| c == '\u{2594}' || c == '\u{2581}')
        })
        .collect()
}

#[test]
fn scr_001_the_mouse_wheel_scrolls_a_pushed_screen_to_its_last_line() {
    for (mode, kind) in CASES {
        let (term, screen) = pushed(mode, kind);
        let rows = painted_rows(&screen);
        let y = rows[0].midpoint(rows[rows.len() - 1]) + 1;
        for _ in 0..80 {
            term.send(format!("\x1b[<65;10;{y}M").as_bytes()); // wheel down
        }
        term.wait_for(
            &format!("{mode} {kind}: the last line after turning the wheel"),
            has_text(LAST_LINE),
        );
    }
}

#[test]
fn scr_001_dragging_the_scrollbar_scrolls_a_pushed_screen_to_its_last_line() {
    for (mode, kind) in CASES {
        let (term, screen) = pushed(mode, kind);
        let (rows, cols) = screen.size();
        let look = |row: u16| {
            screen
                .cell(row, cols - 1)
                .map(|cell| (cell.fgcolor(), cell.bgcolor(), cell.inverse()))
        };
        // Before scrolling the thumb is at the top of the track, so the
        // track's last row looks like the track and the thumb differs.
        let track = scrollbar_rows(&screen);
        let track_look = look(track[track.len() - 1]);
        let Some(&thumb) = track.iter().find(|&&row| look(row) != track_look) else {
            panic!(
                "{mode} {kind}: no scrollbar thumb in the last column:\n{}",
                dump(&screen)
            );
        };
        // Press on the thumb, drag it to the terminal's last row, release.
        term.send(format!("\x1b[<0;{cols};{}M", thumb + 1).as_bytes());
        for y in thumb + 2..=rows {
            term.send(format!("\x1b[<32;{cols};{y}M").as_bytes());
        }
        term.send(format!("\x1b[<0;{cols};{rows}m").as_bytes());
        term.wait_for(
            &format!("{mode} {kind}: the last line after dragging the scrollbar"),
            has_text(LAST_LINE),
        );
    }
}
