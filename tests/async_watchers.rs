//! PR-08c regression: async watchers (`watch_async`) run on the worker pool.
//!
//! Python awaits `async def watch_*` inline. Static Rust has no inline
//! executor in dispatch, so the future rides the worker pool (plain threads
//! with a dedicated current-thread runtime — never a nesting runtime).
//! The future must be `'static`: it receives owned clones, never borrows.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rich_rs::{Console, ConsoleOptions, Segments};
use textual::prelude::*;
use textual::reactive::{ReactiveCtx, RuntimeReactiveEntry, enqueue_runtime_reactive_entry};
use textual::runtime::Pilot;
use textual::widgets::Widget;

#[derive(Reactive)]
struct AsyncHost {
    #[reactive(watch_async)]
    tick: u64,
    done: Arc<AtomicBool>,
}

impl AsyncHost {
    fn watch_tick(
        &self,
        _old: u64,
        new: u64,
    ) -> impl std::future::Future<Output = ()> + Send + 'static {
        let done = Arc::clone(&self.done);
        async move {
            // A real await point: proves a working reactor, not a stub.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert!(new > 0, "watcher must observe the new value");
            done.store(true, Ordering::SeqCst);
        }
    }
}

impl Widget for AsyncHost {
    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        Segments::new()
    }

    fn style_type(&self) -> &'static str {
        "AsyncHost"
    }

    fn focusable(&self) -> bool {
        true
    }

    fn reactive_widget(&mut self) -> Option<&mut dyn textual::reactive::ReactiveWidget> {
        Some(self)
    }

    fn on_event(&mut self, event: &Event, ctx: &mut WidgetCtx) {
        if let Event::Key(_) = event {
            let node_id = self.node_id();
            let mut reactive = ReactiveCtx::new(node_id);
            let next = *self.tick() + 1;
            self.set_tick(next, &mut reactive);
            if reactive.has_changes() {
                enqueue_runtime_reactive_entry(RuntimeReactiveEntry::new(node_id, reactive));
                ctx.set_handled();
            }
        }
    }
}

struct AsyncApp {
    done: Arc<AtomicBool>,
}

impl TextualApp for AsyncApp {
    fn compose(&mut self) -> AppRoot {
        AppRoot::new().with_child(AsyncHost {
            tick: 0,
            done: Arc::clone(&self.done),
        })
    }
}

#[test]
fn async_watcher_runs_on_worker_pool() {
    let done = Arc::new(AtomicBool::new(false));
    let app = AsyncApp {
        done: Arc::clone(&done),
    };

    textual::run_test(app, |pilot: &mut Pilot| {
        pilot.pause()?;
        assert!(
            !done.load(Ordering::SeqCst),
            "no change yet — watcher must not have run"
        );

        pilot.app_mut().action_focus_next();
        pilot.press(&["r"])?;
        pilot.pause()?;

        // Worker completion is pump-awaited, but poll defensively.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !done.load(Ordering::SeqCst) {
            if std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            done.load(Ordering::SeqCst),
            "async watcher future must run to completion on the worker pool"
        );
        Ok(())
    })
    .expect("headless run_test must succeed");
}
