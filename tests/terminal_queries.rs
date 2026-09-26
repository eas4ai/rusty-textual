//! Startup mode negotiation in a terminal that answers its queries.
//!
//! Regression: the driver waited for DECRQM replies with
//! `crossterm::event::poll`, which read the reply into crossterm's input
//! parser. crossterm cannot parse a DECRQM reply: it treats it as an
//! incomplete sequence and swallows every later input byte, so the app froze
//! at startup in any terminal that answers DECRQM. Each test starts the
//! calculator example and quits it with ctrl+q, which reaches the app only
//! if input is not being swallowed. Run idle, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use std::time::Duration;

use pty::{Answers, SHELL_AROUND, Term, calculator, contains, dump, painted_rows, row_of};

/// Start the calculator in a terminal that answers as `answers` says, wait
/// until it has drawn, quit it with ctrl+q, and return what it wrote and the
/// final screen.
fn start_and_quit(answers: Answers) -> (Vec<u8>, vt100::Screen) {
    let term = Term::spawn(SHELL_AROUND, &calculator(), &[], answers);
    term.wait_for("the calculator to draw", |s| {
        s.alternate_screen() && !painted_rows(s).is_empty()
    });
    term.settle();
    term.send(b"\x11"); // ctrl+q
    let raw = term.raw();
    let screen = term.finish();
    assert!(
        row_of(&screen, "after-exit").is_some(),
        "the app did not quit on ctrl+q:\n{}",
        dump(&screen)
    );
    (raw, screen)
}

#[test]
fn app_starts_and_takes_keys_when_the_terminal_answers_mode_queries() {
    let (raw, _) = start_and_quit(Answers::TERMINAL);
    assert!(
        contains(&raw, b"\x1b[?2026h"),
        "synchronized output was not negotiated from the terminal's reply; output began: {}",
        String::from_utf8_lossy(&raw[..raw.len().min(300)]).escape_debug()
    );
}

#[test]
fn app_starts_and_takes_keys_when_replies_arrive_late() {
    // Later than the old 100 ms per-query window, inside the exchange budget.
    let answers = Answers {
        delay: Duration::from_millis(150),
        ..Answers::TERMINAL
    };
    start_and_quit(answers);
}

#[test]
fn app_starts_and_takes_keys_when_the_terminal_answers_nothing() {
    let (raw, _) = start_and_quit(Answers::NONE);
    assert!(
        !contains(&raw, b"\x1b[?2026h"),
        "synchronized output used without the terminal reporting support"
    );
}
