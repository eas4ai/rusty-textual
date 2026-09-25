# Roadmap

Current: inline-render-mode

## inline-render-mode

Requirements: INL-001, INL-002, INL-003, INL-004, INL-005, INL-006, INL-007, INL-008, INL-009, INL-010, INL-011, INL-012, INL-013, INL-014, INL-015, INL-016, TRM-001, TRM-002, TRM-003

Delivers: inline render mode for `TextualApp` on Unix terminals, matching
Python Textual's `App.run(inline=True)` and `inline_no_clear=True`: no
alternate screen, the app drawn below the shell content at its inline
height, relative redraws, origin-relative mouse input, and a clean or
no-clear exit. Windows falls back to full-screen. The inline01, inline02
and clock examples run inline. Full-screen behavior is unchanged.

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and the
demo frames pass as on `main`.
