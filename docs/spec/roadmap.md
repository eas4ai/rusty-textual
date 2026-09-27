# Roadmap

Current: wheel-scroll-sensitivity

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

## overflow-paints-over-screen-border

Requirements: INL-004, INL-017, TRM-003

Delivers: in inline mode each screen is laid out at the app's inline
height, as in Python, so an app taller than the terminal keeps its screen
border inside the frame and scrolls its content instead of painting over
the border. The inline height itself (INL-004) and full-screen behavior
(TRM-003) do not change.

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and
rustfmt pass.

## pushed-screens-do-not-scroll

Requirements: SCR-001, INL-004, INL-017, TRM-003

Delivers: a pushed screen, such as the command palette or a modal
dialog, whose content is taller than the screen scrolls like the app's
own screen: the mouse wheel and a scrollbar drag move its content as far
as its last line, in full-screen and in inline mode, as Python's Screen
and ModalScreen do. The inline height (INL-004), the app's own screen
(INL-017) and full-screen output (TRM-003) do not change.

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and
rustfmt pass.

## widget-update-skips-repaint

Requirements: UPD-001, INL-005, TRM-003

Delivers: a widget an app changes from its own code through the App's
widget access (`with_widget_mut` and the query forms built on it) is
redrawn in the next frame even when its size stays the same, as Python
redraws a widget whose content is updated, in full-screen and in inline
mode. Inline frames still use relative cursor moves (INL-005), and
full-screen output does not change (TRM-003).

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and
rustfmt pass.

## frame-keeps-unwritten-cells

Requirements: UPD-001, UPD-002, INL-005, TRM-002, TRM-003

Delivers: every way an app changes a widget from its own code, including
`App::with_widget_taken_as` and a query's `DomQueryMut::update`, redraws
the widget in the next frame (UPD-001), and a frame that repaints part of
the screen leaves the terminal showing that part as the app draws it, even
where an earlier frame drew a change without writing it (UPD-002). Inline
frames still use relative cursor moves (INL-005), and full-screen output
does not change: the terminal goldens (TRM-002) and the Python goldens
(TRM-003) still match.

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and
rustfmt pass.

## query-mut-acts-on-app-tree

Requirements: SCR-001, SCR-002, TRM-003

Delivers: while a screen is pushed, every change an app makes through
`App::query_mut(...)` (class changes, styles, display, visibility, focus,
blur, removal and `set`) acts on the nodes its query matched on that
screen, as Python's `App.query` acts on the active screen (SCR-002).
Pushed screens still scroll (SCR-001), and full-screen apps still match
the Python goldens (TRM-003).

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and
rustfmt pass.

## full-screen-screen-honors-size-rules

Requirements: SCR-001, SCR-002, SCR-003, INL-004, INL-017, TRM-003

Delivers: the app's own screen fills the whole terminal in full-screen
mode and the whole inline frame in inline mode, as Python places every
screen, whatever its own width, height, min/max size and margin rules
say, checked at 100x30 and at 512x144 (SCR-003). Inline height still
follows the screen's height rules (INL-004) and inline content still
scrolls inside the screen's border (INL-017). Pushed screens still scroll
and take query changes (SCR-001, SCR-002), and full-screen apps still
match the Python goldens (TRM-003).

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and
rustfmt pass.

## interactive-parity-waits-on-timing

Requirements: TRM-001, TRM-003, TRM-004, INL-007

Delivers: on Unix, keys that reach the terminal while an app starts reach
the app, as in Python, in full-screen and in inline mode, whether or not
the terminal answers the startup queries (TRM-004). The interactive parity
harness sends its keys only once the app has drawn, so button_focus no
longer fails when the machine is busy (TRM-003). The driver still starts
and exits the terminal as before (TRM-001), and a cursor position report
still never reaches the app as a key (INL-007).

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and
rustfmt pass.

## query-display-cannot-override-css

Requirements: SCR-002, UPD-003, UPD-004, UPD-005, TRM-003

Delivers: queries act as Python's do. A query's display change writes the
node's own display rule, so showing a node overrides a stylesheet's
`display: none`, while a widget's own hiding (a tab's pane, a scrollbar,
the tooltip) stays as it is (UPD-003). A query that matched nothing no
longer clears and redraws the screen, and setting `loading` repaints only
the nodes whose loading state changed (UPD-004). Removing the focused
widget moves focus as Python's `Screen._reset_focus` does (UPD-005). A
query's `results_where` tests the pushed screen's nodes (SCR-002).
Full-screen rendering is unchanged (TRM-003).

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and
rustfmt pass.

## wheel-scroll-sensitivity

Requirements: SCL-001, SCL-002, SCR-001, INL-017, TRM-003

Delivers: the mouse wheel and the scrollbars act as in Python for every
widget that scrolls. A vertical wheel notch scrolls 2 lines at once,
ctrl or shift turns it into 4 columns, and a horizontal notch scrolls 4
columns; a widget that cannot use a notch passes it to its ancestors
(SCL-001). Scrollbar track clicks page with an animation at 50 lines a
second, thumb drags animate over 0.1 seconds, and horizontal notches
animate too (SCL-002). Keyboard scrolling keeps its steps. Pushed
screens (SCR-001) and inline screens (INL-017) still scroll to their
last line, and full-screen apps keep matching the Python goldens
(TRM-003).

Done when: every listed requirement's mechanism is current and passes,
each mechanism is reviewed, and the full test gate, strict clippy, and
rustfmt pass.
