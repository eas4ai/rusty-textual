//! PTY checks for the mouse wheel and the scrollbars (`docs/spec/scrolling.md`).
//!
//! Part of the `scroll-pty` Sudus mechanism, run through
//! `scripts/scroll_mechanism.py`, which maps each `scl_NNN_` test to its
//! requirement. Each test runs the probe app of `tests/fixtures/inline_probe`
//! in the pseudo terminal of `tests/support/pty.rs`. `PROBE_SCROLL` puts one
//! scrolling widget, 10 rows tall and holding `item 1` to `item 60`, above
//! the probe's body; `PROBE_LINES=60` makes the app's own screen taller than
//! the terminal, and `PROBE_PUSH` pushes a 60-line screen (`pushed 1` to
//! `pushed 60`) on `p`.
//!
//! SCL-001: one wheel notch over a widget moves a line it shows by exactly 2
//! rows; a shift or ctrl notch, or a wheel-right notch, moves a token by
//! exactly 4 columns; a notch over a Log already at its end scrolls the
//! screen around it. The checks find each line or token by its text before
//! and after the notch, so they hold in full-screen and in inline mode.
//!
//! SCL-002: a click on a scrollbar's track past its thumb pages the content
//! with an animation. The check replays what the terminal received after the
//! click, frame by frame (each frame ends with the synchronized-output end,
//! `CSI ? 2026 l`), and looks for a frame between the old and the new page.
//!
//! Each case has its own test, so a failing run shows every case that fails.
//! Run idle and single-threaded, like the other PTY tests.

use std::fmt::Write as _;

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, COLS, ROWS, SHELL_THEN_EXEC, Term, dump, has_text, lines, probe};

/// SGR mouse buttons for wheel notches and the left button.
const WHEEL_UP: u8 = 64;
const WHEEL_DOWN: u8 = 65;
const WHEEL_RIGHT: u8 = 67;
const SHIFT: u8 = 4;
const CTRL: u8 = 16;
const LEFT_BUTTON: u8 = 0;

/// The end of a synchronized-output frame.
const FRAME_END: &[u8] = b"\x1b[?2026l";

/// How long a check waits for a notch or a click to move anything. A case
/// that moves nothing is checked when this ends, not at the harness's
/// longer timeout.
const MOVE_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Polls the screen until `pred` holds or [`MOVE_WAIT`] ends.
fn wait_briefly(term: &Term, pred: impl Fn(&vt100::Screen) -> bool) {
    let start = std::time::Instant::now();
    while start.elapsed() < MOVE_WAIT && !pred(&term.screen()) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Starts the probe in `mode` with `env`, waits for `ready` on screen and
/// for the screen to settle.
fn spawn(mode: &str, env: &[(&str, &str)], ready: &'static str) -> Term {
    let mut env = env.to_vec();
    env.push(("PROBE_MODE", mode));
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), &env, Answers::TERMINAL);
    term.wait_for(&format!("{env:?}: {ready}"), has_text(ready));
    term.settle();
    term
}

/// Starts the probe with its 60-line screen pushed.
fn spawn_pushed(mode: &str) -> Term {
    let term = spawn(mode, &[("PROBE_PUSH", "screen")], "keys:0");
    term.send(b"p");
    term.wait_for(&format!("{mode}: the pushed screen"), has_text("pushed 1"));
    term.settle();
    term
}

/// Where `token` is shown as a whole word, the digits after it included:
/// `(row, column)` of its first character.
fn find(screen: &vt100::Screen, token: &str) -> Option<(usize, usize)> {
    lines(screen).iter().enumerate().find_map(|(row, line)| {
        line.match_indices(token).find_map(|(at, _)| {
            let next = line[at + token.len()..].chars().next();
            let whole = !next.is_some_and(|c| c.is_ascii_digit());
            whole.then(|| (row, line[..at].chars().count()))
        })
    })
}

/// The position of `token` on screen, or a panic that shows the screen.
fn position(screen: &vt100::Screen, token: &str, what: &str) -> (usize, usize) {
    find(screen, token)
        .unwrap_or_else(|| panic!("{what}: {token} is not on screen:\n{}", dump(screen)))
}

/// Sends one SGR mouse event with `button` at a 0-based cell.
fn mouse(term: &Term, button: u8, (row, col): (usize, usize), release: bool) {
    let end = if release { 'm' } else { 'M' };
    term.send(format!("\x1b[<{button};{};{}{end}", col + 1, row + 1).as_bytes());
}

/// Sends one wheel notch at `at`, waits for `token` to move, lets the screen
/// settle and checks that it moved by `rows` rows and `cols` columns.
fn check_notch(
    term: &Term,
    what: &str,
    button: u8,
    at: (usize, usize),
    token: &str,
    moved: (isize, isize),
) {
    let before = position(&term.screen(), token, what);
    mouse(term, button, at, false);
    wait_briefly(term, |s| find(s, token) != Some(before));
    let screen = term.settle();
    let after = position(&screen, token, what);
    let signed = |n: usize| isize::try_from(n).expect("a screen offset");
    let delta = (
        signed(after.0) - signed(before.0),
        signed(after.1) - signed(before.1),
    );
    assert_eq!(
        delta,
        moved,
        "{what}: one notch moved {token} by (rows, columns) {delta:?}, not {moved:?}:\n{}",
        dump(&screen)
    );
}

// -- SCL-001: vertical notches ---------------------------------------------

/// Starts the probe with the `PROBE_SCROLL` widget `kind` and turns the
/// wheel once over its line `item 5`, or `item 55` for a widget that starts
/// at its end, which is turned up instead.
fn check_widget_notch(mode: &str, kind: &str) {
    let at_end = matches!(kind, "log" | "rich-log");
    let term = spawn(mode, &[("PROBE_SCROLL", kind)], "keys:0");
    let (token, button, moved) = if at_end {
        ("item 55", WHEEL_UP, 2)
    } else {
        ("item 5", WHEEL_DOWN, -2)
    };
    let (row, col) = position(&term.screen(), token, kind);
    check_notch(
        &term,
        &format!("{mode} {kind}"),
        button,
        (row, col + 2),
        token,
        (moved, 0),
    );
}

macro_rules! widget_notch_tests {
    ($($name:ident: $mode:literal, $kind:literal;)*) => {
        $(
            #[test]
            fn $name() {
                check_widget_notch($mode, $kind);
            }
        )*
    };
}

widget_notch_tests! {
    scl_001_a_notch_scrolls_a_container_2_lines_in_full_screen: "full", "container";
    scl_001_a_notch_scrolls_a_container_2_lines_inline: "inline", "container";
    scl_001_a_notch_scrolls_a_vertical_scroll_2_lines_in_full_screen: "full", "vertical-scroll";
    scl_001_a_notch_scrolls_a_vertical_scroll_2_lines_inline: "inline", "vertical-scroll";
    scl_001_a_notch_scrolls_a_log_2_lines_in_full_screen: "full", "log";
    scl_001_a_notch_scrolls_a_log_2_lines_inline: "inline", "log";
    scl_001_a_notch_scrolls_a_rich_log_2_lines_in_full_screen: "full", "rich-log";
    scl_001_a_notch_scrolls_a_rich_log_2_lines_inline: "inline", "rich-log";
    scl_001_a_notch_scrolls_an_option_list_2_lines_in_full_screen: "full", "option-list";
    scl_001_a_notch_scrolls_an_option_list_2_lines_inline: "inline", "option-list";
    scl_001_a_notch_scrolls_a_selection_list_2_lines_in_full_screen: "full", "selection-list";
    scl_001_a_notch_scrolls_a_selection_list_2_lines_inline: "inline", "selection-list";
    scl_001_a_notch_scrolls_a_list_view_2_lines_in_full_screen: "full", "list-view";
    scl_001_a_notch_scrolls_a_list_view_2_lines_inline: "inline", "list-view";
    scl_001_a_notch_scrolls_a_tree_2_lines_in_full_screen: "full", "tree";
    scl_001_a_notch_scrolls_a_tree_2_lines_inline: "inline", "tree";
    scl_001_a_notch_scrolls_a_data_table_2_lines_in_full_screen: "full", "data-table";
    scl_001_a_notch_scrolls_a_data_table_2_lines_inline: "inline", "data-table";
    scl_001_a_notch_scrolls_a_key_panel_2_lines_in_full_screen: "full", "key-panel";
    scl_001_a_notch_scrolls_a_key_panel_2_lines_inline: "inline", "key-panel";
}

fn check_app_screen_notch(mode: &str) {
    let term = spawn(mode, &[("PROBE_LINES", "60")], "line 1");
    let (row, col) = position(&term.screen(), "line 5", mode);
    check_notch(
        &term,
        &format!("{mode} app screen"),
        WHEEL_DOWN,
        (row, col + 2),
        "line 5",
        (-2, 0),
    );
}

#[test]
fn scl_001_a_notch_scrolls_the_app_screen_2_lines_in_full_screen() {
    check_app_screen_notch("full");
}

#[test]
fn scl_001_a_notch_scrolls_the_app_screen_2_lines_inline() {
    check_app_screen_notch("inline");
}

fn check_pushed_screen_notch(mode: &str) {
    let term = spawn_pushed(mode);
    let (row, col) = position(&term.screen(), "pushed 5", mode);
    check_notch(
        &term,
        &format!("{mode} pushed screen"),
        WHEEL_DOWN,
        (row, col + 2),
        "pushed 5",
        (-2, 0),
    );
}

#[test]
fn scl_001_a_notch_scrolls_a_pushed_screen_2_lines_in_full_screen() {
    check_pushed_screen_notch("full");
}

#[test]
fn scl_001_a_notch_scrolls_a_pushed_screen_2_lines_inline() {
    check_pushed_screen_notch("inline");
}

// -- SCL-001: horizontal notches -------------------------------------------

/// Starts the probe with the wide `PROBE_SCROLL` widget `kind` and sends one
/// `button` notch over it; the token `c010` (or the column label `c05` of a
/// `DataTable`) must move 4 columns left.
fn check_horizontal_notch(mode: &str, kind: &str, button: u8) {
    let term = spawn(mode, &[("PROBE_SCROLL", kind)], "keys:0");
    let token = if kind == "data-table" { "c05" } else { "c010" };
    let (row, _) = position(&term.screen(), "item 3", kind);
    check_notch(
        &term,
        &format!("{mode} {kind} button {button}"),
        button,
        (row, 2),
        token,
        (0, -4),
    );
}

macro_rules! horizontal_notch_tests {
    ($($name:ident: $mode:literal, $kind:literal, $button:expr;)*) => {
        $(
            #[test]
            fn $name() {
                check_horizontal_notch($mode, $kind, $button);
            }
        )*
    };
}

horizontal_notch_tests! {
    scl_001_shift_wheel_scrolls_a_container_4_columns_in_full_screen: "full", "container", WHEEL_DOWN + SHIFT;
    scl_001_shift_wheel_scrolls_a_container_4_columns_inline: "inline", "container", WHEEL_DOWN + SHIFT;
    scl_001_ctrl_wheel_scrolls_a_container_4_columns_in_full_screen: "full", "container", WHEEL_DOWN + CTRL;
    scl_001_ctrl_wheel_scrolls_a_container_4_columns_inline: "inline", "container", WHEEL_DOWN + CTRL;
    scl_001_wheel_right_scrolls_a_container_4_columns_in_full_screen: "full", "container", WHEEL_RIGHT;
    scl_001_wheel_right_scrolls_a_container_4_columns_inline: "inline", "container", WHEEL_RIGHT;
    scl_001_shift_wheel_scrolls_a_horizontal_scroll_4_columns_in_full_screen: "full", "horizontal-scroll", WHEEL_DOWN + SHIFT;
    scl_001_shift_wheel_scrolls_a_horizontal_scroll_4_columns_inline: "inline", "horizontal-scroll", WHEEL_DOWN + SHIFT;
    scl_001_ctrl_wheel_scrolls_a_horizontal_scroll_4_columns_in_full_screen: "full", "horizontal-scroll", WHEEL_DOWN + CTRL;
    scl_001_ctrl_wheel_scrolls_a_horizontal_scroll_4_columns_inline: "inline", "horizontal-scroll", WHEEL_DOWN + CTRL;
    scl_001_wheel_right_scrolls_a_horizontal_scroll_4_columns_in_full_screen: "full", "horizontal-scroll", WHEEL_RIGHT;
    scl_001_wheel_right_scrolls_a_horizontal_scroll_4_columns_inline: "inline", "horizontal-scroll", WHEEL_RIGHT;
    scl_001_shift_wheel_scrolls_a_data_table_4_columns_in_full_screen: "full", "data-table", WHEEL_DOWN + SHIFT;
    scl_001_shift_wheel_scrolls_a_data_table_4_columns_inline: "inline", "data-table", WHEEL_DOWN + SHIFT;
    scl_001_ctrl_wheel_scrolls_a_data_table_4_columns_in_full_screen: "full", "data-table", WHEEL_DOWN + CTRL;
    scl_001_ctrl_wheel_scrolls_a_data_table_4_columns_inline: "inline", "data-table", WHEEL_DOWN + CTRL;
    scl_001_wheel_right_scrolls_a_data_table_4_columns_in_full_screen: "full", "data-table", WHEEL_RIGHT;
    scl_001_wheel_right_scrolls_a_data_table_4_columns_inline: "inline", "data-table", WHEEL_RIGHT;
}

// -- SCL-001: notches during a scroll animation -----------------------------

/// The first `item N` a screen shows, smallest `N` first.
fn first_item(screen: &vt100::Screen) -> Option<usize> {
    lines(screen)
        .iter()
        .filter_map(|line| {
            let at = line.find("item ")?;
            let digits: String = line[at + 5..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().ok()
        })
        .min()
}

/// Starts the probe with a Container and clicks its scrollbar track below
/// the thumb; with `notch`, a wheel-down notch goes in the same write, while
/// the page animates. Returns the first item shown once the screen settles.
fn page_container(mode: &str, notch: bool) -> usize {
    let term = spawn(mode, &[("PROBE_SCROLL", "container")], "keys:0");
    let screen = term.screen();
    let (row, _) = position(&screen, "item 8", mode);
    let track = (row, from_right(1));
    let mut bytes = format!(
        "\x1b[<0;{};{}M\x1b[<0;{};{}m",
        track.1 + 1,
        row + 1,
        track.1 + 1,
        row + 1
    );
    if notch {
        let _ = write!(bytes, "\x1b[<{WHEEL_DOWN};3;{}M", row + 1);
    }
    term.send(bytes.as_bytes());
    wait_briefly(&term, |s| first_item(s) != Some(1));
    first_item(&term.settle()).expect("items on screen")
}

/// A wheel notch during a track-click page scrolls 2 lines past the page,
/// as Python stops the page at its end first.
fn check_notch_during_a_page(mode: &str) {
    let paged = page_container(mode, false);
    assert!(paged > 1, "{mode}: the track click did not page");
    let with_notch = page_container(mode, true);
    assert_eq!(
        with_notch,
        paged + 2,
        "{mode}: a notch during the page to item {paged} ended on item {with_notch}"
    );
}

#[test]
fn scl_001_a_notch_during_a_page_scrolls_2_lines_past_it_in_full_screen() {
    check_notch_during_a_page("full");
}

#[test]
fn scl_001_a_notch_during_a_page_scrolls_2_lines_past_it_inline() {
    check_notch_during_a_page("inline");
}

/// Two wheel-right notches in one write scroll 8 columns: the second adds to
/// where the first one's animation is heading.
fn check_quick_horizontal_notches(mode: &str) {
    let term = spawn(mode, &[("PROBE_SCROLL", "container")], "keys:0");
    let screen = term.screen();
    let (row, _) = position(&screen, "item 3", mode);
    let before = position(&screen, "c010", mode);
    let notch = format!("\x1b[<{WHEEL_RIGHT};3;{}M", row + 1);
    term.send(format!("{notch}{notch}").as_bytes());
    wait_briefly(&term, |s| {
        find(s, "c010").is_some_and(|at| at.1 + 8 <= before.1)
    });
    let after = position(&term.settle(), "c010", mode);
    assert_eq!(
        before.1 - after.1,
        8,
        "{mode}: two quick wheel-right notches moved c010 from column {} to {}",
        before.1,
        after.1
    );
}

#[test]
fn scl_001_two_quick_horizontal_notches_scroll_8_columns_in_full_screen() {
    check_quick_horizontal_notches("full");
}

#[test]
fn scl_001_two_quick_horizontal_notches_scroll_8_columns_inline() {
    check_quick_horizontal_notches("inline");
}

// -- SCL-001: a notch a widget cannot use -----------------------------------

/// A Log at its end, above 60 body lines: a wheel-down notch over the Log
/// must scroll the app's screen instead.
fn check_log_at_end_passes_the_notch_on(mode: &str) {
    let term = spawn(
        mode,
        &[("PROBE_SCROLL", "log"), ("PROBE_LINES", "60")],
        "line 1",
    );
    let (row, col) = position(&term.screen(), "item 55", mode);
    check_notch(
        &term,
        &format!("{mode} log at its end"),
        WHEEL_DOWN,
        (row, col + 2),
        "line 5",
        (-2, 0),
    );
}

#[test]
fn scl_001_a_log_at_its_end_passes_a_notch_to_the_screen_in_full_screen() {
    check_log_at_end_passes_the_notch_on("full");
}

#[test]
fn scl_001_a_log_at_its_end_passes_a_notch_to_the_screen_inline() {
    check_log_at_end_passes_the_notch_on("inline");
}

// -- SCL-002: track clicks animate -----------------------------------------

/// The furthest `prefix N` (`item`, `line`, `pushed` or the `DataTable`'s
/// `c` column labels) the screen shows, when the numbers it shows run in
/// reading order without a gap, as a whole frame shows them; `None` for a
/// frame the terminal has only partly drawn, whose numbers break the run.
fn furthest(screen: &vt100::Screen, prefix: &str) -> Option<usize> {
    let numbers: Vec<usize> = lines(screen)
        .iter()
        .flat_map(|line| {
            let mut numbers: Vec<usize> = line
                .match_indices(prefix)
                .filter_map(|(at, _)| {
                    let digits: String = line[at + prefix.len()..]
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect();
                    digits.parse().ok()
                })
                .collect();
            // On a line of several labels (the `DataTable`'s header) the
            // last one may be cut short by the widget's right edge (`c3` of
            // `c30`).
            if numbers.len() > 1 {
                numbers.pop();
            }
            numbers
        })
        .collect();
    let in_order = numbers.windows(2).all(|pair| pair[1] == pair[0] + 1);
    in_order.then(|| numbers.last().copied()).flatten()
}

/// Where the frames after `start` in `raw` may end: after each
/// synchronized frame (full-screen mode) and at the end of each read from
/// the terminal (inline mode draws without synchronized output). A read can
/// end inside a frame; [`furthest`] skips such a partly drawn screen.
fn frame_ends(raw: &[u8], read_ends: &[usize], start: usize) -> Vec<usize> {
    let mut ends: Vec<usize> = raw[start..]
        .windows(FRAME_END.len())
        .enumerate()
        .filter(|(_, window)| *window == FRAME_END)
        .map(|(at, _)| start + at + FRAME_END.len())
        .chain(read_ends.iter().copied().filter(|&end| end > start))
        .collect();
    ends.push(raw.len());
    ends.sort_unstable();
    ends.dedup();
    ends
}

/// Clicks the scrollbar track at `at`, waits for the page to settle, and
/// replays what the terminal received after the click frame by frame: some
/// frame must show the content between the old page and the new one, as
/// `furthest(prefix)` measures it.
fn check_click_animates(term: &Term, what: &str, at: (usize, usize), prefix: &str) {
    let old = furthest(&term.screen(), prefix).expect("a numbered line on screen");
    let start = term.raw().len();
    mouse(term, LEFT_BUTTON, at, false);
    mouse(term, LEFT_BUTTON, at, true);
    wait_briefly(term, |s| furthest(s, prefix) != Some(old));
    let new = furthest(&term.settle(), prefix).expect("a numbered line on screen");
    let raw = term.raw();
    let mut replay = vt100::Parser::new(ROWS, COLS, 0);
    replay.process(&raw[..start]);
    let mut seen = Vec::new();
    let mut from = start;
    for end in frame_ends(&raw, &term.read_ends(), start) {
        replay.process(&raw[from..end]);
        from = end;
        seen.extend(furthest(replay.screen(), prefix));
    }
    let (low, high) = (old.min(new), old.max(new));
    assert!(
        seen.iter().any(|&at| at > low && at < high),
        "{what}: the page moved from {prefix}{old} to {prefix}{new} with no frame between; \
         frames showed {seen:?}"
    );
}

/// The column `from_right` cells from the terminal's right edge.
fn from_right(from_right: usize) -> usize {
    usize::from(COLS) - 1 - from_right
}

/// Clicks the scrollbar track of the `PROBE_SCROLL` widget `kind` past its
/// thumb: below it, or above it for a Log or `RichLog`, which start at their
/// end; a `DataTable`'s horizontal scrollbar is the row under its last row.
fn check_widget_track_click(mode: &str, kind: &str) {
    let term = spawn(mode, &[("PROBE_SCROLL", kind)], "keys:0");
    let what = format!("{mode} {kind}");
    let screen = term.screen();
    let row_of = |token| position(&screen, token, &what).0;
    let (at, prefix) = match kind {
        "log" => ((row_of("item 52"), from_right(1)), "item "),
        "rich-log" => ((row_of("item 51"), from_right(1)), "item "),
        "option-list" => ((row_of("item 8"), from_right(3)), "item "),
        "data-table" => ((row_of("item 8") + 1, from_right(10)), "c"),
        _ => ((row_of("item 8"), from_right(1)), "item "),
    };
    check_click_animates(&term, &what, at, prefix);
}

macro_rules! track_click_tests {
    ($($name:ident: $mode:literal, $kind:literal;)*) => {
        $(
            #[test]
            fn $name() {
                check_widget_track_click($mode, $kind);
            }
        )*
    };
}

track_click_tests! {
    scl_002_a_track_click_pages_a_container_with_animation_in_full_screen: "full", "container";
    scl_002_a_track_click_pages_a_container_with_animation_inline: "inline", "container";
    scl_002_a_track_click_pages_a_vertical_scroll_with_animation_in_full_screen: "full", "vertical-scroll";
    scl_002_a_track_click_pages_a_vertical_scroll_with_animation_inline: "inline", "vertical-scroll";
    scl_002_a_track_click_pages_a_log_with_animation_in_full_screen: "full", "log";
    scl_002_a_track_click_pages_a_log_with_animation_inline: "inline", "log";
    scl_002_a_track_click_pages_a_rich_log_with_animation_in_full_screen: "full", "rich-log";
    scl_002_a_track_click_pages_a_rich_log_with_animation_inline: "inline", "rich-log";
    scl_002_a_track_click_pages_an_option_list_with_animation_in_full_screen: "full", "option-list";
    scl_002_a_track_click_pages_an_option_list_with_animation_inline: "inline", "option-list";
    scl_002_a_track_click_pages_a_data_table_with_animation_in_full_screen: "full", "data-table";
    scl_002_a_track_click_pages_a_data_table_with_animation_inline: "inline", "data-table";
}

fn check_app_screen_track_click(mode: &str) {
    let term = spawn(mode, &[("PROBE_LINES", "60")], "line 1");
    let what = format!("{mode} app screen");
    let (row, _) = position(&term.screen(), "line 20", &what);
    check_click_animates(&term, &what, (row, from_right(1)), "line ");
}

#[test]
fn scl_002_a_track_click_pages_the_app_screen_with_animation_in_full_screen() {
    check_app_screen_track_click("full");
}

#[test]
fn scl_002_a_track_click_pages_the_app_screen_with_animation_inline() {
    check_app_screen_track_click("inline");
}

fn check_pushed_screen_track_click(mode: &str) {
    let term = spawn_pushed(mode);
    let what = format!("{mode} pushed screen");
    let (row, _) = position(&term.screen(), "pushed 20", &what);
    check_click_animates(&term, &what, (row, from_right(1)), "pushed ");
}

#[test]
fn scl_002_a_track_click_pages_a_pushed_screen_with_animation_in_full_screen() {
    check_pushed_screen_track_click("full");
}

#[test]
fn scl_002_a_track_click_pages_a_pushed_screen_with_animation_inline() {
    check_pushed_screen_track_click("inline");
}
