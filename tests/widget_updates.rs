//! PTY checks for widget updates (`docs/spec/updates.md`).
//!
//! This file is the `update-pty` Sudus mechanism, run through
//! `scripts/update_mechanism.py`, which maps each `upd_NNN_` test to its
//! requirement. Each test runs the probe app of `tests/fixtures/inline_probe`
//! in the pseudo terminal of `tests/support/pty.rs`.
//!
//! UPD-001: the probe's key handler counts every key it does not bind, and
//! its message handler notes the pointer moving over its hover line. Both
//! write into the status line (`keys:N ... hovered`), which keeps its size
//! for a key. The key chooses the update path (`1` `with_widget_mut`, `2`
//! `with_widget_mut_as`, `3` `with_query_one_mut`, `5` `with_widget_taken_as`,
//! `6` `query_mut` then `DomQueryMut::update`, any other key
//! `with_query_one_mut_as`); `PROBE_HOVER_PATH` names the key whose path the
//! message handler uses.
//!
//! UPD-002: the probe's shared line draws a count kept outside the widget.
//! `c` changes it without asking for a repaint, a hover elsewhere draws a
//! frame while it is changed, and `r` repaints the line.
//!
//! Each case has its own test, so a failing run shows every case that fails.
//! Run idle and single-threaded, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, SHELL_THEN_EXEC, Term, dump, has_text, lines, probe, row_of};

const FULL: &[(&str, &str)] = &[("PROBE_MODE", "full")];
const FULL_BUTTON: &[(&str, &str)] = &[("PROBE_MODE", "full"), ("PROBE_BUTTON", "1")];
const INLINE: &[(&str, &str)] = &[("PROBE_MODE", "inline")];
const INLINE_BUTTON: &[(&str, &str)] = &[("PROBE_MODE", "inline"), ("PROBE_BUTTON", "1")];
const FULL_HOVER: &[(&str, &str)] = &[("PROBE_MODE", "full"), ("PROBE_HOVER", "1")];
const INLINE_HOVER: &[(&str, &str)] = &[("PROBE_MODE", "inline"), ("PROBE_HOVER", "1")];
const FULL_HOVER_TAKEN: &[(&str, &str)] = &[
    ("PROBE_MODE", "full"),
    ("PROBE_HOVER", "1"),
    ("PROBE_HOVER_PATH", "5"),
];
const INLINE_HOVER_TAKEN: &[(&str, &str)] = &[
    ("PROBE_MODE", "inline"),
    ("PROBE_HOVER", "1"),
    ("PROBE_HOVER_PATH", "5"),
];
const FULL_HOVER_QUERY: &[(&str, &str)] = &[
    ("PROBE_MODE", "full"),
    ("PROBE_HOVER", "1"),
    ("PROBE_HOVER_PATH", "6"),
];
const INLINE_HOVER_QUERY: &[(&str, &str)] = &[
    ("PROBE_MODE", "inline"),
    ("PROBE_HOVER", "1"),
    ("PROBE_HOVER_PATH", "6"),
];
const FULL_SHARED: &[(&str, &str)] = &[("PROBE_MODE", "full"), ("PROBE_SHARED", "1")];
const INLINE_SHARED: &[(&str, &str)] = &[("PROBE_MODE", "inline"), ("PROBE_SHARED", "1")];

/// Starts the probe with `env`, runs `act` on the settled screen, then waits
/// until the row that showed the status line shows `after`, and checks that
/// the old status line is gone from every row.
fn check_status_redrawn(
    env: &[(&str, &str)],
    act: impl FnOnce(&Term, &vt100::Screen),
    after: &str,
) {
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), env, Answers::TERMINAL);
    term.wait_for("probe status", has_text("keys:0"));
    let screen = term.settle();
    let row = row_of(&screen, "keys:0").expect("status row");
    let before = lines(&screen)[row].clone();
    act(&term, &screen);
    let screen = term.wait_for(&format!("{env:?}: {after} on the status row"), |s| {
        lines(s).get(row).is_some_and(|line| line.contains(after))
    });
    assert!(
        !lines(&screen).contains(&before),
        "{env:?}: the old status line is still shown:\n{}",
        dump(&screen)
    );
}

/// Presses `key`; the probe counts it and writes the status line through the
/// update path the key names.
fn press(key: &'static [u8]) -> impl FnOnce(&Term, &vt100::Screen) {
    move |term, _| term.send(key)
}

/// Moves the pointer onto the start of the first row that shows `text`.
fn move_pointer_to(term: &Term, screen: &vt100::Screen, text: &str) {
    let row = row_of(screen, text).unwrap_or_else(|| panic!("no row shows {text}"));
    let y = row + 1; // 1-based SGR row
    term.send(format!("\x1b[<35;3;{y}M").as_bytes());
}

/// Moves the pointer onto the probe's hover line. The move changes the
/// hovered widget, so the frame drawn for it at once repaints only the
/// hovered widgets, and the probe updates its status line during that move.
fn hover_line(term: &Term, screen: &vt100::Screen) {
    move_pointer_to(term, screen, "hover here");
}

/// Changes what the shared line draws without asking for a repaint, draws a
/// frame for a hover on the body while it is changed, then repaints the line
/// and waits for it to show the new count.
fn check_repaint_shows_a_skipped_change(env: &[(&str, &str)]) {
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), env, Answers::TERMINAL);
    term.wait_for("the shared line", has_text("shared:0"));
    let screen = term.settle();
    let row = row_of(&screen, "shared:0").expect("shared row");
    term.send(b"c");
    let screen = term.settle();
    assert!(
        lines(&screen)[row].contains("shared:0"),
        "{env:?}: a frame was drawn after c, so no change is left unwritten:\n{}",
        dump(&screen)
    );
    // The hover changes from nothing to the body, so this frame repaints
    // only the body's rows, well above the shared line.
    move_pointer_to(&term, &screen, "line 1");
    term.settle();
    term.send(b"r");
    term.wait_for(&format!("{env:?}: shared:1 on the shared row"), |s| {
        lines(s)
            .get(row)
            .is_some_and(|line| line.contains("shared:1"))
    });
}

#[test]
fn upd_001_a_key_handler_update_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL, press(b"e"), "keys:1");
}

#[test]
fn upd_001_a_key_handler_update_is_redrawn_in_full_screen_with_a_focused_button() {
    check_status_redrawn(FULL_BUTTON, press(b"e"), "keys:1");
}

#[test]
fn upd_001_a_key_handler_update_is_redrawn_inline() {
    check_status_redrawn(INLINE, press(b"e"), "keys:1");
}

#[test]
fn upd_001_a_key_handler_update_is_redrawn_inline_with_a_focused_button() {
    check_status_redrawn(INLINE_BUTTON, press(b"e"), "keys:1");
}

#[test]
fn upd_001_a_hover_message_handler_update_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL_HOVER, hover_line, "hovered");
}

#[test]
fn upd_001_a_hover_message_handler_update_is_redrawn_inline() {
    check_status_redrawn(INLINE_HOVER, hover_line, "hovered");
}

#[test]
fn upd_001_with_widget_taken_as_from_a_hover_message_handler_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL_HOVER_TAKEN, hover_line, "hovered");
}

#[test]
fn upd_001_with_widget_taken_as_from_a_hover_message_handler_is_redrawn_inline() {
    check_status_redrawn(INLINE_HOVER_TAKEN, hover_line, "hovered");
}

#[test]
fn upd_001_a_query_update_from_a_hover_message_handler_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL_HOVER_QUERY, hover_line, "hovered");
}

#[test]
fn upd_001_a_query_update_from_a_hover_message_handler_is_redrawn_inline() {
    check_status_redrawn(INLINE_HOVER_QUERY, hover_line, "hovered");
}

#[test]
fn upd_001_with_widget_mut_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL, press(b"1"), "keys:1");
}

#[test]
fn upd_001_with_widget_mut_is_redrawn_inline() {
    check_status_redrawn(INLINE, press(b"1"), "keys:1");
}

#[test]
fn upd_001_with_widget_mut_as_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL, press(b"2"), "keys:1");
}

#[test]
fn upd_001_with_widget_mut_as_is_redrawn_inline() {
    check_status_redrawn(INLINE, press(b"2"), "keys:1");
}

#[test]
fn upd_001_with_query_one_mut_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL, press(b"3"), "keys:1");
}

#[test]
fn upd_001_with_query_one_mut_is_redrawn_inline() {
    check_status_redrawn(INLINE, press(b"3"), "keys:1");
}

#[test]
fn upd_001_with_widget_taken_as_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL, press(b"5"), "keys:1");
}

#[test]
fn upd_001_with_widget_taken_as_is_redrawn_inline() {
    check_status_redrawn(INLINE, press(b"5"), "keys:1");
}

#[test]
fn upd_001_a_query_update_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL, press(b"6"), "keys:1");
}

#[test]
fn upd_001_a_query_update_is_redrawn_inline() {
    check_status_redrawn(INLINE, press(b"6"), "keys:1");
}

#[test]
fn upd_002_a_repaint_shows_a_change_an_earlier_frame_skipped_in_full_screen() {
    check_repaint_shows_a_skipped_change(FULL_SHARED);
}

#[test]
fn upd_002_a_repaint_shows_a_change_an_earlier_frame_skipped_inline() {
    check_repaint_shows_a_skipped_change(INLINE_SHARED);
}
