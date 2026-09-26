//! P-E: explicit mouse capture + click chains (Python `App.capture_mouse`,
//! `Click.chain`).
//!
//! - Rapid clicks on one spot extend `chain` (500ms threshold).
//! - `capture_mouse(node)` retargets down/up to the captured widget
//!   regardless of pointer position; `capture_mouse(None)` releases.
use rich_rs::{Console, ConsoleOptions, Segments};
use rusty_textual::prelude::*;
use std::sync::{Arc, Mutex};

/// Records MouseDown/MouseUp/Click deliveries with click chains.
#[derive(Debug, Default)]
struct ClickProbeState {
    downs: usize,
    ups: usize,
    chains: Vec<u16>,
}

struct ClickProbe {
    state: Arc<Mutex<ClickProbeState>>,
}

impl Widget for ClickProbe {
    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        // Non-empty render: hit-testing maps cells, so an empty widget is
        // unhittable and clicks would miss it.
        Segments::from(vec![rich_rs::Segment::new("clickme")])
    }

    fn layout_height(&self) -> Option<usize> {
        Some(1)
    }

    fn on_event(&mut self, event: &Event, _ctx: &mut WidgetCtx) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match event {
            Event::MouseDown(_) => state.downs += 1,
            Event::MouseUp(_) => state.ups += 1,
            Event::Click(click) => state.chains.push(click.chain),
            _ => {}
        }
    }
}

struct ProbeApp {
    state: Arc<Mutex<ClickProbeState>>,
}

impl TextualApp for ProbeApp {
    fn compose(&mut self) -> AppRoot {
        AppRoot::new().with_child(ClickProbe {
            state: self.state.clone(),
        })
    }
}

fn probe_app() -> (ProbeApp, Arc<Mutex<ClickProbeState>>) {
    let state = Arc::new(Mutex::new(ClickProbeState::default()));
    (
        ProbeApp {
            state: state.clone(),
        },
        state,
    )
}

fn snapshot(state: &Arc<Mutex<ClickProbeState>>) -> (usize, usize, Vec<u16>) {
    let state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (state.downs, state.ups, state.chains.clone())
}

/// The probe renders "clickme" at the top-left; (2, 0) hits its cells.
/// Every test asserts down counts, so a layout miss fails loudly.
const PROBE_SPOT: (u16, u16) = (2, 0);

/// Empty background far from the one-row probe (80x24 terminal).
const EMPTY_SPOT: (u16, u16) = (79, 23);

/// Rapid clicks on one spot chain: 1, 2, 3.
#[test]
fn rapid_clicks_extend_chain() {
    let (app, state) = probe_app();
    run_test(app, |pilot| {
        pilot.pause()?;
        pilot.click_at(PROBE_SPOT.0, PROBE_SPOT.1)?;
        pilot.click_at(PROBE_SPOT.0, PROBE_SPOT.1)?;
        pilot.click_at(PROBE_SPOT.0, PROBE_SPOT.1)?;
        assert_eq!(snapshot(&state).0, 3, "clicks must land on the probe");
        assert_eq!(
            snapshot(&state).2,
            vec![1, 2, 3],
            "rapid same-spot clicks must chain"
        );
        Ok(())
    })
    .expect("run_test");
}

/// A different offset on the same widget restarts the chain (Python: the
/// chain needs the same screen offset).
#[test]
fn moved_clicks_restart_chain() {
    let (app, state) = probe_app();
    run_test(app, |pilot| {
        pilot.pause()?;
        pilot.click_at(2, 0)?;
        pilot.click_at(5, 0)?;
        pilot.click_at(2, 0)?;
        assert_eq!(
            snapshot(&state).2,
            vec![1, 1, 1],
            "chain restarts when the offset moves"
        );
        Ok(())
    })
    .expect("run_test");
}

/// Clicking empty space produces no click and preserves the chain state
/// (Python only updates chain tracking when a click is emitted).
#[test]
fn empty_clicks_emit_nothing_and_preserve_chain() {
    let (app, state) = probe_app();
    run_test(app, |pilot| {
        pilot.pause()?;
        pilot.click_at(PROBE_SPOT.0, PROBE_SPOT.1)?;
        pilot.click_at(EMPTY_SPOT.0, EMPTY_SPOT.1)?;
        pilot.click_at(PROBE_SPOT.0, PROBE_SPOT.1)?;
        let (downs, _, chains) = snapshot(&state);
        assert_eq!(downs, 2, "empty-space press must not reach the probe");
        assert_eq!(chains, vec![1, 2], "chain state survives empty clicks");
        Ok(())
    })
    .expect("run_test");
}

/// Capture retargets presses to the captured widget; release restores.
#[test]
fn capture_mouse_retargets_and_releases() {
    let (app, state) = probe_app();
    run_test(app, |pilot| {
        pilot.pause()?;
        assert_eq!(pilot.app().mouse_captured(), None);
        let node = pilot.app().query_one("ClickProbe").expect("probe node");

        // Capture the probe, then click empty space: the probe gets the
        // whole down/up/click cycle anyway.
        pilot.app_mut().capture_mouse(Some(node));
        assert_eq!(pilot.app().mouse_captured(), Some(node));
        pilot.click_at(EMPTY_SPOT.0, EMPTY_SPOT.1)?;
        assert_eq!(
            snapshot(&state),
            (1, 1, vec![1]),
            "captured widget receives the off-target cycle"
        );

        // Release: empty-space clicks reach nothing again.
        pilot.app_mut().capture_mouse(None);
        assert_eq!(pilot.app().mouse_captured(), None);
        pilot.click_at(EMPTY_SPOT.0, EMPTY_SPOT.1)?;
        assert_eq!(
            snapshot(&state),
            (1, 1, vec![1]),
            "released capture restores hit-testing"
        );
        Ok(())
    })
    .expect("run_test");
}

// ── R7-deferred: MouseCapture / MouseRelease notices ─────────────────────
// (Python `events.MouseCapture` / `events.MouseRelease`, `bubble=False`).

/// Shared notice log: `(tag, "capture"|"release", screen_x, screen_y)` in
/// delivery order.
#[derive(Debug, Default)]
struct NoticeState {
    notices: Vec<(String, String, u16, u16)>,
}

struct MouseProbe {
    seed: NodeSeed,
    tag: String,
    state: Arc<Mutex<NoticeState>>,
    with_child: bool,
}

impl MouseProbe {
    fn new(id: &str, tag: &str, state: Arc<Mutex<NoticeState>>, with_child: bool) -> Self {
        Self {
            seed: NodeSeed {
                css_id: Some(id.to_string()),
                ..NodeSeed::default()
            },
            tag: tag.to_string(),
            state,
            with_child,
        }
    }

    fn record(&self, kind: &str, x: u16, y: u16) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .notices
            .push((self.tag.clone(), kind.to_string(), x, y));
    }
}

impl Widget for MouseProbe {
    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        Segments::from(vec![rich_rs::Segment::new("mouseprobe")])
    }

    fn layout_height(&self) -> Option<usize> {
        Some(2)
    }

    fn take_node_seed(&mut self) -> NodeSeed {
        std::mem::take(&mut self.seed)
    }

    fn compose(&mut self) -> ComposeResult {
        if self.with_child {
            vec![ChildDecl::new(Box::new(MouseProbe::new(
                "child",
                "child",
                self.state.clone(),
                false,
            )))]
        } else {
            vec![]
        }
    }

    fn on_event(&mut self, event: &Event, _ctx: &mut WidgetCtx) {
        match event {
            Event::MouseCapture(e) => self.record("capture", e.screen_x, e.screen_y),
            Event::MouseRelease(e) => self.record("release", e.screen_x, e.screen_y),
            _ => {}
        }
    }
}

struct NoticeApp {
    probes: Vec<MouseProbe>,
}

impl TextualApp for NoticeApp {
    fn compose(&mut self) -> AppRoot {
        let mut root = AppRoot::new();
        for probe in self.probes.drain(..) {
            root = root.with_child(probe);
        }
        root
    }
}

fn notice_app(ids: &[&str]) -> (NoticeApp, Arc<Mutex<NoticeState>>) {
    let state = Arc::new(Mutex::new(NoticeState::default()));
    let app = NoticeApp {
        probes: ids
            .iter()
            .map(|id| MouseProbe::new(id, id, state.clone(), false))
            .collect(),
    };
    (app, state)
}

fn notices(state: &Arc<Mutex<NoticeState>>) -> Vec<(String, String, u16, u16)> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .notices
        .clone()
}

/// Capturing queues a `MouseCapture` carrying the pointer position at call
/// time (Python `App.capture_mouse` posts `MouseCapture(mouse_position)`).
#[test]
fn capture_posts_capture_with_pointer_position() {
    let (app, state) = notice_app(&["probe"]);
    run_test(app, |pilot| {
        pilot.pause()?;
        pilot.move_to(10, 5)?;
        let node = pilot.app().query_one("#probe").expect("probe node");
        pilot.app_mut().capture_mouse(Some(node));
        assert_eq!(pilot.app().mouse_captured(), Some(node));
        pilot.pause()?;
        assert_eq!(
            notices(&state),
            vec![("probe".to_string(), "capture".to_string(), 10, 5)],
            "capture must notify with the position at call time"
        );
        Ok(())
    })
    .expect("run_test");
}

/// Releasing queues a `MouseRelease` to the previous holder.
#[test]
fn release_posts_release_to_previous_holder() {
    let (app, state) = notice_app(&["probe"]);
    run_test(app, |pilot| {
        pilot.pause()?;
        let node = pilot.app().query_one("#probe").expect("probe node");
        pilot.app_mut().capture_mouse(Some(node));
        pilot.pause()?;
        pilot.app_mut().capture_mouse(None);
        assert_eq!(pilot.app().mouse_captured(), None);
        pilot.pause()?;
        assert_eq!(
            notices(&state),
            vec![
                ("probe".to_string(), "capture".to_string(), 0, 0),
                ("probe".to_string(), "release".to_string(), 0, 0),
            ],
            "release must notify the previous holder"
        );
        Ok(())
    })
    .expect("run_test");
}

/// Switching holders notifies release-then-capture in queue order (Python
/// posts `MouseRelease` to the old holder before `MouseCapture` to the new).
#[test]
fn switch_posts_release_then_capture_in_order() {
    let (app, state) = notice_app(&["a", "b"]);
    run_test(app, |pilot| {
        pilot.pause()?;
        let a = pilot.app().query_one("#a").expect("probe a");
        let b = pilot.app().query_one("#b").expect("probe b");
        pilot.app_mut().capture_mouse(Some(a));
        // No pause: both transitions drain together, preserving post order.
        pilot.app_mut().capture_mouse(Some(b));
        pilot.pause()?;
        assert_eq!(
            notices(&state),
            vec![
                ("a".to_string(), "capture".to_string(), 0, 0),
                ("a".to_string(), "release".to_string(), 0, 0),
                ("b".to_string(), "capture".to_string(), 0, 0),
            ],
            "switch must notify old-release before new-capture"
        );
        Ok(())
    })
    .expect("run_test");
}

/// Re-capturing the current holder is a no-op: no duplicate notices (Python
/// `capture_mouse` returns early when the holder is unchanged).
#[test]
fn recapture_same_target_is_noop() {
    let (app, state) = notice_app(&["probe"]);
    run_test(app, |pilot| {
        pilot.pause()?;
        let node = pilot.app().query_one("#probe").expect("probe node");
        pilot.app_mut().capture_mouse(Some(node));
        pilot.pause()?;
        pilot.app_mut().capture_mouse(Some(node));
        pilot.pause()?;
        assert_eq!(
            notices(&state),
            vec![("probe".to_string(), "capture".to_string(), 0, 0)],
            "re-capture must not re-notify"
        );
        Ok(())
    })
    .expect("run_test");
}

/// Notices go to the capturer only — ancestors never see them (Python
/// `bubble=False`).
#[test]
fn capture_notice_does_not_bubble_to_ancestors() {
    let state = Arc::new(Mutex::new(NoticeState::default()));
    let app = NoticeApp {
        probes: vec![MouseProbe::new("parent", "parent", state.clone(), true)],
    };
    run_test(app, |pilot| {
        pilot.pause()?;
        let child = pilot.app().query_one("#child").expect("child node");
        pilot.app_mut().capture_mouse(Some(child));
        pilot.pause()?;
        assert_eq!(
            notices(&state),
            vec![("child".to_string(), "capture".to_string(), 0, 0)],
            "only the capturer must be notified, never ancestors"
        );
        Ok(())
    })
    .expect("run_test");
}
