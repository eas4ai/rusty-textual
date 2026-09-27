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

[UPD-003] A query's display change (`App::query_mut(...)` then
`DomQueryMut::set_display`, or `DomQueryMut::set` with a display value)
MUST act as Python's `display` setter, which writes the node's own
`display` rule: a node shown this way appears even where the stylesheet
says `display: none`, and a node hidden this way is hidden whatever the
stylesheet says. This holds in full-screen and in inline mode.
Falsifier: The probe shows a line its stylesheet hides with `display: none`, through `App::query_mut(...)` then `set_display(true)`, and the line does not appear, in full-screen or in inline mode.
Mechanism: query-pty
Rationale: Python's DOMNode.display setter writes styles.display, the node's inline rule, which wins over the stylesheet (dom.py:917-934); here a node was shown only when both the stylesheet and the query's change allowed it.
Status: Agreed 2026-09-26

[UPD-004] In full-screen mode, a change the app makes through a query that
matched no node (`App::query_mut(...)` then any `DomQueryMut` method) MUST
NOT clear and redraw the whole screen.
Falsifier: In full-screen mode, after the app sets `loading` through `DomQueryMut::set`, or asks for a repaint with `DomQueryMut::refresh`, on a query that matches nothing, the terminal receives a screen clear (`CSI 2 J`).
Mechanism: query-pty
Rationale: Python's DOMQuery applies each change to each matched node (query.py), so a query that matched nothing changes nothing; here an empty repaint request was read as a request to clear the screen (request_query_refresh sets clear_on_next_render).
Status: Agreed 2026-09-26

[UPD-005] When the app removes the focused widget (`App::remove`,
`App::remove_node`, or `App::query_mut(...)` then `DomQueryMut::remove`),
focus MUST move as Python's `Screen._reset_focus` moves it: to the nearest
focusable widget before it in the focus chain that is not being removed;
when there is none before it, to the last such widget in the chain; when
none is left, nothing has focus. This holds in full-screen and in inline
mode.
Falsifier: With three buttons in the probe, removing the focused second button does not focus the first, or removing the focused first button does not focus the third, through any of the three paths, in full-screen or in inline mode.
Mechanism: query-pty
Rationale: Python's App._prune calls Screen._reset_focus with the pruned nodes to avoid (app.py:4370-4392, screen.py:1020-1071), which takes the first of reversed(chain[idx+1:] + chain[:idx]) not being removed; here App::remove_node cleared focus and left nothing focused.
Status: Agreed 2026-09-26
