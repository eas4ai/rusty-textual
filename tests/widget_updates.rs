//! PTY checks for widget updates (`docs/spec/updates.md`).
//!
//! This file is the `update-pty` Sudus mechanism, run through
//! `scripts/update_mechanism.py`, which maps each `upd_NNN_` test to its
//! requirement. Each test runs the probe app of `tests/fixtures/inline_probe`
//! in the pseudo terminal of `tests/support/pty.rs`. The probe's key handler
//! counts every key it does not bind and writes the count into its status
//! line (`keys:N`) with `App::with_query_one_mut_as`, which keeps the line's
//! size. Every case runs in full-screen and in inline mode, with and without
//! a focused button. Run idle and single-threaded, like the other PTY tests.

#[path = "support/pty.rs"]
mod pty;

use pty::{Answers, SHELL_THEN_EXEC, Term, has_text, probe};

/// Each render mode, with and without a focused `Press` button.
const CASES: [&[(&str, &str)]; 4] = [
    &[("PROBE_MODE", "full")],
    &[("PROBE_MODE", "full"), ("PROBE_BUTTON", "1")],
    &[("PROBE_MODE", "inline")],
    &[("PROBE_MODE", "inline"), ("PROBE_BUTTON", "1")],
];

#[test]
fn upd_001_a_status_line_updated_from_a_key_handler_is_redrawn() {
    for env in CASES {
        let term = Term::spawn(SHELL_THEN_EXEC, &probe(), env, Answers::TERMINAL);
        term.wait_for("probe status", has_text("keys:0"));
        term.settle();
        term.send(b"e"); // a key the probe does not bind
        term.wait_for(
            &format!("{env:?}: keys:1 after one unbound key"),
            has_text("keys:1"),
        );
    }
}
