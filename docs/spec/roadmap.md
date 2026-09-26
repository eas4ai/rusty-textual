# Roadmap

Current: frame-keeps-unwritten-cells

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
