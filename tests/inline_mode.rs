//! PTY checks for inline render mode (`docs/spec/inline.md`) and the
//! full-screen terminal lifecycle (`docs/spec/terminal.md`, TRM-001).
//!
//! This file is the `inline-pty` Sudus mechanism, run through
//! `scripts/inline_mechanism.py`, which maps each `inl_NNN_` or `trm_NNN_`
//! test to its requirement. Each test runs a real binary in the pseudo
//! terminal of `tests/support/pty.rs`, which answers what a modern terminal
//! answers: cursor position reports, the synchronized-output and
//! in-band-resize mode queries, and primary device attributes.
//!
//! Binaries: the probe app in `tests/fixtures/inline_probe` (built here), the
//! docs examples inline01, inline02 and clock, and the calculator example.
//! A binary that does not build fails the tests that need it. Run idle and
//! single-threaded, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use std::path::PathBuf;
use std::sync::OnceLock;

use pty::Answers;
use pty::{
    ROWS, SHELL_AROUND, SHELL_THEN_EXEC, Term, built, calculator, contains, dump, has_text, lines,
    painted_rows, repo_root, row_of,
};

fn probe() -> PathBuf {
    static CELL: OnceLock<pty::Built> = OnceLock::new();
    let target_dir = repo_root().join("target/inline-probe");
    let target_arg = target_dir.display().to_string();
    let manifest = "tests/fixtures/inline_probe/Cargo.toml";
    built(
        &CELL,
        &[
            "build",
            "--manifest-path",
            manifest,
            "--target-dir",
            &target_arg,
        ],
        "inline_probe",
    )
}

fn docs_example(name: &str) -> PathBuf {
    static HOW_TO: OnceLock<pty::Built> = OnceLock::new();
    static WIDGETS: OnceLock<pty::Built> = OnceLock::new();
    let manifest = "docs/examples/Cargo.toml";
    if name == "clock" {
        let args = [
            "build",
            "--manifest-path",
            manifest,
            "-p",
            "textual-docs-widgets",
            "--example",
            "clock",
        ];
        built(&WIDGETS, &args, name)
    } else {
        let args = [
            "build",
            "--manifest-path",
            manifest,
            "-p",
            "textual-docs-how-to",
            "--example",
            "inline01",
            "--example",
            "inline02",
        ];
        built(&HOW_TO, &args, name)
    }
}

/// The painted rows form one block of `height` rows starting at `top`.
fn assert_block(screen: &vt100::Screen, top: u16, height: u16, what: &str) {
    let expected: Vec<u16> = (top..top + height).collect();
    assert_eq!(
        painted_rows(screen),
        expected,
        "{what}; screen:\n{}",
        dump(screen)
    );
}

/// Absolute cursor positions (`CSI row ; col H`, `CSI H`) and whole-display
/// erases (`CSI 2 J`) in `bytes`, with their offsets.
fn absolute_moves(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let pattern = regex::Regex::new(r"\x1b\[(\d*(;\d*)?H|2J)").expect("regex");
    pattern
        .find_iter(&text)
        .map(|m| format!("{:?} at {}", m.as_str(), m.start()))
        .collect()
}

const PROBE_ROWS_PER_BODY: u16 = 4; // status + inline-css marker + top and bottom border

#[test]
fn inl_001_inline_option_starts_inline_and_default_stays_full_screen() {
    let bin = probe();
    let inline = Term::spawn(SHELL_THEN_EXEC, &bin, &[], Answers::TERMINAL);
    let screen = inline.wait_for("probe body", has_text("line 1"));
    assert!(
        !screen.alternate_screen(),
        "inline probe is on the alternate screen"
    );
    drop(inline);

    let full = Term::spawn(
        SHELL_THEN_EXEC,
        &bin,
        &[("PROBE_MODE", "full")],
        Answers::TERMINAL,
    );
    let screen = full.wait_for("probe body", has_text("line 1"));
    assert!(
        screen.alternate_screen(),
        "default run is not on the alternate screen"
    );
}

#[test]
fn inl_002_draws_on_main_screen_below_shell_content() {
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), &[], Answers::TERMINAL);
    term.wait_for("probe body", has_text("line 1"));
    let screen = term.settle();
    assert!(!screen.alternate_screen());
    let text = lines(&screen);
    assert_eq!(
        text[0],
        "shell-1",
        "shell content moved; screen:\n{}",
        dump(&screen)
    );
    assert_eq!(
        text[1],
        "shell-2",
        "shell content moved; screen:\n{}",
        dump(&screen)
    );
    assert!(row_of(&screen, "line 1").is_some_and(|row| row > 1));
}

#[test]
fn inl_003_padding_lines_come_before_the_first_frame() {
    // inline01 keeps the framework default (one padding line).
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &docs_example("inline01"),
        &[],
        Answers::TERMINAL,
    );
    let screen = term.wait_for("clock", |s| !painted_rows(s).is_empty());
    assert_eq!(
        painted_rows(&screen).first(),
        Some(&3),
        "default padding; screen:\n{}",
        dump(&screen)
    );
    drop(term);

    for (padding, top) in [("0", 2u16), ("2", 4)] {
        let term = Term::spawn(
            SHELL_THEN_EXEC,
            &probe(),
            &[("PROBE_PADDING", padding)],
            Answers::TERMINAL,
        );
        term.wait_for("probe body", has_text("line 1"));
        let screen = term.settle();
        assert_eq!(
            painted_rows(&screen).first(),
            Some(&top),
            "padding {padding}; screen:\n{}",
            dump(&screen)
        );
    }
}

#[test]
fn inl_004_height_is_the_screen_auto_height_capped_at_the_terminal() {
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &docs_example("inline01"),
        &[],
        Answers::TERMINAL,
    );
    term.wait_for("clock", |s| !painted_rows(s).is_empty());
    let screen = term.settle();
    assert_block(
        &screen,
        3,
        5,
        "inline01 takes 5 rows below the padding line",
    );
    drop(term);

    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &probe(),
        &[("PROBE_LINES", "3")],
        Answers::TERMINAL,
    );
    term.wait_for("probe body", has_text("line 3"));
    let screen = term.settle();
    assert_block(&screen, 3, 3 + PROBE_ROWS_PER_BODY, "3 body lines");
    drop(term);

    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &probe(),
        &[("PROBE_LINES", "60")],
        Answers::TERMINAL,
    );
    // The status line is below the visible rows by design: wait for the body.
    term.wait_for("probe body", has_text("line 20"));
    let screen = term.settle();
    assert_block(
        &screen,
        0,
        ROWS,
        "a taller app is capped at the terminal height",
    );
}

#[test]
fn inl_004_command_palette_holds_its_inline_min_height() {
    // Python's `CommandPalette:inline { min-height: 20; }`: while the palette
    // is open, the inline app is 20 rows tall even if its content is shorter.
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &docs_example("inline01"),
        &[],
        Answers::TERMINAL,
    );
    term.wait_for("clock", |s| !painted_rows(s).is_empty());
    term.settle();
    term.send(b"\x10"); // ctrl+p
    term.wait_for("the palette", |s| painted_rows(s).len() > 5);
    let screen = term.settle();
    assert_block(
        &screen,
        3,
        20,
        "the open command palette holds the inline app at 20 rows",
    );
}

#[test]
fn inl_005_frames_use_relative_cursor_moves_only() {
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &probe(),
        &[("PROBE_LINES", "4")],
        Answers::TERMINAL,
    );
    term.wait_for("probe body", has_text("line 4"));
    term.send(b"s");
    term.wait_for("shrunk body", |s| row_of(s, "line 4").is_none());
    term.settle();
    let found = absolute_moves(&term.raw());
    assert!(
        found.is_empty(),
        "absolute positioning in inline output: {found:?}"
    );
}

#[test]
fn inl_006_shrinking_erases_rows_below_the_frame() {
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &probe(),
        &[("PROBE_LINES", "6")],
        Answers::TERMINAL,
    );
    term.wait_for("probe body", has_text("line 6"));
    assert_block(&term.settle(), 3, 6 + PROBE_ROWS_PER_BODY, "6 body lines");
    term.send(b"s");
    term.wait_for("shrunk body", |s| row_of(s, "line 6").is_none());
    let screen = term.settle();
    assert_block(
        &screen,
        3,
        1 + PROBE_ROWS_PER_BODY,
        "1 body line after shrinking",
    );
    let below = &lines(&screen)[usize::from(3 + 1 + PROBE_ROWS_PER_BODY)..];
    assert!(
        below.iter().all(String::is_empty),
        "stale rows below the app:\n{}",
        dump(&screen)
    );
}

#[test]
fn inl_007_mouse_is_relative_to_the_origin_and_reports_are_not_keys() {
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &probe(),
        &[("PROBE_BUTTON", "1")],
        Answers::TERMINAL,
    );
    term.wait_for("button", has_text("Press"));
    let screen = term.settle();
    let row = row_of(&screen, "Press").expect("button row");
    let col = lines(&screen)[row].find("Press").expect("button column");
    assert!(row > 2, "the app should start below the shell lines");
    let (x, y) = (col + 2, row + 1); // 1-based SGR coordinates inside the label
    term.send(format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes());
    let screen = term.wait_for("click to register", has_text("clicked"));
    let status = &lines(&screen)[row_of(&screen, "keys:").expect("status")];
    assert!(
        status.contains("keys:0"),
        "cursor position reports reached the app as keys: {status}"
    );
}

#[test]
fn inl_007_a_silent_terminal_is_asked_twice_then_left_alone() {
    // Each unanswered query blocks for crossterm's 2 s timeout. The app asks,
    // retries once (a late reply would be queued by then), and then stops, so
    // a terminal that never answers does not freeze the app again and again.
    let answers = Answers {
        cursor_position: false,
        ..Answers::TERMINAL
    };
    let term = Term::spawn(SHELL_THEN_EXEC, &docs_example("inline01"), &[], answers);
    term.wait_for("clock", |s| !painted_rows(s).is_empty());
    // The clock redraws every second, and each frame may ask again. A
    // doubling backoff would ask a third time about 7 s in.
    std::thread::sleep(std::time::Duration::from_secs(10));
    let queries = term.raw().windows(4).filter(|w| w == b"\x1b[6n").count();
    assert_eq!(queries, 2, "cursor position queries in the first 10 s");
}

#[test]
fn inl_007_mouse_recovers_after_a_slow_cursor_report() {
    // The first report comes after crossterm's 2 s timeout; it stays queued
    // and answers the next query, so the app learns its origin anyway.
    let answers = Answers {
        cursor_delay: std::time::Duration::from_millis(2500),
        ..Answers::TERMINAL
    };
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), &[("PROBE_BUTTON", "1")], answers);
    term.wait_for("button", has_text("Press"));
    // Past the late reply and the retry wait, draw another frame.
    // A resize always redraws (a key would go to the focused button).
    std::thread::sleep(std::time::Duration::from_secs(4));
    term.resize(ROWS - 1);
    std::thread::sleep(std::time::Duration::from_secs(1));
    let screen = term.settle();
    let row = row_of(&screen, "Press").expect("button row");
    let col = lines(&screen)[row].find("Press").expect("button column");
    let (x, y) = (col + 2, row + 1);
    term.send(format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes());
    term.wait_for("click to register", has_text("clicked"));
}

#[test]
fn inl_008_a_run_that_stops_before_starting_leaves_the_shell_alone() {
    // `configure` calls `App::exit`: nothing was drawn, so nothing is erased.
    let term = Term::spawn(
        SHELL_AROUND,
        &probe(),
        &[("PROBE_EXIT_IN_CONFIGURE", "1")],
        Answers::TERMINAL,
    );
    let screen = term.finish();
    let text = lines(&screen);
    assert_eq!(
        text[0],
        "shell-1",
        "shell output erased:\n{}",
        dump(&screen)
    );
    assert_eq!(
        text[1],
        "shell-2",
        "shell output erased:\n{}",
        dump(&screen)
    );
    assert!(
        row_of(&screen, "after-exit").is_some_and(|row| row >= 2),
        "{}",
        dump(&screen)
    );
}

#[test]
fn inl_008_exit_erases_the_app_and_padding() {
    let term = Term::spawn(SHELL_AROUND, &probe(), &[], Answers::TERMINAL);
    term.wait_for("probe body", has_text("line 1"));
    term.settle();
    term.send(b"q");
    let screen = term.finish();
    let text = lines(&screen);
    assert_eq!(
        text[2],
        "after-exit",
        "prompt did not return to the padding row:\n{}",
        dump(&screen)
    );
    assert!(
        row_of(&screen, "line 1").is_none(),
        "app rows left on screen:\n{}",
        dump(&screen)
    );
    assert!(
        painted_rows(&screen).is_empty(),
        "painted rows left:\n{}",
        dump(&screen)
    );
}

#[test]
fn inl_009_no_clear_exit_keeps_the_last_frame() {
    let term = Term::spawn(
        SHELL_AROUND,
        &probe(),
        &[("PROBE_MODE", "inline-no-clear")],
        Answers::TERMINAL,
    );
    term.wait_for("probe body", has_text("line 1"));
    term.settle();
    term.send(b"q");
    let screen = term.finish();
    let body = row_of(&screen, "line 1").expect("last frame kept");
    let after = row_of(&screen, "after-exit").expect("prompt marker");
    let last_painted = painted_rows(&screen)
        .last()
        .copied()
        .expect("painted rows kept");
    assert!(
        after > body,
        "prompt overwrote the frame:\n{}",
        dump(&screen)
    );
    assert_eq!(
        after,
        usize::from(last_painted) + 1,
        "prompt not on the line below:\n{}",
        dump(&screen)
    );
}

#[test]
fn inl_009_an_exit_message_clears_even_with_no_clear() {
    // Python `App.exit(message=...)` drops the no-clear frame.
    let env = [
        ("PROBE_MODE", "inline-no-clear"),
        ("PROBE_EXIT_MESSAGE", "bye-message"),
    ];
    let term = Term::spawn(SHELL_AROUND, &probe(), &env, Answers::TERMINAL);
    term.wait_for("probe body", has_text("line 1"));
    term.settle();
    term.send(b"q");
    let screen = term.finish();
    assert!(
        row_of(&screen, "bye-message").is_some(),
        "exit message missing:\n{}",
        dump(&screen)
    );
    assert!(
        row_of(&screen, "line 1").is_none(),
        "frame kept despite an exit message:\n{}",
        dump(&screen)
    );
}

#[test]
fn inl_009_a_return_value_keeps_the_no_clear_frame() {
    // Python `App.exit(result=...)` is not a message: the frame stays.
    let env = [
        ("PROBE_MODE", "inline-no-clear"),
        ("PROBE_EXIT_RESULT", "the-result"),
    ];
    let term = Term::spawn(SHELL_AROUND, &probe(), &env, Answers::TERMINAL);
    term.wait_for("probe body", has_text("line 1"));
    term.settle();
    term.send(b"q");
    let screen = term.finish();
    let body = row_of(&screen, "line 1").expect("frame kept with a return value");
    let result = row_of(&screen, "the-result").expect("main prints the return value");
    assert!(
        result > body,
        "return value printed over the frame:\n{}",
        dump(&screen)
    );
}

#[test]
fn inl_010_exit_restores_the_terminal() {
    let term = Term::spawn(SHELL_AROUND, &probe(), &[], Answers::TERMINAL);
    term.wait_for("probe body", has_text("line 1"));
    term.settle();
    term.send(b"q");
    let screen = term.finish();
    assert!(!screen.hide_cursor(), "cursor still hidden");
    assert_eq!(
        screen.mouse_protocol_mode(),
        vt100::MouseProtocolMode::None,
        "mouse still on"
    );
    assert!(!screen.bracketed_paste(), "bracketed paste still on");
    let flags = lines(&screen)
        .into_iter()
        .find(|line| line.contains("icanon"))
        .expect("stty flags line");
    let words: Vec<&str> = flags.split_whitespace().collect();
    assert!(
        words.contains(&"icanon") && words.contains(&"echo"),
        "raw input left on: {flags}"
    );
}

#[test]
fn inl_011_resize_erases_and_recomputes_the_height() {
    // inline02: `height: 50vh`, no border.
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &docs_example("inline02"),
        &[],
        Answers::TERMINAL,
    );
    term.wait_for("clock", |s| painted_rows(s).len() == 15);
    term.resize(20);
    term.wait_for("height after resize", |s| painted_rows(s).len() == 10);
    let screen = term.settle();
    assert_eq!(
        painted_rows(&screen).len(),
        10,
        "height after resize; screen:\n{}",
        dump(&screen)
    );
    assert!(
        row_of(&screen, "shell-1").is_none(),
        "display was not erased on resize:\n{}",
        dump(&screen)
    );
}

#[test]
fn inl_012_inline_frames_skip_synchronized_output() {
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), &[], Answers::TERMINAL);
    term.wait_for("probe body", has_text("line 1"));
    term.settle();
    assert!(
        !contains(&term.raw(), b"\x1b[?2026h"),
        "inline frame wrapped in synchronized output"
    );
    drop(term);

    // Control: full-screen frames use it, so the harness did advertise support.
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &probe(),
        &[("PROBE_MODE", "full")],
        Answers::TERMINAL,
    );
    term.wait_for("probe body", has_text("line 1"));
    term.settle();
    assert!(
        contains(&term.raw(), b"\x1b[?2026h"),
        "full-screen control did not use synchronized output"
    );
}

#[test]
fn inl_013_inline_pseudo_class_matches_only_inline() {
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), &[], Answers::TERMINAL);
    term.wait_for("probe body", has_text("line 1"));
    assert!(
        row_of(&term.settle(), "inline-css").is_some(),
        "Screen:inline rule not applied inline"
    );
    drop(term);

    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &probe(),
        &[("PROBE_MODE", "full")],
        Answers::TERMINAL,
    );
    term.wait_for("probe body", has_text("line 1"));
    assert!(
        row_of(&term.settle(), "inline-css").is_none(),
        "Screen:inline rule applied full-screen"
    );
}

fn process_state(pid: u32) -> char {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("read /proc stat");
    let after_name = stat.rsplit_once(')').expect("stat format").1;
    after_name.trim_start().chars().next().expect("state")
}

#[test]
fn inl_014_suspend_is_refused_inline() {
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &probe(),
        &[("PROBE_LINES", "2")],
        Answers::TERMINAL,
    );
    term.wait_for("probe body", has_text("line 2"));
    term.send(b"z");
    term.wait_for("suspend result", has_text("suspend:refused"));
    term.send(b"x");
    term.wait_for("suspend-process result", has_text("suspend:process"));
    assert_ne!(
        process_state(term.pid()),
        'T',
        "the suspend-process action stopped the app"
    );
    term.send(b"s");
    term.wait_for("app still responds", |s| row_of(s, "line 2").is_none());
}

#[test]
fn inl_016_docs_examples_run_inline() {
    for name in ["inline01", "inline02", "clock"] {
        let term = Term::spawn(SHELL_THEN_EXEC, &docs_example(name), &[], Answers::TERMINAL);
        let screen = term.wait_for(name, |s| !painted_rows(s).is_empty());
        assert!(
            !screen.alternate_screen(),
            "{name} runs on the alternate screen"
        );
    }
}

#[test]
fn inl_016_inline02_renders_its_inline_rule() {
    // Python inline02: `&:inline { border: none; height: 50vh; Digits { color: $success } }`.
    let term = Term::spawn(
        SHELL_THEN_EXEC,
        &docs_example("inline02"),
        &[],
        Answers::TERMINAL,
    );
    term.wait_for("inline02", |s| {
        painted_rows(s).len() == usize::from(ROWS / 2)
    });
    let screen = term.settle();
    assert_eq!(
        painted_rows(&screen).len(),
        usize::from(ROWS / 2),
        "50vh; screen:\n{}",
        dump(&screen)
    );
    let text = lines(&screen);
    assert!(
        !text
            .iter()
            .any(|line| line.contains('\u{2594}') || line.contains('\u{2581}')),
        "a border row is drawn:\n{}",
        dump(&screen)
    );
    // textual-dark `$success` is #4EBF71.
    let success = vt100::Color::Rgb(0x4e, 0xbf, 0x71);
    let (rows, cols) = screen.size();
    let green = (0..rows).any(|row| {
        (0..cols).any(|col| {
            screen
                .cell(row, col)
                .is_some_and(|cell| cell.contents().trim() != "" && cell.fgcolor() == success)
        })
    });
    assert!(
        green,
        "no digit drawn in the success color:\n{}",
        dump(&screen)
    );
}

#[test]
fn trm_001_full_screen_uses_and_restores_the_alternate_screen() {
    let term = Term::spawn(SHELL_AROUND, &calculator(), &[], Answers::TERMINAL);
    let screen = term.wait_for("calculator", |s| {
        s.alternate_screen() && !painted_rows(s).is_empty()
    });
    assert!(screen.hide_cursor(), "cursor visible while running");
    term.settle();
    term.send(b"\x11"); // ctrl+q
    let screen = term.finish();
    assert!(
        !screen.alternate_screen(),
        "still on the alternate screen after exit"
    );
    assert!(!screen.hide_cursor(), "cursor hidden after exit");
    assert!(
        row_of(&screen, "shell-1").is_some(),
        "shell content not restored:\n{}",
        dump(&screen)
    );
    assert!(
        row_of(&screen, "after-exit").is_some(),
        "shell did not continue:\n{}",
        dump(&screen)
    );
}
