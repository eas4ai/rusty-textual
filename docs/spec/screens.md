Prefix: SCR

# Screens

The screens an app pushes on top of its own screen, such as the command
palette, a modal dialog or a help screen. Python Textual lays out and
scrolls every screen the same way; the requirements below state what a
user sees, with citations into the Python 8.2.8 source.

[SCR-001] A pushed screen whose content is taller than the screen MUST
scroll as the app's own screen does: turning the mouse wheel over it, or
dragging its vertical scrollbar, moves its content, and it scrolls as far
as its last line. This holds in full-screen and in inline mode.
Falsifier: A pushed screen with 60 lines of content in a 30-row terminal does not show its last line after the mouse wheel is turned down over it or its scrollbar thumb is dragged to the bottom, in full-screen or in inline mode.
Mechanism: screen-pty
Rationale: Python Screen and ModalScreen set overflow-y: auto (screen.py:174-188, 2164-2172), and Widget._on_mouse_scroll_down scrolls any widget that allows vertical scroll (widget.py:4777-4795); Screen binds no scroll keys (screen.py:269-273). Here the pushed screen's root (ScreenHost) has no scroll state.
Status: Agreed 2026-09-25

[SCR-002] While a screen is pushed, a change the app makes through a
query, `App::query_mut(...)` then any `DomQueryMut` method, MUST act on the
nodes the query matched on that screen. This holds in full-screen and in
inline mode.
Falsifier: With the probe's 60-line screen pushed, the app hides that screen's text through `App::query_mut(...)` then `set_display(false)`, and the pushed screen still shows `pushed 1`, in full-screen or in inline mode.
Mechanism: screen-pty
Rationale: Python's App has one child, the active screen (app.py:992-1005), so App.query and every change made through its result act on that screen's nodes; here the query matched in the active screen's tree, but most DomQueryMut methods looked the ids up in the app's own tree.
Status: Agreed 2026-09-26
