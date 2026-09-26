# Glossary

**Full-screen mode.** The default way an app runs: it switches the terminal
to the alternate screen and draws over the whole of it.

**Inline mode.** Running an app below the shell prompt, in the terminal's
main screen, without the alternate screen. Python: `App.run(inline=True)`.

**Alternate screen.** The terminal's second screen buffer (DEC private mode
1049). Leaving it brings back the shell text that was on screen before.

**Shell content.** The text on the terminal's main screen before an inline
app starts, such as earlier commands and their output.

**Padding line.** The blank line an inline app writes below the cursor
before its first frame. Python: `App.INLINE_PADDING`, 1 by default.

**Origin.** The terminal cell where an inline app's top-left corner is
drawn. The runtime learns it from cursor position reports.

**Cursor position report.** The terminal's reply `CSI row ; col R` to the
query `CSI 6 n`.

**Inline height.** The number of terminal rows an inline app occupies.

**Screen stack.** The screens an app has pushed; the top one is visible.

**Pushed screen.** A screen on top of the app's own screen, pushed with
`App::push_screen` (Python `App.push_screen`), such as the command palette
or a modal dialog.

**Frame.** One complete rendering of the app, written to the terminal.

**Driver.** The code that puts the terminal into and out of application
mode and reads its size and input (`src/driver/`).

**No-clear exit.** Python's `inline_no_clear=True`: on exit an inline app
leaves its last frame on screen.
