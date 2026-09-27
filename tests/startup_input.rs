//! PTY checks for keys typed while the app starts (`docs/spec/terminal.md`).
//!
//! This file is the `startup-pty` Sudus mechanism, run through
//! `scripts/startup_mechanism.py`, which maps each `trm_NNN_` test to its
//! requirement. Each test starts the probe app of `tests/fixtures/inline_probe`
//! in the pseudo terminal of `tests/support/pty.rs` and at once sends `e`, `é`
//! and F5: a key of one byte, a key of two UTF-8 bytes and a key sent as an
//! escape sequence. The probe binds none of them, so it counts each one on its
//! status line (`keys:N`). They reach the terminal while the app starts,
//! before or during its startup terminal queries, which the terminal answers
//! or leaves unanswered. Each case has its own test. Run idle and
//! single-threaded, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, SHELL_THEN_EXEC, Term, dump, has_text, probe};

/// `e`, `é` and F5 (`CSI 15 ~`).
const KEYS: &str = "eé\x1b[15~";

/// A terminal that answers cursor position reports, which inline mode
/// needs, but not the startup mode and device attributes queries.
const SILENT: Answers = Answers {
    modes: false,
    device_attributes: false,
    ..Answers::TERMINAL
};

/// The status line's key count, followed by the rest of the line, so
/// `keys:30` does not match `keys:3`.
const THREE_KEYS: &str = "keys:3 ";

/// Starts the probe, sends the keys at once, and waits for the probe to count
/// exactly three once the screen has settled: no key lost, and nothing else
/// the startup exchange read counted as a key.
fn check_keys_typed_at_launch_arrive(mode: &str, answers: Answers) {
    let env = [("PROBE_MODE", mode)];
    let term = Term::spawn(SHELL_THEN_EXEC, &probe(), &env, answers);
    term.send(KEYS.as_bytes());
    term.wait_for(
        &format!("{mode}: keys:3 on the status line"),
        has_text(THREE_KEYS),
    );
    let screen = term.settle();
    assert!(
        has_text(THREE_KEYS)(&screen),
        "{mode}: the status line moved past keys:3:\n{}",
        dump(&screen)
    );
}

#[test]
fn trm_004_keys_typed_at_launch_reach_the_app_in_full_screen() {
    check_keys_typed_at_launch_arrive("full", Answers::TERMINAL);
}

#[test]
fn trm_004_keys_typed_at_launch_reach_the_app_in_full_screen_when_the_queries_go_unanswered() {
    check_keys_typed_at_launch_arrive("full", SILENT);
}

#[test]
fn trm_004_keys_typed_at_launch_reach_the_app_inline() {
    check_keys_typed_at_launch_arrive("inline", Answers::TERMINAL);
}

#[test]
fn trm_004_keys_typed_at_launch_reach_the_app_inline_when_the_queries_go_unanswered() {
    check_keys_typed_at_launch_arrive("inline", SILENT);
}
