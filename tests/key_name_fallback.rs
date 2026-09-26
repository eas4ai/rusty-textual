//! P-F: `key_<name>` fallback (Python `textual._dispatch_key.dispatch_key`).
//!
//! - The focused widget's `handle_key_name` runs (canonical name first,
//!   then aliases) when no binding consumes the key.
//! - A consumed binding skips the hook entirely.
//! - Every matching alias runs and the last wins; `true` marks the key
//!   handled, suppressing the action-map fallback (here: Tab/Shift-Tab
//!   focus movement).
use rich_rs::{Console, ConsoleOptions, Segments};
use rusty_textual::prelude::*;
use std::sync::{Arc, Mutex};

/// Shared probe state: key names seen by `handle_key_name`, in order, plus
/// actions served by `execute_action`.
#[derive(Debug, Default)]
struct KeyProbeState {
    names: Vec<String>,
    actions: Vec<String>,
}

struct KeyProbe {
    seed: NodeSeed,
    state: Arc<Mutex<KeyProbeState>>,
    /// Return value of `handle_key_name` (whether the key counts handled).
    consume: bool,
    /// Optional `(key, action)` declarative binding served by `execute_action`.
    bound: Option<(String, String)>,
}

impl KeyProbe {
    fn new(
        id: &str,
        state: Arc<Mutex<KeyProbeState>>,
        consume: bool,
        bound: Option<(&str, &str)>,
    ) -> Self {
        Self {
            seed: NodeSeed {
                css_id: Some(id.to_string()),
                ..NodeSeed::default()
            },
            state,
            consume,
            bound: bound.map(|(k, a)| (k.to_string(), a.to_string())),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, KeyProbeState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Widget for KeyProbe {
    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        Segments::from(vec![rich_rs::Segment::new("keyprobe")])
    }

    fn layout_height(&self) -> Option<usize> {
        Some(1)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn take_node_seed(&mut self) -> NodeSeed {
        std::mem::take(&mut self.seed)
    }

    fn bindings(&self) -> Vec<BindingDecl> {
        self.bound
            .clone()
            .map(|(key, action)| vec![BindingDecl::new(&key, &action, "Probe binding")])
            .unwrap_or_default()
    }

    fn execute_action(&mut self, action: &ParsedAction, _ctx: &mut WidgetCtx) -> bool {
        if self.bound.as_ref().is_some_and(|(_, a)| *a == action.name) {
            self.lock().actions.push(action.name.clone());
            true
        } else {
            false
        }
    }

    fn handle_key_name(&mut self, name: &str, _ctx: &mut WidgetCtx) -> bool {
        self.lock().names.push(name.to_string());
        self.consume
    }
}

struct ProbeApp {
    probes: Vec<KeyProbe>,
}

impl TextualApp for ProbeApp {
    fn compose(&mut self) -> AppRoot {
        let mut root = AppRoot::new();
        for probe in self.probes.drain(..) {
            root = root.with_child(probe);
        }
        root
    }
}

fn one_probe(consume: bool, bound: Option<(&str, &str)>) -> (ProbeApp, Arc<Mutex<KeyProbeState>>) {
    let state = Arc::new(Mutex::new(KeyProbeState::default()));
    let app = ProbeApp {
        probes: vec![KeyProbe::new("probe", state.clone(), consume, bound)],
    };
    (app, state)
}

fn two_probes(
    consume_a: bool,
) -> (
    ProbeApp,
    Arc<Mutex<KeyProbeState>>,
    Arc<Mutex<KeyProbeState>>,
) {
    let state_a = Arc::new(Mutex::new(KeyProbeState::default()));
    let state_b = Arc::new(Mutex::new(KeyProbeState::default()));
    let app = ProbeApp {
        probes: vec![
            KeyProbe::new("probe_a", state_a.clone(), consume_a, None),
            KeyProbe::new("probe_b", state_b.clone(), false, None),
        ],
    };
    (app, state_a, state_b)
}

fn names(state: &Arc<Mutex<KeyProbeState>>) -> Vec<String> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .names
        .clone()
}

fn actions(state: &Arc<Mutex<KeyProbeState>>) -> Vec<String> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .actions
        .clone()
}

/// Unbound key reaches the focused widget's hook under its canonical name.
/// ("z" has no aliases, no binding, and no action-map entry.)
#[test]
fn key_name_hook_runs_when_no_binding_consumes() {
    let (app, state) = one_probe(false, None);
    run_test(app, |pilot| {
        // Mount auto-focuses the first focusable; `action_focus` returns
        // false when focus is already there, so only the lookup may fail.
        pilot.app_mut().action_focus("probe").expect("focus probe");
        pilot.pause()?;
        pilot.press_key("z")?;
        assert_eq!(names(&state), vec!["z"], "hook must see the canonical name");
        Ok(())
    })
    .expect("run_test");
}

/// A binding that handles its action consumes the key: the hook never runs
/// for it, but still runs for other keys.
#[test]
fn binding_consumes_key_skips_hook() {
    let (app, state) = one_probe(false, Some(("k", "probe_ping")));
    run_test(app, |pilot| {
        // Mount auto-focuses the first focusable; `action_focus` returns
        // false when focus is already there, so only the lookup may fail.
        pilot.app_mut().action_focus("probe").expect("focus probe");
        pilot.pause()?;
        pilot.press_key("k")?;
        assert_eq!(
            actions(&state),
            vec!["probe_ping"],
            "binding action must run"
        );
        assert!(
            names(&state).is_empty(),
            "consumed key must not reach the hook (got {:?})",
            names(&state)
        );
        // The default `tab -> focus_next` adapter binding also consumes its
        // key before the hook.
        pilot.press_key("tab")?;
        assert!(
            names(&state).is_empty(),
            "default-bound Tab must not reach the hook (got {:?})",
            names(&state)
        );
        pilot.press_key("z")?;
        assert_eq!(
            names(&state),
            vec!["z"],
            "unconsumed key must still reach the hook"
        );
        Ok(())
    })
    .expect("run_test");
}

/// Enter carries the `ctrl+m` alias and is bound by nothing: every matching
/// alias runs, canonical first, last result wins.
#[test]
fn duplicate_aliases_run_in_order_last_wins() {
    let (app, state_a, state_b) = two_probes(false);
    run_test(app, |pilot| {
        pilot
            .app_mut()
            .action_focus("probe_a")
            .expect("focus probe_a");
        pilot.pause()?;
        pilot.press_key("enter")?;
        assert_eq!(
            names(&state_a),
            vec!["enter", "ctrl+m"],
            "every matching alias must run, canonical first"
        );
        assert!(
            names(&state_b).is_empty(),
            "unfocused probe must see nothing (got {:?})",
            names(&state_b)
        );
        Ok(())
    })
    .expect("run_test");
}

/// Returning `true` marks the key handled and suppresses the action-map
/// fallback: with `z` mapped to `FocusNext`, focus stays — proven by the next
/// key landing on the same probe.
#[test]
fn consumed_key_suppresses_action_map_fallback() {
    let (app, state_a, state_b) = two_probes(true);
    run_test(app, |pilot| {
        use crossterm::event::{KeyCode, KeyModifiers};
        pilot.app_mut().bind_key(
            KeyBind::new(KeyCode::Char('z'), KeyModifiers::empty()),
            Action::FocusNext,
        );
        pilot
            .app_mut()
            .action_focus("probe_a")
            .expect("focus probe_a");
        pilot.pause()?;
        pilot.press_key("z")?;
        assert_eq!(names(&state_a), vec!["z"], "hook must run first");
        pilot.press_key("y")?;
        assert_eq!(
            names(&state_a),
            vec!["z", "y"],
            "consumed key must suppress focus move: y stays on probe_a"
        );
        assert!(
            names(&state_b).is_empty(),
            "probe_b must stay silent (got {:?})",
            names(&state_b)
        );
        Ok(())
    })
    .expect("run_test");
}

/// Control case: when the hook declines (`false`), the mapped fallback runs —
/// `z` moves focus and the next key lands on the other probe.
#[test]
fn declined_key_still_runs_action_map_fallback() {
    let (app, state_a, state_b) = two_probes(false);
    run_test(app, |pilot| {
        use crossterm::event::{KeyCode, KeyModifiers};
        pilot.app_mut().bind_key(
            KeyBind::new(KeyCode::Char('z'), KeyModifiers::empty()),
            Action::FocusNext,
        );
        pilot
            .app_mut()
            .action_focus("probe_a")
            .expect("focus probe_a");
        pilot.pause()?;
        pilot.press_key("z")?;
        assert_eq!(
            names(&state_a),
            vec!["z"],
            "hook still runs before the fallback"
        );
        pilot.press_key("y")?;
        assert_eq!(
            names(&state_b),
            vec!["y"],
            "declined key must run the fallback: y lands on probe_b"
        );
        Ok(())
    })
    .expect("run_test");
}
