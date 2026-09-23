//! PR-04 regression: unmounting a widget cancels its workers.
//!
//! A widget whose `on_mount` starts a never-completing (parked) worker must
//! have that worker cancelled when the widget is unmounted — Python cancels
//! node workers on unmount. Pre-fix, unmount purged only timers, so the
//! parked worker ran forever and the job never observed cancellation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rich_rs::{Console, ConsoleOptions, Segments};
use rusty_textual::prelude::*;
use rusty_textual::reactive::{ReactiveCtx, RuntimeReactiveEntry, enqueue_runtime_reactive_entry};
use rusty_textual::runtime::Pilot;
use rusty_textual::widgets::Widget;

// ---------------------------------------------------------------------------
// Fixture: a worker that parks until its cancellation token fires.
// ---------------------------------------------------------------------------

struct ParkingWorker {
    started: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
}

impl Widget for ParkingWorker {
    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        Segments::new()
    }

    fn style_type(&self) -> &'static str {
        "ParkingWorker"
    }

    fn on_mount(&mut self, ctx: &mut WidgetCtx) {
        let started = Arc::clone(&self.started);
        let exited = Arc::clone(&self.exited);
        ctx.request_worker_task(Some("parker"), move |cancel| {
            started.store(true, Ordering::SeqCst);
            while !cancel.is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            exited.store(true, Ordering::SeqCst);
            Ok(())
        });
    }
}

// ---------------------------------------------------------------------------
// Host: shows the parking worker until a keypress recomposes it away.
// ---------------------------------------------------------------------------

#[derive(Reactive)]
struct Host {
    #[reactive(recompose)]
    show: bool,
    started: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
}

impl Widget for Host {
    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        Segments::new()
    }

    fn style_type(&self) -> &'static str {
        "Host"
    }

    fn focusable(&self) -> bool {
        true
    }

    fn reactive_widget(&mut self) -> Option<&mut dyn rusty_textual::reactive::ReactiveWidget> {
        Some(self)
    }

    fn compose(&mut self) -> ComposeResult {
        if *self.show() {
            vec![ChildDecl::new(Box::new(ParkingWorker {
                started: Arc::clone(&self.started),
                exited: Arc::clone(&self.exited),
            }))]
        } else {
            Vec::new()
        }
    }

    fn on_event(&mut self, event: &Event, ctx: &mut WidgetCtx) {
        if let Event::Key(_) = event {
            let node_id = self.node_id();
            let mut reactive = ReactiveCtx::new(node_id);
            self.set_show(false, &mut reactive);
            if reactive.has_changes() {
                enqueue_runtime_reactive_entry(RuntimeReactiveEntry::new(node_id, reactive));
                ctx.set_handled();
            }
        }
    }
}

struct UnmountApp {
    started: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
}

impl TextualApp for UnmountApp {
    fn compose(&mut self) -> AppRoot {
        AppRoot::new().with_child(Host {
            show: true,
            started: Arc::clone(&self.started),
            exited: Arc::clone(&self.exited),
        })
    }
}

#[test]
fn unmount_cancels_parked_worker() {
    let started = Arc::new(AtomicBool::new(false));
    let exited = Arc::new(AtomicBool::new(false));
    let app = UnmountApp {
        started: Arc::clone(&started),
        exited: Arc::clone(&exited),
    };

    rusty_textual::run_test(app, |pilot: &mut Pilot| {
        pilot.pause()?;
        assert!(
            started.load(Ordering::SeqCst),
            "parking worker must be running before the unmount"
        );
        assert!(
            !exited.load(Ordering::SeqCst),
            "worker must not have exited before the unmount"
        );

        // Recompose the worker away; the Unmount drain must cancel it.
        pilot.app_mut().action_focus_next();
        pilot.press(&["r"])?;
        pilot.pause()?;

        // Cancellation is async (worker thread observes the token): poll.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !exited.load(Ordering::SeqCst) {
            if std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            exited.load(Ordering::SeqCst),
            "unmounting the widget must cancel its parked worker (was never \
             cancelled pre-fix: unmount purged only timers)"
        );
        Ok(())
    })
    .expect("headless run_test must succeed");
}
