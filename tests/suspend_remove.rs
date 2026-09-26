//! PR-14: `App.suspend` signals and `AwaitRemove` completion ordering.
//!
//! Python parity (`app.py`): suspending publishes suspend *before* the driver
//! stops and resume *after* it restarts; `remove` returns an awaitable that
//! resolves once the loop has drained the removal's unmount work.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rich_rs::{Console, ConsoleOptions, Segments};
use rusty_textual::prelude::*;
use rusty_textual::runtime::{App, AppResumed, AppSuspended};

static SUSPENDS: AtomicUsize = AtomicUsize::new(0);
static RESUMES: AtomicUsize = AtomicUsize::new(0);

#[allow(clippy::trivially_copy_pass_by_ref)] // `Signal` handlers receive the payload as `&T`.
fn on_suspend(_: &AppSuspended) -> SignalResponse {
    SUSPENDS.fetch_add(1, Ordering::SeqCst);
    SignalResponse::Continue
}

#[allow(clippy::trivially_copy_pass_by_ref)] // `Signal` handlers receive the payload as `&T`.
fn on_resume(_: &AppResumed) -> SignalResponse {
    RESUMES.fetch_add(1, Ordering::SeqCst);
    SignalResponse::Continue
}

#[test]
fn suspend_publishes_signals_in_order() {
    SUSPENDS.store(0, Ordering::SeqCst);
    RESUMES.store(0, Ordering::SeqCst);
    let mut app = App::new().expect("app initializes");
    app.subscribe_suspend(NodeId::default(), on_suspend);
    app.subscribe_resume(NodeId::default(), on_resume);
    assert!(!app.is_suspended());

    let guard = app.suspend().expect("suspend");
    assert!(guard.is_active());
    assert_eq!(SUSPENDS.load(Ordering::SeqCst), 1);
    assert_eq!(RESUMES.load(Ordering::SeqCst), 0);

    drop(guard);
    assert_eq!(RESUMES.load(Ordering::SeqCst), 1);
    assert!(!app.is_suspended());
}

struct ProbeWidget {
    unmounts: Arc<AtomicUsize>,
}

impl Widget for ProbeWidget {
    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        Segments::new()
    }

    fn on_unmount(&mut self) {
        self.unmounts.fetch_add(1, Ordering::SeqCst);
    }
}

struct ProbeApp {
    unmounts: Arc<AtomicUsize>,
}

impl TextualApp for ProbeApp {
    fn compose(&mut self) -> AppRoot {
        AppRoot::new().with_child(ProbeWidget {
            unmounts: self.unmounts.clone(),
        })
    }
}

#[test]
fn await_remove_completes_after_loop_drain() {
    let unmounts = Arc::new(AtomicUsize::new(0));
    let probe_unmounts = unmounts.clone();
    run_test(ProbeApp { unmounts }, |pilot| {
        let await_remove = pilot.app_mut().remove("ProbeWidget").expect("remove probe");
        assert_eq!(await_remove.removed().len(), 1);
        assert!(
            !await_remove.is_complete(pilot.app()),
            "unmount work not yet drained"
        );
        pilot.pause()?;
        assert!(
            await_remove.is_complete(pilot.app()),
            "loop drain completes the await"
        );
        assert_eq!(probe_unmounts.load(Ordering::SeqCst), 1);
        Ok(())
    })
    .expect("run_test");
}
