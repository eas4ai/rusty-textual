//! Reminder app example: title + due date in, JSON file out.
//!
//! A tiny persistent todo list showcasing `Input`, `DateInput` (plus the
//! `DateChanged` message), `Button` / `ButtonPressed` routing, dynamic
//! mount/remove for the reminder rows, and `serde_json` persistence to
//! `reminders.json` in the working directory.
//!
//! Run with:
//!
//! ```text
//! cargo run --example reminder
//! ```
//!
//! Type a title, pick a due date in the `DateInput` (digits or the `< >`
//! spinner strip), then Enter or Add. Each row gets Done/Reopen and Delete
//! buttons; every change is saved to `reminders.json` immediately.
use rusty_textual::prelude::*;
use serde_json::{Value, json};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Embedded CSS
// ---------------------------------------------------------------------------

const CSS: &str = r#"
Screen {
    background: $panel;
}

// One dock per edge: same-edge docks overlap (Python parity), so the form
// rows live in a single docked container and stack in normal flow inside it.
#form {
    dock: top;
    margin: 0 0 1 0;
}

#title {
    margin: 0 0 1 0;
}

#due {
    margin: 0 0 1 0;
}

#add {
    width: 100%;
}

#list {
    height: 1fr;
    margin: 0 0 1 0;
}

#status {
    dock: bottom;
}
"#;

// ---------------------------------------------------------------------------
// Reminder state + JSON persistence (serde_json::Value only — no new crates)
// ---------------------------------------------------------------------------

/// Store file, relative to the working directory.
const STORE: &str = "reminders.json";

/// Default due date for a fresh compose (edit to taste).
const DEFAULT_DUE: (i32, u8, u8) = (2026, 10, 1);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Reminder {
    id: u64,
    title: String,
    year: i32,
    month: u8,
    day: u8,
    done: bool,
}

impl Reminder {
    fn due(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn row_text(&self) -> String {
        let mark = if self.done { "[x]" } else { "[ ]" };
        format!("{} {} — due {}", mark, self.title, self.due())
    }
}

fn load_store(path: &std::path::Path) -> Vec<Reminder> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    value
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            Some(Reminder {
                id: item.get("id")?.as_u64()?,
                title: item.get("title")?.as_str()?.to_string(),
                year: item.get("year")?.as_i64()? as i32,
                month: item.get("month")?.as_u64()? as u8,
                day: item.get("day")?.as_u64()? as u8,
                done: item.get("done")?.as_bool()?,
            })
        })
        .collect()
}

fn save_store(path: &std::path::Path, reminders: &[Reminder]) {
    let items: Vec<Value> = reminders
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "title": r.title,
                "year": r.year,
                "month": r.month,
                "day": r.day,
                "done": r.done,
            })
        })
        .collect();
    let text = serde_json::to_string_pretty(&Value::Array(items)).unwrap_or_default();
    let _ = std::fs::write(path, text);
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct ReminderApp {
    reminders: Vec<Reminder>,
    next_id: u64,
    due: (i32, u8, u8),
    store: PathBuf,
    /// Live row nodes under `#list`, so refresh can remove them first.
    rows: Vec<NodeId>,
}

impl ReminderApp {
    pub fn new() -> Self {
        Self::with_store(PathBuf::from(STORE))
    }

    pub fn with_store(store: PathBuf) -> Self {
        let reminders = load_store(&store);
        let next_id = reminders.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        Self {
            reminders,
            next_id,
            due: DEFAULT_DUE,
            store,
            rows: Vec::new(),
        }
    }

    fn save(&self) {
        save_store(&self.store, &self.reminders);
    }

    /// Read + clear the title input; empty titles are ignored by the caller.
    fn take_title(app: &mut App) -> String {
        app.with_query_one_mut_as::<Input, _>("#title", |input| {
            let title = input.value().trim().to_string();
            input.clear();
            title
        })
        .unwrap_or_default()
    }

    fn add_from_inputs(&mut self, app: &mut App) {
        let title = Self::take_title(app);
        if title.is_empty() {
            return;
        }
        let (year, month, day) = self.due;
        self.reminders.push(Reminder {
            id: self.next_id,
            title,
            year,
            month,
            day,
            done: false,
        });
        self.next_id += 1;
        self.save();
        self.refresh_list(app);
    }

    fn set_done(&mut self, app: &mut App, id: u64, done: bool) {
        let Some(reminder) = self.reminders.iter_mut().find(|r| r.id == id) else {
            return;
        };
        reminder.done = done;
        self.save();
        self.refresh_list(app);
    }

    fn delete(&mut self, app: &mut App, id: u64) {
        let before = self.reminders.len();
        self.reminders.retain(|r| r.id != id);
        if self.reminders.len() != before {
            self.save();
            self.refresh_list(app);
        }
    }

    fn row_widget(reminder: &Reminder) -> Horizontal {
        let toggle = if reminder.done { "Reopen" } else { "Done" };
        Horizontal::new()
            .with_child(Static::new(reminder.row_text()).without_markup())
            .with_child(Button::new(toggle).id(format!("toggle-{}", reminder.id)))
            .with_child(Button::new("Delete").id(format!("del-{}", reminder.id)))
    }

    /// Rebuild `#list` from state (insertion order, so rows and state share
    /// indexes trivially) and refresh the status line.
    fn refresh_list(&mut self, app: &mut App) {
        for row in std::mem::take(&mut self.rows) {
            let _ = app.remove_node(row);
        }
        if self.reminders.is_empty() {
            if let Ok(node) = app.mount_under("#list", Static::new("Nothing due. Enjoy the quiet."))
            {
                self.rows.push(node);
            }
        } else {
            for reminder in &self.reminders {
                if let Ok(node) = app.mount_under("#list", Self::row_widget(reminder)) {
                    self.rows.push(node);
                }
            }
        }
        let total = self.reminders.len();
        let done = self.reminders.iter().filter(|r| r.done).count();
        let status = if total == 0 {
            "No reminders yet — add one above.".to_string()
        } else {
            format!("{total} reminder(s) · {done} done")
        };
        let _ = app.with_query_one_mut_as::<Static, _>("#status", |s| {
            s.update(status);
        });
    }
}

impl Default for ReminderApp {
    fn default() -> Self {
        Self::new()
    }
}

impl TextualApp for ReminderApp {
    fn compose(&mut self) -> AppRoot {
        let (year, month, day) = DEFAULT_DUE;
        AppRoot::new()
            .with_child(
                Vertical::new()
                    .with_child(
                        Input::new()
                            .with_placeholder("What needs doing?")
                            .id("title"),
                    )
                    .with_child(DateInput::new(year, month, day).id("due"))
                    .with_child(Button::new("Add").id("add").variant(ButtonVariant::Primary))
                    .id("form"),
            )
            .with_child(Vertical::new().id("list"))
            .with_child(Static::new("").id("status"))
    }

    fn configure(&mut self, app: &mut App) -> rusty_textual::Result<()> {
        app.load_stylesheet(CSS);
        Ok(())
    }

    fn on_mount_with_app(&mut self, app: &mut App, _ctx: &mut rusty_textual::event::WidgetCtx) {
        // Rows come from the JSON store, so the list populates post-mount.
        self.refresh_list(app);
    }

    fn on_message_with_app(
        &mut self,
        app: &mut App,
        message: &MessageEvent,
        ctx: &mut rusty_textual::event::WidgetCtx,
    ) {
        let _ = ctx;
        if let Some(submitted) = message.downcast_ref::<InputSubmitted>() {
            let _ = submitted;
            self.add_from_inputs(app);
            return;
        }
        if let Some(changed) = message.downcast_ref::<DateChanged>() {
            self.due = (changed.year, changed.month, changed.day);
            return;
        }
        if let Some(pressed) = message.downcast_ref::<ButtonPressed>() {
            match pressed.button_id.as_deref() {
                Some("add") => self.add_from_inputs(app),
                Some(id) if id.starts_with("toggle-") => {
                    if let Ok(n) = id["toggle-".len()..].parse::<u64>() {
                        let done = self
                            .reminders
                            .iter()
                            .find(|r| r.id == n)
                            .is_some_and(|r| !r.done);
                        self.set_done(app, n, done);
                    }
                }
                Some(id) if id.starts_with("del-") => {
                    if let Ok(n) = id["del-".len()..].parse::<u64>() {
                        self.delete(app, n);
                    }
                }
                _ => {}
            }
        }
    }
}

fn main() -> rusty_textual::Result<()> {
    run_sync(ReminderApp::new())
}

// ---------------------------------------------------------------------------
// Smoke tests — headless runs through the real paths (example-local store)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod smoke {
    use super::*;

    fn test_store(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "reminder-example-{}-{}.json",
            name,
            std::process::id()
        ))
    }

    fn status(pilot: &mut Pilot) -> String {
        let node = pilot.app().query_one("#status").expect("status node");
        pilot
            .app_mut()
            .with_widget_mut_as::<Static, _>(node, |d| d.text().to_string())
            .expect("status text")
    }

    fn type_text(pilot: &mut Pilot, text: &str) {
        for ch in text.chars() {
            pilot.press_key(&ch.to_string()).expect("press key");
        }
    }

    /// JSON round-trips without the runtime: save, load, compare.
    #[test]
    fn store_round_trip() {
        let path = test_store("roundtrip");
        let _ = std::fs::remove_file(&path);
        let reminders = vec![
            Reminder {
                id: 1,
                title: "Milk".to_string(),
                year: 2026,
                month: 10,
                day: 1,
                done: false,
            },
            Reminder {
                id: 2,
                title: "Rent".to_string(),
                year: 2026,
                month: 11,
                day: 1,
                done: true,
            },
        ];
        save_store(&path, &reminders);
        assert_eq!(load_store(&path), reminders);
        let _ = std::fs::remove_file(&path);
    }

    /// Corrupt or missing store starts empty instead of failing.
    #[test]
    fn bad_store_starts_empty() {
        let path = test_store("bad");
        let _ = std::fs::remove_file(&path);
        assert!(load_store(&path).is_empty());
        std::fs::write(&path, "{not json").expect("write corrupt store");
        assert!(load_store(&path).is_empty());
        let _ = std::fs::remove_file(&path);
    }

    /// End-to-end: type a title, Enter adds it, the status line and the
    /// JSON file agree.
    #[test]
    fn headless_add_persists() {
        let path = test_store("add");
        let _ = std::fs::remove_file(&path);
        run_test(ReminderApp::with_store(path.clone()), |pilot| {
            pilot.pause()?;
            assert!(status(pilot).contains("No reminders"));
            type_text(pilot, "Milk");
            pilot.press_key("enter")?;
            pilot.pause()?;
            assert!(
                status(pilot).contains("1 reminder(s)"),
                "status was {:?}",
                status(pilot)
            );
            Ok(())
        })
        .expect("run_test");
        let stored = load_store(&path);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].title, "Milk");
        assert_eq!(
            (stored[0].year, stored[0].month, stored[0].day),
            DEFAULT_DUE
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Layout: the form rows stack top-to-bottom with no overlap. Regression
    /// test — three `dock: top` siblings used to paint over each other
    /// (same-edge docks overlap by design), so the rows now live in one
    /// docked `#form` container.
    #[test]
    fn headless_form_rows_do_not_overlap() {
        fn overlaps(a: (u16, u16, u16, u16), b: (u16, u16, u16, u16)) -> bool {
            a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
        }

        let path = test_store("layout");
        let _ = std::fs::remove_file(&path);
        run_test_sized(ReminderApp::with_store(path.clone()), 80, 24, |pilot| {
            pilot.pause()?;
            let rect = |sel: &str| {
                let node = pilot.app().query_one(sel).expect("form node");
                pilot.app().layout_rect_for_test(node).expect("layout rect")
            };
            let title = rect("#title");
            let due = rect("#due");
            let add = rect("#add");
            for (name, r) in [("title", title), ("due", due), ("add", add)] {
                assert!(r.2 > r.0 && r.3 > r.1, "{name} has zero area: {r:?}");
            }
            assert!(
                title.3 <= due.1 && due.3 <= add.1,
                "form rows out of order: title={title:?} due={due:?} add={add:?}"
            );
            assert!(
                !overlaps(title, due) && !overlaps(due, add) && !overlaps(title, add),
                "form rows overlap: title={title:?} due={due:?} add={add:?}"
            );
            Ok(())
        })
        .expect("run_test_sized");
        let _ = std::fs::remove_file(&path);
    }

    /// Toggle and delete drive through real button clicks.
    #[test]
    fn headless_toggle_and_delete() {
        let path = test_store("toggle");
        let _ = std::fs::remove_file(&path);
        run_test(ReminderApp::with_store(path.clone()), |pilot| {
            pilot.pause()?;
            type_text(pilot, "Milk");
            pilot.press_key("enter")?;
            pilot.pause()?;
            type_text(pilot, "Bread");
            pilot.press_key("enter")?;
            pilot.pause()?;
            assert!(status(pilot).contains("2 reminder(s)"));
            // Buttons have no painted region in headless mode, so drive
            // them by explicit focus + Enter instead of clicks.
            pilot
                .app_mut()
                .action_focus("toggle-1")
                .expect("focus toggle");
            pilot.pause()?;
            pilot.press_key("enter")?;
            pilot.pause()?;
            assert!(
                status(pilot).contains("2 reminder(s) · 1 done"),
                "status was {:?}",
                status(pilot)
            );
            pilot.app_mut().action_focus("del-2").expect("focus delete");
            pilot.pause()?;
            pilot.press_key("enter")?;
            pilot.pause()?;
            assert!(
                status(pilot).contains("1 reminder(s) · 1 done"),
                "status was {:?}",
                status(pilot)
            );
            Ok(())
        })
        .expect("run_test");
        let stored = load_store(&path);
        assert_eq!(stored.len(), 1);
        assert!(stored[0].done);
        let _ = std::fs::remove_file(&path);
    }
}
