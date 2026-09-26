Prefix: UPD

# Widget updates

How an app changes its widgets while it runs: the text of a label, a
counter or a status line, set from the app's own code. Python Textual
redraws a widget whenever its content is updated; the requirements below
state what a user sees, with citations into the Python 8.2.8 source.

[UPD-001] When an app changes a widget from its own code through the App's
widget access (`App::with_widget_mut`, `with_widget_mut_as`,
`with_query_one_mut` or `with_query_one_mut_as`), the next frame MUST show
the change, even when the widget keeps its size. This holds in full-screen
and in inline mode.
Falsifier: The inline probe, whose key handler updates its status line from keys:0 to keys:1 through `with_query_one_mut_as` when an unbound key is pressed, still shows keys:0 after that key, in full-screen or in inline mode.
Mechanism: update-pty
Rationale: Python's Static.update always calls self.refresh (_static.py:85-95). Here App::with_widget_mut requests a relayout only when the widget's size changes and never a repaint, while Handle::update and DomQueryMut::update request one.
Status: Agreed 2026-09-26
