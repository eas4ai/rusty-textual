//! PTY checks for changes an app makes through a query
//! (`docs/spec/updates.md`, UPD-003 to UPD-005).
//!
//! This file is the `query-pty` Sudus mechanism, run through
//! `scripts/query_mechanism.py`, which maps each `upd_NNN_` test to its
//! requirement. Each test runs the probe app of `tests/fixtures/inline_probe`
//! with `PROBE_QUERY` set in the pseudo terminal of `tests/support/pty.rs`.
//! The probe then shows a `css-hidden` line its stylesheet hides, and three
//! buttons, `One`, `Two` and `Three`. The focused button draws its label in
//! reverse video, as Python's `Button:focus` rule does.
//!
//! UPD-003: `d` shows the `css-hidden` line with `set_display(true)`.
//!
//! UPD-004: `l` sets `loading` and `f` asks for a repaint, each on a query
//! that matches nothing; the terminal must not receive a screen clear.
//!
//! UPD-005: the checks move focus with Tab, then `k`, `n` or `m` removes the
//! focused button (`PROBE_REMOVE`) through `App::remove`, `App::remove_node`
//! or `DomQueryMut::remove`.
//!
//! Each case has its own test, so a failing run shows every case that
//! fails. Run idle and single-threaded, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, SHELL_THEN_EXEC, Term, contains, dump, has_text, lines, probe, row_of};

/// A full-screen clear (`CSI 2 J`).
const CLEAR: &[u8] = b"\x1b[2J";

/// The probe's button labels, in the order it shows them.
const LABELS: [&str; 3] = ["One", "Two", "Three"];

/// Starts the probe with `PROBE_QUERY` and `env` in `mode` and waits for its
/// first frame to settle.
fn spawn_query_probe(mode: &str, env: &[(&str, &str)]) -> Term {
    let mut env = env.to_vec();
    env.extend([("PROBE_MODE", mode), ("PROBE_QUERY", "1")]);
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), &env, Answers::TERMINAL);
    term.wait_for(&format!("{env:?}: probe status"), has_text("keys:0"));
    term.wait_for(&format!("{env:?}: the buttons"), has_text("Three"));
    term.settle();
    term
}

fn check_query_shows_a_css_hidden_line(mode: &str) {
    let term = spawn_query_probe(mode, &[]);
    let screen = term.screen();
    assert!(
        row_of(&screen, "css-hidden").is_none(),
        "{mode}: the stylesheet does not hide the css-hidden line:\n{}",
        dump(&screen)
    );
    term.send(b"d");
    term.wait_for(
        &format!("{mode}: css-hidden shown by set_display(true)"),
        has_text("css-hidden"),
    );
}

#[test]
fn upd_003_set_display_shows_a_line_the_stylesheet_hides_in_full_screen() {
    check_query_shows_a_css_hidden_line("full");
}

#[test]
fn upd_003_set_display_shows_a_line_the_stylesheet_hides_inline() {
    check_query_shows_a_css_hidden_line("inline");
}

/// Presses `key`, a change on a query that matches nothing, then `e`, which
/// the probe counts, and checks that the terminal received no screen clear
/// in between.
fn check_empty_query_does_not_clear(key: &[u8]) {
    let term = spawn_query_probe("full", &[]);
    let start = term.raw().len();
    term.send(key);
    term.send(b"e");
    // `e` comes after `key`, so once its count is drawn, the frame after
    // `key` has been drawn too.
    term.wait_for("keys:1 on the status line", has_text("keys:1 "));
    term.settle();
    let written = term.raw()[start..].to_vec();
    assert!(
        !contains(&written, CLEAR),
        "{}: the terminal received a screen clear: {}",
        String::from_utf8_lossy(key),
        String::from_utf8_lossy(&written).escape_debug()
    );
}

#[test]
fn upd_004_loading_on_an_empty_query_does_not_clear_the_screen() {
    check_empty_query_does_not_clear(b"l");
}

#[test]
fn upd_004_refresh_on_an_empty_query_does_not_clear_the_screen() {
    check_empty_query_does_not_clear(b"f");
}

/// The label of the button drawn as focused: the one whose label is in
/// reverse video.
fn focused_label(screen: &vt100::Screen) -> Option<&'static str> {
    let rows = lines(screen);
    LABELS.into_iter().find(|label| {
        rows.iter().enumerate().any(|(row, line)| {
            line.find(label).is_some_and(|at| {
                let col = line[..at].chars().count();
                let (row, col) = (u16::try_from(row), u16::try_from(col));
                matches!((row, col), (Ok(row), Ok(col)) if screen
                    .cell(row, col)
                    .is_some_and(vt100::Cell::inverse))
            })
        })
    })
}

/// Moves focus with Tab until the button labelled `label` is drawn focused.
fn focus_button(term: &Term, label: &str) {
    for _ in 0..4 {
        let screen = term.settle();
        let focused = focused_label(&screen);
        if focused == Some(label) {
            return;
        }
        term.send(b"\t");
        term.wait_for(&format!("Tab to move focus from {focused:?}"), |s| {
            focused_label(s) != focused
        });
    }
    panic!("Tab never focused {label}:\n{}", dump(&term.screen()));
}

/// Focuses the button `removed` names (id, label), removes it with `key`,
/// and checks that the button labelled `expected` is drawn focused then.
fn check_removal_moves_focus(mode: &str, key: &[u8], removed: (&str, &str), expected: &str) {
    let (id, label) = removed;
    let term = spawn_query_probe(mode, &[("PROBE_REMOVE", id)]);
    focus_button(&term, label);
    term.send(key);
    let removal = String::from_utf8_lossy(key);
    term.wait_for(&format!("{mode}: {removal} removes {label}"), |s| {
        row_of(s, label).is_none()
    });
    let screen = term.settle();
    assert_eq!(
        focused_label(&screen),
        Some(expected),
        "{mode}: after {removal} removed the focused {label}, {expected} is not focused:\n{}",
        dump(&screen)
    );
}

#[test]
fn upd_005_app_remove_of_the_second_button_focuses_the_first_in_full_screen() {
    check_removal_moves_focus("full", b"k", ("two", "Two"), "One");
}

#[test]
fn upd_005_app_remove_of_the_first_button_focuses_the_last_in_full_screen() {
    check_removal_moves_focus("full", b"k", ("one", "One"), "Three");
}

#[test]
fn upd_005_app_remove_of_the_second_button_focuses_the_first_inline() {
    check_removal_moves_focus("inline", b"k", ("two", "Two"), "One");
}

#[test]
fn upd_005_app_remove_of_the_first_button_focuses_the_last_inline() {
    check_removal_moves_focus("inline", b"k", ("one", "One"), "Three");
}

#[test]
fn upd_005_remove_node_of_the_second_button_focuses_the_first_in_full_screen() {
    check_removal_moves_focus("full", b"n", ("two", "Two"), "One");
}

#[test]
fn upd_005_remove_node_of_the_first_button_focuses_the_last_in_full_screen() {
    check_removal_moves_focus("full", b"n", ("one", "One"), "Three");
}

#[test]
fn upd_005_remove_node_of_the_second_button_focuses_the_first_inline() {
    check_removal_moves_focus("inline", b"n", ("two", "Two"), "One");
}

#[test]
fn upd_005_remove_node_of_the_first_button_focuses_the_last_inline() {
    check_removal_moves_focus("inline", b"n", ("one", "One"), "Three");
}

#[test]
fn upd_005_query_remove_of_the_second_button_focuses_the_first_in_full_screen() {
    check_removal_moves_focus("full", b"m", ("two", "Two"), "One");
}

#[test]
fn upd_005_query_remove_of_the_first_button_focuses_the_last_in_full_screen() {
    check_removal_moves_focus("full", b"m", ("one", "One"), "Three");
}

#[test]
fn upd_005_query_remove_of_the_second_button_focuses_the_first_inline() {
    check_removal_moves_focus("inline", b"m", ("two", "Two"), "One");
}

#[test]
fn upd_005_query_remove_of_the_first_button_focuses_the_last_inline() {
    check_removal_moves_focus("inline", b"m", ("one", "One"), "Three");
}
