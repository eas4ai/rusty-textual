//! PR-19: App/Pilot API surface parity (Python `App` + `Pilot`).
//!
//! Covers `App::exit` (result/code/message), `App::bell`,
//! `App::open_url`, `App::export_screenshot` / `App::save_screenshot`,
//! and Pilot `pause_for` / `wait_for_animation` /
//! `wait_for_scheduled_animations` / `exit` / `mouse_down`+`mouse_up` /
//! `double_click` / `triple_click`, each grounded against
//! `textual/app.py` and `textual/pilot.py`.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use textual::prelude::*;

/// Counter app: every button press bumps `presses`.
struct CounterApp {
    presses: Arc<Mutex<usize>>,
}

impl TextualApp for CounterApp {
    fn compose(&mut self) -> AppRoot {
        AppRoot::new().with_child(Button::new("hit me").id("hit"))
    }

    fn on_button_pressed(&mut self, _description: &str, _ctx: &mut WidgetCtx) {
        if let Ok(mut presses) = self.presses.lock() {
            *presses += 1;
        }
    }
}

fn counter_app() -> (CounterApp, Arc<Mutex<usize>>) {
    let presses = Arc::new(Mutex::new(0usize));
    (
        CounterApp {
            presses: presses.clone(),
        },
        presses,
    )
}

fn press_count(presses: &Arc<Mutex<usize>>) -> usize {
    *presses.lock().unwrap_or_else(|e| e.into_inner())
}

/// Python `App.exit(result, return_code, message)` records the payload and
/// requests stop; `Pilot.exit(result)` routes through it with code 0.
#[test]
fn app_exit_carries_result_code_message() {
    let (app, _) = counter_app();
    run_test(app, |pilot| {
        pilot
            .app_mut()
            .exit(Some("done".to_string()), 3, Some("bye".to_string()));
        assert_eq!(pilot.app().return_value(), Some("done"));
        assert_eq!(pilot.app().return_code(), 3);
        assert_eq!(pilot.app().exit_message(), Some("bye"));
        assert!(
            pilot.app().headless_stop_requested(),
            "exit() must mark the stop flag Pilot tests observe"
        );

        pilot.exit(Some("pilot-done".to_string()))?;
        assert_eq!(pilot.app().return_value(), Some("pilot-done"));
        assert_eq!(pilot.app().return_code(), 0);
        Ok(())
    })
    .expect("run_test");
}

/// Python `App.bell`: headless is a silent no-op returning Ok.
#[test]
fn bell_headless_is_ok_noop() {
    let (app, _) = counter_app();
    run_test(app, |pilot| {
        pilot.app().bell()?;
        Ok(())
    })
    .expect("run_test");
}

/// Python `App.open_url`: headless records instead of launching.
#[test]
fn open_url_headless_records_url() {
    let (app, _) = counter_app();
    run_test(app, |pilot| {
        assert_eq!(pilot.app().last_opened_url(), None);
        pilot.app_mut().open_url("https://example.com/app", true)?;
        assert_eq!(
            pilot.app().last_opened_url(),
            Some("https://example.com/app")
        );
        Ok(())
    })
    .expect("run_test");
}

/// Python `App.export_screenshot`: SVG string of the current frame.
#[test]
fn export_screenshot_returns_svg_of_frame() {
    struct LabelApp;
    impl TextualApp for LabelApp {
        fn compose(&mut self) -> AppRoot {
            AppRoot::new().with_child(Label::new("screenshot-probe-xyz"))
        }
    }
    run_test(LabelApp, |pilot| {
        pilot.pause()?;
        let svg = pilot.app().export_screenshot(None)?;
        assert!(
            svg.contains("<svg"),
            "export must be SVG, got {} bytes starting {:?}",
            svg.len(),
            &svg[..svg.len().min(60)]
        );
        assert!(
            svg.contains("screenshot-probe-xyz"),
            "exported SVG must carry the frame text"
        );
        Ok(())
    })
    .expect("run_test");
}

/// Python `App.save_screenshot`: writes the file, returns the path.
#[test]
fn save_screenshot_writes_file_and_returns_path() {
    struct LabelApp;
    impl TextualApp for LabelApp {
        fn compose(&mut self) -> AppRoot {
            AppRoot::new().with_child(Label::new("save-probe"))
        }
    }
    let path = std::env::temp_dir().join(format!("pr19-shot-{}.svg", std::process::id()));
    let path_str = path.to_string_lossy().to_string();
    run_test(LabelApp, |pilot| {
        pilot.pause()?;
        let written = pilot.app().save_screenshot(Some(&path_str), Some("pr19"))?;
        assert_eq!(written, path_str);
        let body = std::fs::read_to_string(&written).expect("screenshot file");
        assert!(body.contains("<svg"), "saved file must be SVG");
        Ok(())
    })
    .expect("run_test");
    let body = std::fs::read_to_string(&path).expect("screenshot file persists");
    assert!(body.contains("save-probe"));
    std::fs::remove_file(&path).expect("cleanup screenshot");
}

/// Auto filename: title slug + epoch.
#[test]
fn save_screenshot_auto_filename() {
    struct LabelApp;
    impl TextualApp for LabelApp {
        fn compose(&mut self) -> AppRoot {
            AppRoot::new().with_child(Label::new("auto"))
        }
    }
    run_test(LabelApp, |pilot| {
        pilot.pause()?;
        let written = pilot.app().save_screenshot(None, None)?;
        assert!(
            written.ends_with(".svg"),
            "auto filename must be SVG: {written}"
        );
        assert!(
            std::fs::metadata(&written).is_ok(),
            "auto file must exist: {written}"
        );
        std::fs::remove_file(&written).expect("cleanup auto screenshot");
        Ok(())
    })
    .expect("run_test");
}

/// Python `await pilot.pause(delay)`: deterministic timers fire across the
/// delay — a 1s interval fires exactly 3 times over 3s.
#[test]
fn pause_for_fires_timers_across_delay() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct TimerApp {
        fires: Arc<AtomicUsize>,
    }
    impl TextualApp for TimerApp {
        fn compose(&mut self) -> AppRoot {
            AppRoot::new().with_child(Label::new("timers"))
        }
        fn on_mount_with_app(&mut self, app: &mut App, _ctx: &mut WidgetCtx) {
            let fires = self.fires.clone();
            app.set_interval(
                Duration::from_secs(1),
                None,
                false,
                Box::new(move |_, _| {
                    fires.fetch_add(1, Ordering::SeqCst);
                }),
            );
        }
    }
    let fires = Arc::new(AtomicUsize::new(0));
    let fires_probe = fires.clone();
    run_test(TimerApp { fires }, |pilot| {
        assert!(pilot.clock_is_manual());
        pilot.pause_for(Duration::from_secs(3))?;
        assert_eq!(
            fires_probe.load(Ordering::SeqCst),
            3,
            "1s interval over pause_for(3s) fires exactly 3 times"
        );
        Ok(())
    })
    .expect("run_test");
}

/// `wait_for_animation` on an idle app returns immediately.
#[test]
fn wait_for_animation_idle_is_ok() {
    let (app, _) = counter_app();
    run_test(app, |pilot| {
        assert!(pilot.app().animator_is_idle());
        pilot.wait_for_animation()?;
        pilot.wait_for_scheduled_animations()?;
        Ok(())
    })
    .expect("run_test");
}

/// `wait_for_animation` drains a running style animation to completion.
#[test]
fn wait_for_animation_drains_style_animation() {
    use textual::event::StyleValue;
    struct AnimApp;
    impl TextualApp for AnimApp {
        fn compose(&mut self) -> AppRoot {
            AppRoot::new().with_child(Label::new("anim").id("anim"))
        }
        fn on_mount_with_app(&mut self, app: &mut App, ctx: &mut WidgetCtx) {
            let node = app.query_one("#anim").expect("anim node");
            ctx.animate_style(
                node,
                "opacity",
                StyleValue::Float(100.0),
                StyleValue::Float(0.0),
                Duration::from_millis(200),
                AnimationEase::Linear,
            );
        }
    }
    run_test(AnimApp, |pilot| {
        assert!(
            !pilot.app().animator_is_idle(),
            "mount-enqueued animation must be running"
        );
        pilot.wait_for_animation()?;
        assert!(
            pilot.app().animator_is_idle(),
            "wait_for_animation must drain the animation"
        );
        Ok(())
    })
    .expect("run_test");
}

/// `mouse_down` + `mouse_up` pair into one click; split injection works.
#[test]
fn mouse_down_up_pair_clicks_button() {
    let (app, presses) = counter_app();
    run_test(app, |pilot| {
        pilot.pause()?;
        pilot.mouse_down("#hit")?;
        assert_eq!(press_count(&presses), 0, "press alone is not a click");
        pilot.mouse_up("#hit")?;
        assert_eq!(press_count(&presses), 1);

        // Absolute-coordinate variants share the same injectors.
        let node = pilot.app().query_one("#hit").expect("hit node");
        let (x0, y0, x1, y1) = pilot.app().node_screen_rect(node).expect("hit rect");
        let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
        pilot.mouse_down_at(cx, cy)?;
        pilot.mouse_up_at(cx, cy)?;
        assert_eq!(press_count(&presses), 2);
        Ok(())
    })
    .expect("run_test");
}

/// `double_click` / `triple_click` replay press/release cycles: two clicks
/// yield two presses (no chained click-count event yet — RFC follow-up).
#[test]
fn double_and_triple_click_replay_cycles() {
    let (app, presses) = counter_app();
    run_test(app, |pilot| {
        pilot.pause()?;
        pilot.double_click("#hit")?;
        assert_eq!(press_count(&presses), 2);
        pilot.triple_click("#hit")?;
        assert_eq!(press_count(&presses), 5);
        Ok(())
    })
    .expect("run_test");
}
