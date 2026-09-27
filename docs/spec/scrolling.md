Prefix: SCL

# Scrolling

How the mouse wheel and the scrollbars move what a scrolling widget
shows: the app's own screen, a pushed screen, a container, and the
widgets that scroll their own content. Python Textual handles the wheel
the same way for every widget and animates the scrolls its scrollbars
ask for; the requirements below state what a user sees, with citations
into the Python 8.2.8 source.

[SCL-001] One notch of the mouse wheel MUST scroll the widget under the
pointer, or its nearest ancestor that can scroll that way, as Python
does: a vertical notch by 2 lines at once, or by 4 columns
horizontally when ctrl or shift is held, and a horizontal notch by 4
columns. A widget that cannot scroll that way, or is already at its
end, MUST leave the notch to its ancestors. This holds for the app's screen, pushed
screens, containers and the widgets that scroll their own content (the
ScrollView family, Log, RichLog, OptionList, SelectionList, ListView,
Tree, DataTable and KeyPanel), in full-screen and in inline mode.
Keyboard scrolling keeps its own steps.
Falsifier: In the probe, one wheel-down notch over any of those widgets, taller than its viewport, moves its first visible line by other than 2 lines; or one shift or ctrl wheel-down notch, or one wheel-right notch, over a Container, a HorizontalScroll or a DataTable wider than its viewport moves it by other than 4 columns; or a wheel-down notch over a Log already scrolled to its end does not scroll the screen around it; in full-screen or in inline mode.
Mechanism: scroll-pty
Rationale: Python's App.scroll_sensitivity_y is 2.0 and scroll_sensitivity_x 4.0 (app.py:744-749); Widget._on_mouse_scroll_down/up/right/left (widget.py:4777-4805) scroll by them, turn a ctrl or shift notch horizontal, and stop the event only when they scrolled, and only the Footer overrides them. Here most widgets scrolled 1 line or 2 columns, only shift turned a notch horizontal, DataTable ignored vertical notches and scrolled whole columns, and Log and RichLog kept notches they could not use.
Status: Agreed 2026-09-27

[SCL-002] A click on a scrollbar's track MUST scroll one page toward
the click with animation, at Python's speed of 50 lines (or columns) a
second with an out-cubic ease, and each drag of its thumb MUST animate
to the new position over 0.1 seconds; a horizontal wheel notch MUST
animate as a track click does. Animations that are off
(`TEXTUAL_ANIMATIONS=none`) scroll at once. This holds for every
scrollbar the port draws: on the app's screen, pushed screens,
containers, the ScrollView family, Log, RichLog, OptionList and
DataTable, in full-screen and in inline mode.
Falsifier: After a click on the track past the thumb of any of those scrollbars, with 60 lines or columns to scroll, no frame the terminal draws shows the content between its old and its new position; or a track click or a horizontal wheel notch does not animate at 50 lines or columns a second with an out-cubic ease, or a thumb drag does not animate over 0.1 seconds; in full-screen or in inline mode.
Mechanism: scroll-pty
Rationale: Python's ScrollBar posts ScrollDown/ScrollUp when the track is pressed (scrollbar.py:141-142, 338-346), which Widget pages with animation at speed 50 and DEFAULT_SCROLL_EASING out_cubic (widget.py:2767-2773, 3389-3400, 4812-4820), and ScrollTo(animate=not supports_smooth_scrolling) for a drag, scrolled over duration 0.1 (scrollbar.py:395, widget.py:4807-4810); smooth scrolling needs in-band resize, which this port never turns on. Here only the app's screen (over 100 ms) and the ScrollView family animated, and pushed screens, containers, Log, RichLog, OptionList and DataTable jumped.
Status: Agreed 2026-09-27
