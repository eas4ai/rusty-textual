//! PTY checks for the size of the app's screen (`docs/spec/screens.md`).
//!
//! Part of the `screen-pty` Sudus mechanism, run through
//! `scripts/screen_mechanism.py`, which maps each `scr_NNN_` test to its
//! requirement. Each test runs the probe app of `tests/fixtures/inline_probe`
//! in the pseudo terminal of `tests/support/pty.rs`, in a 100x30 terminal or
//! a 512x144 one (a 2560x1440 screen at 5x10 pixels per cell). The probe's
//! `Screen` rule gets `border: tall red` and one set of size rules through
//! `PROBE_SCREEN_RULES`. The screen must still fill the area the app draws
//! in: its border runs along the first and last columns of the terminal and
//! the first and last rows of that area, the whole terminal in full-screen
//! mode and the inline frame in inline mode. Each case has its own test.
//! Run idle and single-threaded, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, SHELL_THEN_EXEC, Term, dump, has_text, lines, probe, row_of};

/// Rows and columns of the two terminals.
const SMALL: (u16, u16) = (30, 100);
const LARGE: (u16, u16) = (144, 512);

/// Size rules that would shrink and move the screen.
const SIZED: &str = "border: tall red; width: 20; height: 10; margin: 2;";
/// Size limits smaller than the area.
const CAPPED: &str = "border: tall red; max-width: 20; max-height: 10;";
/// Size floors larger than either terminal.
const RAISED: &str = "border: tall red; min-width: 600; min-height: 200;";

/// The text of one cell.
fn cell(screen: &vt100::Screen, row: u16, col: u16) -> String {
    screen
        .cell(row, col)
        .map(vt100::Cell::contents)
        .unwrap_or_default()
}

/// The first and last rows of the area the app draws in. Full-screen, that
/// is the whole terminal. Inline, the frame starts after the two shell lines
/// and the probe's one padding line, and ends on the last row with text. A
/// frame as tall as the terminal (INL-004) pushes those lines off the top.
fn area_rows(screen: &vt100::Screen, mode: &str, rows: u16) -> (u16, u16) {
    if mode == "full" {
        return (0, rows - 1);
    }
    let top = row_of(screen, "shell-2").map_or(0, |shell| shell + 2);
    let top = u16::try_from(top).expect("row fits");
    let last = lines(screen)
        .iter()
        .rposition(|line| !line.is_empty())
        .expect("the app draws something");
    (top, u16::try_from(last).expect("row fits"))
}

/// Starts the probe with `rules` in a terminal of `rows` by `cols` and checks
/// that the screen's border runs along the edges of the area it draws in.
fn check_screen_fills_its_area(mode: &str, rules: &str, (rows, cols): (u16, u16)) {
    let env = [("PROBE_MODE", mode), ("PROBE_SCREEN_RULES", rules)];
    let term = Term::spawn_sized(
        SHELL_THEN_EXEC,
        &probe(),
        &env,
        Answers::TERMINAL,
        rows,
        cols,
    );
    term.wait_for("probe status", has_text("keys:0"));
    let screen = term.settle();
    let (top, bottom) = area_rows(&screen, mode, rows);
    let what = format!("{mode} {cols}x{rows} with {rules:?}");
    let mut wrong = Vec::new();
    for row in top..=bottom {
        if cell(&screen, row, 0) != "▊" {
            wrong.push(format!("row {row}: no left border in the first column"));
        }
        if cell(&screen, row, cols - 1) != "▎" {
            wrong.push(format!("row {row}: no right border in the last column"));
        }
    }
    if cell(&screen, top, 1) != "▔" {
        wrong.push(format!("row {top}: no top border on the area's first row"));
    }
    if cell(&screen, bottom, 1) != "▁" {
        wrong.push(format!(
            "row {bottom}: no bottom border on the area's last row"
        ));
    }
    assert!(
        wrong.is_empty(),
        "{what}: the screen does not fill rows {top}..={bottom}:\n{}\n{}",
        wrong.iter().take(6).cloned().collect::<Vec<_>>().join("\n"),
        dump(&screen)
    );
}

#[test]
fn scr_003_size_rules_do_not_shrink_the_screen_in_full_screen_at_100x30() {
    check_screen_fills_its_area("full", SIZED, SMALL);
}

#[test]
fn scr_003_size_rules_do_not_shrink_the_screen_in_full_screen_at_512x144() {
    check_screen_fills_its_area("full", SIZED, LARGE);
}

#[test]
fn scr_003_size_rules_do_not_shrink_the_screen_inline_at_100x30() {
    check_screen_fills_its_area("inline", SIZED, SMALL);
}

#[test]
fn scr_003_size_rules_do_not_shrink_the_screen_inline_at_512x144() {
    check_screen_fills_its_area("inline", SIZED, LARGE);
}

#[test]
fn scr_003_max_size_rules_do_not_shrink_the_screen_in_full_screen_at_100x30() {
    check_screen_fills_its_area("full", CAPPED, SMALL);
}

#[test]
fn scr_003_max_size_rules_do_not_shrink_the_screen_in_full_screen_at_512x144() {
    check_screen_fills_its_area("full", CAPPED, LARGE);
}

#[test]
fn scr_003_max_size_rules_do_not_shrink_the_screen_inline_at_100x30() {
    check_screen_fills_its_area("inline", CAPPED, SMALL);
}

#[test]
fn scr_003_max_size_rules_do_not_shrink_the_screen_inline_at_512x144() {
    check_screen_fills_its_area("inline", CAPPED, LARGE);
}

#[test]
fn scr_003_min_size_rules_do_not_grow_the_screen_in_full_screen_at_100x30() {
    check_screen_fills_its_area("full", RAISED, SMALL);
}

#[test]
fn scr_003_min_size_rules_do_not_grow_the_screen_in_full_screen_at_512x144() {
    check_screen_fills_its_area("full", RAISED, LARGE);
}

#[test]
fn scr_003_min_size_rules_do_not_grow_the_screen_inline_at_100x30() {
    check_screen_fills_its_area("inline", RAISED, SMALL);
}

#[test]
fn scr_003_min_size_rules_do_not_grow_the_screen_inline_at_512x144() {
    check_screen_fills_its_area("inline", RAISED, LARGE);
}
