Prefix: TRM

# Full-screen terminal behavior

What the full-screen driver and renderer do today. Adding inline mode must
not change it.

[TRM-001] In full-screen mode the driver MUST switch to the alternate
screen, hide the cursor and turn off line wrap when the app starts, and
reverse all three when it exits.
Falsifier: A full-screen app starts outside the alternate screen, or after it exits the terminal is still on the alternate screen or the cursor is hidden.
Mechanism: inline-pty
Rationale: src/driver/platform/posix.rs:38-46 and 120-125.
Status: Agreed 2026-09-25

[TRM-002] In full-screen mode each frame MUST start from the home position
and place changed cells with absolute cursor moves, and the frame MUST be
the size of the terminal.
Falsifier: The terminal output goldens change, or a full-screen frame contains a relative cursor move.
Mechanism: terminal-golden
Rationale: src/render/mod.rs:431 and 474-477; pinned by tests/terminal_output_golden.rs.
Status: Agreed 2026-09-25

[TRM-003] Full-screen apps MUST keep matching the Python goldens: every
pty_parity case passes and every visual_parity case passes.
Falsifier: A pty_parity or visual_parity case that passes at the start of the commitment fails.
Mechanism: pty-parity
Rationale: tests/pty_parity.rs and tests/visual_parity.rs compare against goldens recorded from Python.
Status: Agreed 2026-09-25
