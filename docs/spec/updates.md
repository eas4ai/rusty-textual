Prefix: UPD

# Widget updates

How an app changes its widgets while it runs: the text of a label, a
counter or a status line, set from the app's own code. Python Textual
redraws a widget whenever its content is updated; the requirements below
state what a user sees, with citations into the Python 8.2.8 source.

[UPD-001] When an app changes a widget from its own code through the App's
widget access (`App::with_widget_mut`, `with_widget_mut_as`,
`with_query_one_mut`, `with_query_one_mut_as` or `with_widget_taken_as`) or
through a query (`App::query_mut(...)` then `DomQueryMut::update`), the next
frame MUST show the change, even when the widget keeps its size. This holds
in full-screen and in inline mode.
Falsifier: The inline probe updates its status line through one of these paths, from a key handler or from a message handler while a hover repaints another widget, and the status line still shows the old text afterwards, in full-screen or in inline mode.
Mechanism: update-pty
Rationale: Python's Static.update always calls self.refresh (_static.py:85-95). Here these paths change the widget in a closure that has no ctx to ask for a repaint, so the path itself must ask, as Handle::update does.
Status: Agreed 2026-09-26

[UPD-002] When a frame repaints part of the screen, the terminal MUST then
show that part as the app draws it in that frame, even where an earlier
frame drew a change it did not write. This holds in full-screen and in
inline mode.
Falsifier: In full-screen mode, the probe's shared-count line changes without a repaint request, a frame for a hover elsewhere is drawn while it is changed, then the app repaints the line with `DomQueryMut::refresh`, and the line still shows the old count.
Mechanism: update-pty
Rationale: Python writes every repainted region from the current render and never compares it with an earlier frame (_compositor.py:166-230 and 1096-1187); here a region-scoped full-screen frame stored every composed cell as written, including cells outside its regions.
Status: Agreed 2026-09-26
