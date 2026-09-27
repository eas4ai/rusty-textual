//! What a full-screen app writes to a real terminal: every frame starts
//! from the home position, places cells with absolute cursor moves only, and
//! covers the terminal (TRM-002). `tests/terminal_output_golden.rs` pins the
//! frame encoder; this checks the runtime path that sends frames to the
//! terminal. Run idle, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, COLS, ROWS, SHELL_AROUND, Term, calculator, has_text, painted_rows};

const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";
const HOME: &str = "\x1b[H";

/// A CSI sequence with only digit and `;` parameters: `(params, final byte)`.
fn plain_csi(text: &str) -> Vec<(&str, char)> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("\x1b[") {
        let body = &rest[start + 2..];
        let params = body
            .find(|c: char| !(c.is_ascii_digit() || c == ';'))
            .unwrap_or(body.len());
        if let Some(final_byte) = body[params..].chars().next() {
            found.push((&body[..params], final_byte));
        }
        rest = body;
    }
    found
}

#[test]
fn trm_002_full_screen_frames_start_home_and_move_absolutely() {
    let term = Term::spawn(SHELL_AROUND, &calculator(), &[], Answers::TERMINAL);
    term.wait_for("the calculator to draw", |s| {
        s.alternate_screen() && !painted_rows(s).is_empty()
    });
    term.settle();
    term.send(b"12"); // a partial frame: the display changes
    term.settle();
    term.send(b"\x11"); // ctrl+q
    term.wait_for("the app to exit", has_text("after-exit"));
    let raw = term.raw();
    term.finish();
    let text = String::from_utf8_lossy(&raw);

    let start = text
        .find("\x1b[?1049h")
        .expect("entered the alternate screen");
    let end = text
        .rfind("\x1b[?1049l")
        .expect("left the alternate screen");
    let session = &text[start..end];

    // The terminal reports synchronized output, so each frame is wrapped.
    let frames: Vec<&str> = session
        .split(SYNC_BEGIN)
        .skip(1)
        .map(|part| part.split(SYNC_END).next().unwrap_or(part))
        .collect();
    assert!(
        frames.len() >= 2,
        "expected the first frame and an update, got {} synchronized frames",
        frames.len()
    );
    for (n, frame) in frames.iter().enumerate() {
        assert!(
            frame.starts_with(HOME),
            "frame {n} does not start from home: {:?}",
            frame.chars().take(40).collect::<String>()
        );
    }

    for (params, final_byte) in plain_csi(session) {
        assert!(
            !('A'..='F').contains(&final_byte),
            "relative cursor move CSI {params}{final_byte} in full-screen output"
        );
        if final_byte == 'H' && !params.is_empty() {
            let mut parts = params.split(';').map(|p| p.parse::<u16>().unwrap_or(1));
            let (row, col) = (parts.next().unwrap_or(1), parts.next().unwrap_or(1));
            assert!(
                (1..=ROWS).contains(&row) && (1..=COLS).contains(&col),
                "cursor move to {row};{col} outside the {COLS}x{ROWS} terminal"
            );
        }
    }

    let first = frames[0];
    for row in 1..=ROWS {
        assert!(
            first.contains(&format!("\x1b[{row};1H")),
            "the first frame never draws row {row}: it is not the terminal's size"
        );
    }
}
