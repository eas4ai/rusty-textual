//! `DateInput` headless tests: focus opens the strip, typing/keys drive the
//! date, and every commit posts `DateChanged`.
use rusty_textual::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct DateLog {
    changed: Vec<(i32, u8, u8)>,
}

struct DateApp {
    log: Arc<Mutex<DateLog>>,
}

impl TextualApp for DateApp {
    fn compose(&mut self) -> AppRoot {
        AppRoot::new()
            .with_child(DateInput::new(2024, 1, 5).id("date"))
            .with_child(Button::new("elsewhere").id("other"))
    }

    fn on_message_with_app(
        &mut self,
        _app: &mut App,
        message: &MessageEvent,
        _ctx: &mut WidgetCtx,
    ) {
        if let Some(m) = message.downcast_ref::<DateChanged>() {
            self.log
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .changed
                .push((m.year, m.month, m.day));
        }
    }
}

fn read_date(pilot: &mut Pilot, node: NodeId) -> (i32, u8, u8) {
    pilot
        .app_mut()
        .with_widget_mut_as::<DateInput, _>(node, |d| d.date())
        .expect("date value")
}

fn is_open(pilot: &mut Pilot, node: NodeId) -> bool {
    pilot
        .app_mut()
        .with_widget_mut_as::<DateInput, _>(node, |d| d.is_open())
        .expect("open flag")
}

fn logged(log: &Arc<Mutex<DateLog>>) -> Vec<(i32, u8, u8)> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .changed
        .clone()
}

/// The mount-focused widget opens its strip on first input; typing fills
/// the day segment and posts `DateChanged`.
#[test]
fn focus_opens_strip_and_typing_fills_day() {
    let log = Arc::new(Mutex::new(DateLog::default()));
    let app = DateApp { log: log.clone() };
    run_test(app, |pilot| {
        pilot.pause()?;
        let node = pilot.app().query_one("#date").expect("date node");
        // Mount auto-focus opens the strip (headless startup posts Focus).
        assert!(is_open(pilot, node), "mount focus must open the strip");
        // Day is position 2 in the default YMD order: left wraps 0 -> 2,
        // then type 2 then 8 -> day 28.
        pilot.press_key("left")?;
        pilot.press_key("2")?;
        pilot.press_key("8")?;
        assert_eq!(read_date(pilot, node).2, 28);
        assert!(
            logged(&log).contains(&(2024, 1, 28)),
            "typing must post DateChanged (got {:?})",
            logged(&log)
        );
        Ok(())
    })
    .expect("run_test");
}

/// Up steps with wrap (Jan 31 + 1 -> Jan 1); Left/Right moves segments.
#[test]
fn stepping_wraps_and_segments_move() {
    let log = Arc::new(Mutex::new(DateLog::default()));
    let app = DateApp { log: log.clone() };
    run_test(app, |pilot| {
        pilot.pause()?;
        let node = pilot.app().query_one("#date").expect("date node");
        pilot.app_mut().action_focus("date").expect("focus date");
        pilot.pause()?;
        // Left wraps year (0) -> day (2): type 31, step up -> wraps to 1.
        pilot.press_key("left")?;
        pilot.press_key("3")?;
        pilot.press_key("1")?;
        assert_eq!(read_date(pilot, node).2, 31);
        pilot.press_key("up")?;
        assert_eq!(read_date(pilot, node).2, 1, "day must wrap");
        // Move to the month segment and step it.
        pilot.press_key("left")?;
        pilot.press_key("up")?;
        assert_eq!(read_date(pilot, node).1, 2, "left must reach month");
        let _ = log;
        Ok(())
    })
    .expect("run_test");
}

/// Backspace pops one buffer digit without committing: 2, backspace, 3, 1
/// lands on day 31 (a broken backspace would commit the stale "23").
#[test]
fn backspace_pops_buffer_digit() {
    let log = Arc::new(Mutex::new(DateLog::default()));
    let app = DateApp { log: log.clone() };
    run_test(app, |pilot| {
        pilot.pause()?;
        let node = pilot.app().query_one("#date").expect("date node");
        pilot.press_key("left")?;
        pilot.press_key("2")?;
        assert_eq!(read_date(pilot, node).2, 5, "partial input commits nothing");
        pilot.press_key("backspace")?;
        pilot.press_key("3")?;
        pilot.press_key("1")?;
        assert_eq!(read_date(pilot, node).2, 31);
        let _ = log;
        Ok(())
    })
    .expect("run_test");
}

/// Refocus keeps the segment: moving to day, blurring, and refocusing must
/// still step the day (a reset to segment 0 would step the year instead).
#[test]
fn refocus_preserves_segment() {
    let log = Arc::new(Mutex::new(DateLog::default()));
    let app = DateApp { log: log.clone() };
    run_test(app, |pilot| {
        pilot.pause()?;
        let node = pilot.app().query_one("#date").expect("date node");
        pilot.press_key("left")?;
        pilot.app_mut().action_focus("other").expect("focus other");
        pilot.pause()?;
        pilot.app_mut().action_focus("date").expect("focus date");
        pilot.pause()?;
        pilot.press_key("up")?;
        assert_eq!(read_date(pilot, node), (2024, 1, 6));
        let _ = log;
        Ok(())
    })
    .expect("run_test");
}

/// Moving focus away closes the strip again (height back to 1).
#[test]
fn blur_closes_strip() {
    let log = Arc::new(Mutex::new(DateLog::default()));
    let app = DateApp { log: log.clone() };
    run_test(app, |pilot| {
        pilot.pause()?;
        let node = pilot.app().query_one("#date").expect("date node");
        pilot.app_mut().action_focus("other").expect("focus other");
        pilot.pause()?;
        assert!(!is_open(pilot, node), "strip starts closed");
        pilot.app_mut().action_focus("date").expect("focus date");
        pilot.pause()?;
        assert!(is_open(pilot, node), "explicit focus must open");
        pilot.app_mut().action_focus("other").expect("focus away");
        pilot.pause()?;
        assert!(!is_open(pilot, node), "blur must close the strip");
        Ok(())
    })
    .expect("run_test");
}
