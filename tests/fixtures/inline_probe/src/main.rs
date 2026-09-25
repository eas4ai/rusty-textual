//! Inline-mode probe app for `tests/inline_mode.rs` (the `inline-pty` Sudus
//! mechanism). One binary covers every case; environment variables choose
//! the behavior:
//!
//! - `PROBE_MODE`: `inline` (the default), `inline-no-clear`, or `full`.
//! - `PROBE_PADDING`: inline padding lines (unset: 1, Python's default).
//! - `PROBE_LINES`: lines in the body (default 3).
//! - `PROBE_BUTTON`: when set, a `Press` button follows the body.
//! - `PROBE_EXIT_MESSAGE`: when set, `q` exits through `App::exit` with this
//!   message (Python `App.exit(message=...)`).
//! - `PROBE_EXIT_RESULT`: when set, the app returns this value
//!   (`take_exit_output`, Python `App.exit(result=...)`); `main` prints it.
//! - `PROBE_EXIT_IN_CONFIGURE`: when set, `configure` calls `App::exit`, so the
//!   app stops before it starts.
//!
//! Keys: `s` shrinks the body to one line, `z` tries `App::suspend`, `x`
//! runs the suspend-process action, `q` quits. The status line counts every
//! other key that arrives (`keys:N`) and shows the last suspend result, and
//! `clicked` once the button has been pressed.

use textual::prelude::*;

const CSS: &str = "
#cssmark { display: none; }
Screen:inline #cssmark { display: block; }
";

struct Probe {
    lines: usize,
    padding: usize,
    button: bool,
    exit_message: Option<String>,
    exit_result: Option<String>,
    exit_in_configure: bool,
    other_keys: usize,
    suspend: &'static str,
    clicked: bool,
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
            button: std::env::var_os("PROBE_BUTTON").is_some(),
            exit_message: std::env::var("PROBE_EXIT_MESSAGE").ok(),
            exit_result: std::env::var("PROBE_EXIT_RESULT").ok(),
            exit_in_configure: std::env::var_os("PROBE_EXIT_IN_CONFIGURE").is_some(),
            other_keys: 0,
            suspend: "none",
            clicked: false,
        }
    }

    fn body(&self) -> String {
        (1..=self.lines)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn status(&self) -> String {
        let clicked = if self.clicked { " clicked" } else { "" };
        format!("keys:{} suspend:{}{clicked}", self.other_keys, self.suspend)
    }

    fn refresh(&self, app: &mut App) {
        let body = self.body();
        let status = self.status();
        let _ = app.with_query_one_mut_as::<Static, _>("#body", |s| s.update(body));
        let _ = app.with_query_one_mut_as::<Static, _>("#status", |s| s.update(status));
    }
}

impl TextualApp for Probe {
    fn configure(&mut self, app: &mut App) -> textual::Result<()> {
        app.load_stylesheet(CSS);
        if self.exit_in_configure {
            app.exit(None, 0, None);
        }
        Ok(())
    }

    fn compose(&mut self) -> AppRoot {
        let root = AppRoot::new()
            .with_child(Static::new(self.body()).id("body"))
            .with_child(Static::new(self.status()).id("status"))
            .with_child(Static::new("inline-css").id("cssmark"));
        if self.button {
            root.with_child(Button::new("Press").id("press"))
        } else {
            root
        }
    }

    fn inline_padding(&self) -> usize {
        self.padding
    }

    fn on_key_with_app(&mut self, app: &mut App, key: &KeyEventData, ctx: &mut WidgetCtx) {
        match key.key.as_str() {
            "s" => self.lines = 1,
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
        self.refresh(app);
    }

    fn on_message_with_app(&mut self, app: &mut App, message: &MessageEvent, ctx: &mut WidgetCtx) {
        if message.downcast_ref::<ButtonPressed>().is_some() {
            self.clicked = true;
            ctx.set_handled();
            self.refresh(app);
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
