//! PTY checks for queries on a pushed screen (`docs/spec/screens.md`).
//!
//! Part of the `screen-pty` Sudus mechanism, run through
//! `scripts/screen_mechanism.py`, which maps each `scr_NNN_` test to its
//! requirement. Each test runs the probe app of `tests/fixtures/inline_probe`
//! in the pseudo terminal of `tests/support/pty.rs`, presses `p` to push a
//! screen of 60 lines (`pushed 1` to `pushed 60`, then `pushed-end`), then
//! `h`, which hides that text through `App::query_mut("#pushed-body")` then
//! `DomQueryMut::set_display(false)`. Each case has its own test. Run idle
//! and single-threaded, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, SHELL_THEN_EXEC, Term, dump, has_text, lines, probe};

/// Pushes the probe's screen, hides its text through a query, and checks
/// that the text is gone.
fn check_query_hides_pushed_text(mode: &str, push: &str) {
    let env = [("PROBE_MODE", mode), ("PROBE_PUSH", push)];
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), &env, Answers::TERMINAL);
    term.wait_for("probe status", has_text("keys:0"));
    term.settle();
    term.send(b"p");
    term.wait_for(&format!("{env:?}: the pushed screen"), has_text("pushed 1"));
    term.settle();
    term.send(b"h");
    let screen = term.wait_for(&format!("{env:?}: pushed 1 hidden"), |s| {
        !lines(s).iter().any(|line| line.contains("pushed 1"))
    });
    assert!(
        !lines(&screen).iter().any(|line| line.contains("pushed")),
        "{env:?}: pushed text is still shown:\n{}",
        dump(&screen)
    );
}

#[test]
fn scr_002_a_query_hides_a_pushed_screens_text_in_full_screen() {
    check_query_hides_pushed_text("full", "screen");
}

#[test]
fn scr_002_a_query_hides_a_pushed_modal_screens_text_in_full_screen() {
    check_query_hides_pushed_text("full", "modal");
}

#[test]
fn scr_002_a_query_hides_a_pushed_screens_text_inline() {
    check_query_hides_pushed_text("inline", "screen");
}

#[test]
fn scr_002_a_query_hides_a_pushed_modal_screens_text_inline() {
    check_query_hides_pushed_text("inline", "modal");
}
