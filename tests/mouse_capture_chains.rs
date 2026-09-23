//! P-E: explicit mouse capture + click chains (Python `App.capture_mouse`,
//! `Click.chain`).
//!
//! - Rapid clicks on one spot extend `chain` (500ms threshold).
//! - `capture_mouse(node)` retargets down/up to the captured widget
//!   regardless of pointer position; `capture_mouse(None)` releases.
use rich_rs::{Console, ConsoleOptions, Segments};
use std::sync::{Arc, Mutex};
use textual::prelude::*;

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
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
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
