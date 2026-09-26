//! PTY checks for widget updates (`docs/spec/updates.md`).
//!
//! This file is the `update-pty` Sudus mechanism, run through
//! `scripts/update_mechanism.py`, which maps each `upd_NNN_` test to its
//! requirement. Each test runs the probe app of `tests/fixtures/inline_probe`
//! in the pseudo terminal of `tests/support/pty.rs`. The probe's key handler
//! counts every key it does not bind, and its message handler notes the
//! pointer moving over its hover line. Both write into the status line
//! (`keys:N ... hovered`) with `App::with_query_one_mut_as`, which keeps the
//! line's size. Each case has its own test, so a failing run shows every case
//! that fails. Run idle and single-threaded, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, SHELL_THEN_EXEC, Term, dump, has_text, lines, probe, row_of};

const FULL: &[(&str, &str)] = &[("PROBE_MODE", "full")];
const FULL_BUTTON: &[(&str, &str)] = &[("PROBE_MODE", "full"), ("PROBE_BUTTON", "1")];
const INLINE: &[(&str, &str)] = &[("PROBE_MODE", "inline")];
const INLINE_BUTTON: &[(&str, &str)] = &[("PROBE_MODE", "inline"), ("PROBE_BUTTON", "1")];
const FULL_HOVER: &[(&str, &str)] = &[("PROBE_MODE", "full"), ("PROBE_HOVER", "1")];
const INLINE_HOVER: &[(&str, &str)] = &[("PROBE_MODE", "inline"), ("PROBE_HOVER", "1")];

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

/// Presses a key the probe does not bind.
fn unbound_key(term: &Term, _: &vt100::Screen) {
    term.send(b"e");
}

/// Moves the pointer onto the probe's hover line. The move changes the
/// hovered widget, so the frame drawn for it at once repaints only the
/// hovered widgets, and the probe updates its status line during that move.
fn hover_line(term: &Term, screen: &vt100::Screen) {
    let row = row_of(screen, "hover here").expect("hover line row");
    let y = row + 1; // 1-based SGR row
    term.send(format!("\x1b[<35;3;{y}M").as_bytes());
}

#[test]
fn upd_001_a_key_handler_update_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL, unbound_key, "keys:1");
}

#[test]
fn upd_001_a_key_handler_update_is_redrawn_in_full_screen_with_a_focused_button() {
    check_status_redrawn(FULL_BUTTON, unbound_key, "keys:1");
}

#[test]
fn upd_001_a_key_handler_update_is_redrawn_inline() {
    check_status_redrawn(INLINE, unbound_key, "keys:1");
}

#[test]
fn upd_001_a_key_handler_update_is_redrawn_inline_with_a_focused_button() {
    check_status_redrawn(INLINE_BUTTON, unbound_key, "keys:1");
}

#[test]
fn upd_001_a_hover_message_handler_update_is_redrawn_in_full_screen() {
    check_status_redrawn(FULL_HOVER, hover_line, "hovered");
}

#[test]
fn upd_001_a_hover_message_handler_update_is_redrawn_inline() {
    check_status_redrawn(INLINE_HOVER, hover_line, "hovered");
}

