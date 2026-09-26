//! Inline-mode probe app for `tests/inline_mode.rs` (the `inline-pty` Sudus
//! mechanism). One binary covers every case; environment variables choose
//! the behavior:
//!
//! - `PROBE_MODE`: `inline` (the default), `inline-no-clear`, or `full`.
//! - `PROBE_PADDING`: inline padding lines (unset: 1, Python's default).
//! - `PROBE_LINES`: lines in the body (default 3).
//! - `PROBE_SCREEN_HEIGHT`: when set, the height of a `Screen:inline` rule
//!   (for example `10`), so the inline height is below the content height.
//! - `PROBE_SCREEN_OVERFLOW`: when set, the `overflow-y` of a `Screen` rule
//!   (for example `hidden`), for every screen, pushed or not.
//! - `PROBE_BUTTON`: when set, a `Press` button follows the body.
//! - `PROBE_HOVER`: when set, a `hover here` line comes before the body, away
//!   from the status line. Moving the pointer over it posts a message, and
//!   the app's message handler updates the status line.
//! - `PROBE_HOVER_PATH`: the key whose update path that message handler uses
//!   (see `Probe::show_status`; unset: `with_query_one_mut_as`).
//! - `PROBE_SHARED`: when set, a `shared:N` line comes two rows below the
//!   status line. It draws a count kept outside the widget: `c` adds one
//!   without asking for a repaint, and `r` repaints the line with
//!   `DomQueryMut::refresh`.
//! - `PROBE_EXIT_MESSAGE`: when set, `q` exits through `App::exit` with this
//!   message (Python `App.exit(message=...)`).
//! - `PROBE_EXIT_RESULT`: when set, the app returns this value
//!   (`take_exit_output`, Python `App.exit(result=...)`); `main` prints it.
//! - `PROBE_EXIT_IN_CONFIGURE`: when set, `configure` calls `App::exit`, so the
//!   app stops before it starts.
//! - `PROBE_PUSH`: `screen` or `modal`; `p` then pushes a `Screen` or a
//!   `ModalScreen` of 60 lines, `pushed 1` to `pushed 60`, then `pushed-end`
//!   (Python `App.push_screen`).
//!
//! Keys: `s` shrinks the body to one line, `z` tries `App::suspend`, `x`
//! runs the suspend-process action, `p` pushes the `PROBE_PUSH` screen, `c`
//! and `r` change and repaint the `PROBE_SHARED` line, `q` quits. The status
//! line counts every other key that arrives (`keys:N`) and shows the last
//! suspend result, `clicked` once the button has been pressed, and `hovered`
//! once the pointer has moved over the hover line. The key chooses the path
//! that writes the status line (see `Probe::show_status`).

use std::any::Any;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};

use textual::prelude::*;

const CSS: &str = "
#cssmark { display: none; }
Screen:inline #cssmark { display: block; }
HoverLine { height: 1; }
SharedLine { height: 1; margin-top: 2; }
";

const PUSHED_LINES: usize = 60;

/// Posted when the pointer moves over the `PROBE_HOVER` line.
#[derive(Debug, Clone)]
struct Hovered;

textual::impl_message!(Hovered);

/// The `PROBE_HOVER` line: posts `Hovered` for each pointer move over it.
struct HoverLine(Static);

impl Widget for HoverLine {
    fn style_type(&self) -> &'static str {
        "HoverLine"
    }

    fn render(
        &self,
        console: &rich_rs::Console,
        options: &rich_rs::ConsoleOptions,
    ) -> rich_rs::Segments {
        self.0.render(console, options)
    }

    fn on_event(&mut self, event: &Event, ctx: &mut WidgetCtx) {
        if matches!(event, Event::MouseMove(_)) {
            ctx.post_message(Hovered);
        }
    }
}

/// The count the `PROBE_SHARED` line draws. It lives outside the widget, so
/// changing it changes what the line draws without asking for a repaint.
static SHARED: AtomicUsize = AtomicUsize::new(0);

/// The `PROBE_SHARED` line: draws `shared:N` from `SHARED`. Its top margin
/// keeps it out of the status line's repaint region, which the renderer
/// widens by one cell.
struct SharedLine;

impl Widget for SharedLine {
    fn style_type(&self) -> &'static str {
        "SharedLine"
    }

    fn render(
        &self,
        console: &rich_rs::Console,
        options: &rich_rs::ConsoleOptions,
    ) -> rich_rs::Segments {
        let count = SHARED.load(Ordering::Relaxed);
        Static::new(format!("shared:{count}")).render(console, options)
    }
}

/// Sets the text of `widget` when it is a `Static`.
fn set_static(widget: &mut dyn Widget, text: String) {
    if let Some(label) = (widget as &mut dyn Any).downcast_mut::<Static>() {
        label.update(text);
    }
}

/// The screen `p` pushes: taller than the 30-row test terminal.
struct Pushed {
    modal: bool,
}

impl Screen for Pushed {
    fn compose(&self) -> Box<dyn Widget> {
        let lines: Vec<String> = (1..=PUSHED_LINES).map(|n| format!("pushed {n}")).collect();
        Box::new(Static::new(format!("{}\npushed-end", lines.join("\n"))))
    }

    fn is_modal(&self) -> bool {
        self.modal
    }
}

/// The optional widgets the probe shows (`PROBE_BUTTON`, `PROBE_HOVER`,
/// `PROBE_SHARED`).
struct Extras {
    button: bool,
    hover: bool,
    shared: bool,
}

struct Probe {
    lines: usize,
    padding: usize,
    screen_height: Option<String>,
    screen_overflow: Option<String>,
    extras: Extras,
    hover_path: String,
    exit_message: Option<String>,
    exit_result: Option<String>,
    exit_in_configure: bool,
    push: Option<String>,
    other_keys: usize,
    suspend: &'static str,
    /// Marks shown after the counts, in the order they first happened:
    /// `clicked` and `hovered`.
    marks: Vec<&'static str>,
}

impl Probe {
    fn from_env() -> Self {
        let number = |name: &str, default: usize| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default)
        };
        Self {
            lines: number("PROBE_LINES", 3),
            padding: number("PROBE_PADDING", 1),
            screen_height: std::env::var("PROBE_SCREEN_HEIGHT").ok(),
            screen_overflow: std::env::var("PROBE_SCREEN_OVERFLOW").ok(),
            extras: Extras {
                button: std::env::var_os("PROBE_BUTTON").is_some(),
                hover: std::env::var_os("PROBE_HOVER").is_some(),
                shared: std::env::var_os("PROBE_SHARED").is_some(),
            },
            hover_path: std::env::var("PROBE_HOVER_PATH").unwrap_or_default(),
            exit_message: std::env::var("PROBE_EXIT_MESSAGE").ok(),
            exit_result: std::env::var("PROBE_EXIT_RESULT").ok(),
            exit_in_configure: std::env::var_os("PROBE_EXIT_IN_CONFIGURE").is_some(),
            push: std::env::var("PROBE_PUSH").ok(),
            other_keys: 0,
            suspend: "none",
            marks: Vec::new(),
        }
    }

    fn body(&self) -> String {
        (1..=self.lines)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn status(&self) -> String {
        let mut status = format!("keys:{} suspend:{}", self.other_keys, self.suspend);
        for mark in &self.marks {
            status.push(' ');
            status.push_str(mark);
        }
        status
    }

    /// Adds `mark` to the status line unless it is there already; returns
    /// whether it was added.
    fn mark(&mut self, mark: &'static str) -> bool {
        let added = !self.marks.contains(&mark);
        if added {
            self.marks.push(mark);
        }
        added
    }

    /// Writes the status line through the update path `key` names: `1`
    /// `App::with_widget_mut`, `2` `with_widget_mut_as`, `3`
    /// `with_query_one_mut`, `5` `with_widget_taken_as`, `6` `query_mut` then
    /// `DomQueryMut::update`, and any other key `with_query_one_mut_as`.
    fn show_status(&self, app: &mut App, key: &str) {
        let status = self.status();
        let Ok(id) = app.query_one("#status") else {
            return;
        };
        match key {
            "1" => {
                let _ = app.with_widget_mut(id, |w| set_static(w, status));
            }
            "2" => {
                let _ = app.with_widget_mut_as::<Static, _>(id, |s| s.update(status));
            }
            "3" => {
                let _ = app.with_query_one_mut("#status", |w| set_static(w, status));
            }
            "5" => {
                let _ = app.with_widget_taken_as::<Static, _>(id, |s, _| s.update(status));
            }
            "6" => {
                let _ = app
                    .query_mut("#status")
                    .map(|query| query.update(|w| set_static(w, status.clone())));
            }
            _ => {
                let _ = app.with_query_one_mut_as::<Static, _>("#status", |s| s.update(status));
            }
        }
    }
}

impl TextualApp for Probe {
    fn configure(&mut self, app: &mut App) -> textual::Result<()> {
        let mut css = CSS.to_string();
        if let Some(height) = &self.screen_height {
            let _ = writeln!(css, "Screen:inline {{ height: {height}; }}");
        }
        if let Some(overflow) = &self.screen_overflow {
            let _ = writeln!(css, "Screen {{ overflow-y: {overflow}; }}");
        }
        app.load_stylesheet(&css);
        if self.exit_in_configure {
            app.exit(None, 0, None);
        }
        Ok(())
    }

    fn compose(&mut self) -> AppRoot {
        let mut root = AppRoot::new();
        if self.extras.hover {
            root = root.with_child(HoverLine(Static::new("hover here")));
        }
        root = root
            .with_child(Static::new(self.body()).id("body"))
            .with_child(Static::new(self.status()).id("status"));
        if self.extras.shared {
            root = root.with_child(SharedLine);
        }
        let root = root.with_child(Static::new("inline-css").id("cssmark"));
        if self.extras.button {
            root.with_child(Button::new("Press").id("press"))
        } else {
            root
        }
    }

    fn inline_padding(&self) -> usize {
        self.padding
    }

    fn on_key_with_app(&mut self, app: &mut App, key: &KeyEventData, ctx: &mut WidgetCtx) {
        let key = key.key.as_str();
        match key {
            "s" => {
                self.lines = 1;
                let body = self.body();
                let _ = app.with_query_one_mut_as::<Static, _>("#body", |s| s.update(body));
            }
            "z" => {
                self.suspend = match app.suspend() {
                    Ok(_guard) => "ok",
                    Err(_) => "refused",
                };
            }
            "x" => {
                self.suspend = if app.action_suspend_process() {
                    "process"
                } else {
                    "process-refused"
                };
            }
            "p" if self.push.is_some() => {
                let modal = self.push.as_deref() == Some("modal");
                app.push_screen(Box::new(Pushed { modal }))
                    .expect("push the PROBE_PUSH screen");
                ctx.set_handled();
                return;
            }
            "c" if self.extras.shared => {
                SHARED.fetch_add(1, Ordering::Relaxed);
                ctx.set_handled();
                return;
            }
            "r" if self.extras.shared => {
                let _ = app.query_mut("SharedLine").map(DomQueryMut::refresh);
                ctx.set_handled();
                return;
            }
            "q" => {
                if let Some(message) = self.exit_message.take() {
                    app.exit(None, 0, Some(message));
                }
                ctx.request_stop();
                ctx.set_handled();
                return;
            }
            _ => self.other_keys += 1,
        }
        ctx.set_handled();
        self.show_status(app, key);
    }

    fn on_message_with_app(&mut self, app: &mut App, message: &MessageEvent, ctx: &mut WidgetCtx) {
        if message.downcast_ref::<ButtonPressed>().is_some() {
            self.mark("clicked");
            ctx.set_handled();
            self.show_status(app, "");
        } else if message.downcast_ref::<Hovered>().is_some() && self.mark("hovered") {
            // Not marked handled, like the mouse01 example: a handled message
            // repaints the whole frame, so only the update asks for a repaint.
            self.show_status(app, &self.hover_path);
        }
    }

    fn take_exit_output(&mut self) -> Option<String> {
        self.exit_result.take()
    }
}

fn main() -> textual::Result<()> {
    let mode = std::env::var("PROBE_MODE").unwrap_or_default();
    let options = RunOptions {
        inline: mode != "full",
        inline_no_clear: mode == "inline-no-clear",
    };
    if let Some(result) = run_sync_with_options(Probe::from_env(), options)? {
        println!("{result}");
    }
    Ok(())
}
