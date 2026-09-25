Prefix: INL

# Inline render mode

Python Textual runs an app inline with `App.run(inline=True)`: the app draws
below the shell prompt instead of on the alternate screen. The requirements
below state what a user sees; citations point at the Python 8.2.8 source.
"The app" means a `TextualApp` run in inline mode on a Unix terminal,
unless a requirement says otherwise.

[INL-001] A `TextualApp` MUST be runnable in inline mode, and in inline mode
with the no-clear exit, through the public run API. Full-screen mode MUST
stay the default.
Falsifier: No public run function starts an app inline, or an app run without the inline option starts inline.
Mechanism: inline-pty
Rationale: Python app.py:2220-2243 (run and run_async take inline and inline_no_clear, both False by default).
Status: Agreed 2026-09-25

[INL-002] The app MUST NOT switch the terminal to the alternate screen. It
MUST draw on the main screen below the shell content, which stays on screen
above the app.
Falsifier: After the app starts, the terminal is on the alternate screen, or a line printed before the app started is gone from above it.
Mechanism: inline-pty
Rationale: Python linux_inline_driver.py:209-244 never writes CSI ? 1049 h.
Status: Agreed 2026-09-25

[INL-003] Before its first frame the app MUST write its padding lines (one
by default) below the cursor, and the first frame MUST start below them. An
app MUST be able to set the padding to zero or more lines.
Falsifier: With the default padding the first frame starts on the cursor's row, or with padding 0 a blank line appears above the first frame.
Mechanism: inline-pty
Rationale: Python linux_inline_driver.py:215-216 and app.py:482-483 (INLINE_PADDING = 1).
Status: Agreed 2026-09-25

[INL-004] The app MUST occupy its inline height: over every screen in the
stack, the largest of the screen's height rule (auto means the content
height at the full terminal width) plus its vertical padding and border,
held within min-height and max-height; the result MUST NOT exceed the
terminal height. With the default CSS the height is the content height
plus 2 rows.
Falsifier: The inline01 clock (3-row Digits, default CSS) occupies other than 5 rows, or an app taller than the terminal draws past the terminal's last row.
Mechanism: inline-pty
Rationale: Python screen.py:1436-1458 and app.py:1679-1686; docs/how-to/style-inline-apps.md says the clock takes 5 lines.
Status: Agreed 2026-09-25

[INL-005] Each frame MUST be drawn relative to the app's origin, with
relative cursor moves and line breaks. Outside a resize, a frame MUST NOT
contain an absolute cursor position or an erase of the whole display.
Falsifier: An inline frame, not written for a resize, contains CSI row ; col H or CSI 2 J.
Mechanism: inline-pty
Rationale: Python _compositor.py:137-162 (InlineUpdate) and app.py:3844-3861.
Status: Agreed 2026-09-25

[INL-006] When the inline height gets smaller, the next frame MUST erase
every row below its new last row, so no row of the taller frame stays on
screen.
Falsifier: After the app gets shorter, a row that belonged to the taller frame is still on screen below the app.
Mechanism: inline-pty
Rationale: Python screen.py:1193-1216 and _compositor.py:137-162 (clear when the height shrinks: CSI J).
Status: Agreed 2026-09-25

[INL-007] Mouse event coordinates MUST be relative to the app's top-left
cell. The app MUST learn its origin from the cursor position report it
requests after each frame, and a cursor position report MUST NOT reach the
app as a key.
Falsifier: With the app's origin below the terminal's first row, a click on a widget activates another widget or none, or a cursor position report arrives as a key event.
Mechanism: inline-pty
Rationale: Python driver.py:86-95, linux_inline_driver.py:154-163 and _xterm_parser.py:286-294.
Status: Agreed 2026-09-25

[INL-008] On exit the app MUST erase its rows and its padding lines and
leave the cursor on the row where the padding began, so the shell prompt
returns where the app started.
Falsifier: After the app exits, a row of the app is still on screen, or the prompt appears below the row where the app started.
Mechanism: inline-pty
Rationale: Python app.py:3495-3514 and linux_inline_driver.py:309-325 (move up by the padding, then CSI J).
Status: Agreed 2026-09-25

[INL-009] With the no-clear exit and no exit message, the app's last frame
MUST stay on screen after exit and the cursor MUST end on the line below
it. With an exit message the no-clear option has no effect.
Falsifier: With the no-clear exit and no exit message, the last frame is erased or overwritten by the prompt, or with an exit message the frame stays.
Mechanism: inline-pty
Rationale: Python app.py:1286-1288 and 3495-3514.
Status: Agreed 2026-09-25

[INL-010] On exit the terminal MUST be restored as in full-screen mode: the
cursor visible, mouse reporting, bracketed paste and focus reporting off,
the keyboard protocol popped, and raw input off.
Falsifier: After the app exits, the cursor is hidden, mouse reporting or bracketed paste is still on, or typed characters are not echoed.
Mechanism: inline-pty
Rationale: Python linux_inline_driver.py:309-325.
Status: Agreed 2026-09-25

[INL-011] On a terminal resize the app MUST erase the visible display
(CSI 2 J), recompute its inline height and redraw.
Falsifier: After a resize the app keeps its old height, or a row of the frame drawn before the resize is still on screen.
Mechanism: inline-pty
Rationale: Python linux_inline_driver.py:182-207 writes CSI 2 J on SIGWINCH; full-screen mode does not.
Status: Agreed 2026-09-25

[INL-012] Inline frames MUST NOT be wrapped in synchronized output (DEC
private mode 2026), even when the terminal supports it.
Falsifier: An inline frame is preceded by CSI ? 2026 h.
Mechanism: inline-pty
Rationale: Python app.py:4581-4594 turns synchronized output off for inline drivers.
Status: Agreed 2026-09-25

[INL-013] The `:inline` pseudo-class MUST match while the app runs inline
and MUST NOT match in full-screen mode, so the default `Screen:inline` rule
applies only inline.
Falsifier: An inline app does not apply a `Screen:inline` rule, or a full-screen app applies it.
Mechanism: inline-pty
Rationale: Python app.py:543 and screen.py:180-185; the flag exists in src/runtime/mod.rs but is never set.
Status: Agreed 2026-09-25

[INL-014] Suspending an inline app MUST be refused as in Python: the
suspend call reports that suspend is unsupported, and the suspend-process
action does nothing.
Falsifier: Suspending an inline app stops the driver or sends SIGTSTP to the process.
Mechanism: inline-pty
Rationale: Python's inline driver keeps can_suspend False (driver.py:62-65; app.py:4741-4780).
Status: Agreed 2026-09-25

[INL-015] On Windows, a request to run inline MUST run the app in
full-screen mode, as Python does.
Falsifier: On Windows, an app run with the inline option does not use the alternate screen.
Mechanism: inline-pty
Rationale: Python app.py:3334-3343 picks the inline driver only when not on Windows.
Status: Agreed 2026-09-25

[INL-016] The `how-to/inline01`, `how-to/inline02` and `widgets/clock`
examples MUST run inline, as their Python originals do, and inline02 MUST
carry its Python `Screen:inline` rule (no border, height 50vh, green
digits).
Falsifier: One of these examples starts on the alternate screen, or inline02 lacks its Screen:inline rule.
Mechanism: inline-pty
Rationale: Python docs/examples/how-to/inline01.py:31, inline02.py:11-38 and widgets/clock.py:31.
Status: Agreed 2026-09-25

[INL-017] Each screen MUST be laid out at the app's inline height, not at
its own height rule: its border and padding stay inside the frame, and
content taller than the frame scrolls inside the screen.
Falsifier: An inline app whose content is taller than the terminal (the inline probe with 60 body lines in a 30-row terminal, default CSS) does not draw its bottom border on the frame's last row, or draws no vertical scrollbar.
Mechanism: inline-pty
Rationale: Python screen.py:1316-1320 lays the screen out at size.with_height(app._get_inline_height()) and _compositor.py:743-752 places it on that whole region, so the height rule only sets the inline height (INL-004); full-screen apps with a bordered, overflowing Screen already keep their border and scrollbar here.
Status: Agreed 2026-09-25
