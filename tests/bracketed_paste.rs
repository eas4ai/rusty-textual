//! PR-15a: bracketed-paste payloads arrive as one `Event::Paste`, not raw keys.
//!
//! Drives `Pilot::paste` (the headless form of DECSET-2004 bytes, enabled at
//! driver start) and asserts the focused widget receives the exact text.

use std::sync::{Arc, Mutex};

use rich_rs::{Console, ConsoleOptions, Segments};
use textual::prelude::*;

struct PasteObserver {
    pastes: Arc<Mutex<Vec<String>>>,
}

impl Widget for PasteObserver {
    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        Segments::new()
    }

    // NOTE: the `Widget` trait hooks, not the same-named `Interactive` /
    // `Focus` capability hooks — the runtime dispatches and focus-checks
    // through the `Widget` trait object (PR-15a).
    fn on_event(&mut self, event: &Event, _ctx: &mut WidgetCtx) {
        if let Event::Paste(paste) = event {
            self.pastes.lock().unwrap().push(paste.text.clone());
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

struct PasteApp {
    pastes: Arc<Mutex<Vec<String>>>,
}

impl TextualApp for PasteApp {
    fn compose(&mut self) -> AppRoot {
        AppRoot::new().with_child(PasteObserver {
            pastes: self.pastes.clone(),
        })
    }
}

#[test]
fn paste_arrives_as_single_paste_event() {
    let pastes = Arc::new(Mutex::new(Vec::new()));
    let probe = pastes.clone();
    run_test(PasteApp { pastes }, |pilot| {
        pilot
            .app_mut()
            .query_mut("PasteObserver")
            .expect("observer mounted")
            .focus();
        pilot.paste("hello,\nworld\t!")?;
        assert_eq!(
            *probe.lock().unwrap(),
            vec!["hello,\nworld\t!".to_string()],
            "paste payload must arrive whole, not as keystrokes"
        );
        Ok(())
    })
    .expect("run_test");
}
