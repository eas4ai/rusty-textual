/// Port of Python Textual `examples/merlin.py`.
///
/// The Merlin handheld game: a 3x3 grid of toggle switches. Flipping one
/// also flips its orthogonally-adjacent partners (the `TOGGLES` table). Win
/// by lighting every switch except the center one (`{1,2,3,4,6,7,8,9}`).
/// The timer counts up until you win.
///
/// Run with:
///
/// ```text
/// cargo run --example merlin
/// ```
///
/// Keys `1`–`9` flip switches; mouse and Tab/Space work through the normal
/// `Switch` focus and press behavior.
use rich_rs::{Console, ConsoleOptions, Segments};
use rusty_textual::prelude::*;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Game data (mirrors merlin.py)
// ---------------------------------------------------------------------------

/// Toggling switch `n` also toggles these (orthogonal adjacency on the 3x3
/// pad, numbered like a phone keypad in the Python original's layout).
fn toggles(switch_no: u8) -> &'static [u8] {
    match switch_no {
        1 => &[2, 4, 5],
        2 => &[1, 3],
        3 => &[2, 5, 6],
        4 => &[1, 7],
        5 => &[2, 4, 6, 8],
        6 => &[3, 9],
        7 => &[4, 5, 8],
        8 => &[7, 9],
        9 => &[5, 6, 8],
        _ => &[],
    }
}

/// Winning set: every switch on except the center (Python `check_win`).
fn is_win(on: &[u8]) -> bool {
    let mut sorted = on.to_vec();
    sorted.sort_unstable();
    sorted == [1, 2, 3, 4, 6, 7, 8, 9]
}

fn fmt_elapsed(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

const CSS: &str = r#"
Screen {
    align: center middle;
}

#timer {
    text-align: center;
    width: auto;
    margin: 1 2;
    text-style: bold;
}

Grid {
    border: thick $border;
    padding: 1 2;
    grid-size: 3 3;
    grid-gutter: 1 1;
    background: $surface;
}

LabelSwitch {
    width: auto;
    height: auto;
    content-align: center middle;
}

LabelSwitch Label {
    text-align: center;
    width: 100%;
    text-style: bold;
}
"#;

// ---------------------------------------------------------------------------
// LabelSwitch: a numbered label over a Switch (Python `LabelSwitch`)
// ---------------------------------------------------------------------------

pub struct LabelSwitch {
    switch_no: u8,
    seed: NodeSeed,
}

impl LabelSwitch {
    pub fn new(switch_no: u8) -> Self {
        Self {
            switch_no,
            seed: NodeSeed::default(),
        }
    }
}

impl Widget for LabelSwitch {
    fn style_type(&self) -> &'static str {
        "LabelSwitch"
    }

    fn render(&self, _console: &Console, _options: &ConsoleOptions) -> Segments {
        Segments::new()
    }

    fn compose(&mut self) -> ComposeResult {
        vec![
            ChildDecl::new(Box::new(Label::new(self.switch_no.to_string())))
                .with_id(&format!("label-{}", self.switch_no)),
            ChildDecl::new(Box::new(Switch::new(false)))
                .with_id(&format!("switch-{}", self.switch_no)),
        ]
    }

    fn take_node_seed(&mut self) -> NodeSeed {
        std::mem::take(&mut self.seed)
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct MerlinApp {
    start: Option<Instant>,
    running: bool,
    won: bool,
    /// Reentrancy guard for cascades. Python wraps the cascade in
    /// `with self.prevent(Switch.Changed)`; a flag is timing-proof here
    /// because watcher-flushed messages arrive after the scope would close.
    cascading: bool,
    /// Suppresses cascade logic while the opening position is dealt.
    dealing: bool,
    /// Cached `#switch-n` node ids (resolved on mount).
    ids: [Option<NodeId>; 9],
    rng: u64,
    last_shown_secs: Option<u64>,
}

impl Default for MerlinApp {
    fn default() -> Self {
        Self::new()
    }
}

impl MerlinApp {
    pub fn new() -> Self {
        // xorshift64 seeded from wall-clock (no rand dependency for example).
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15)
            .max(1);
        Self {
            start: None,
            running: true,
            won: false,
            cascading: false,
            dealing: false,
            ids: [None; 9],
            rng: seed,
            last_shown_secs: None,
        }
    }

    fn next_bit(&mut self) -> bool {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x.max(1);
        (x & 1) == 1
    }

    fn selector(n: u8) -> String {
        format!("#switch-{n}")
    }

    fn read_switch(&mut self, app: &mut App, n: u8) -> bool {
        app.with_query_one_mut_as::<Switch, _>(&Self::selector(n), |s| s.value())
            .unwrap_or(false)
    }

    fn set_switch(&mut self, app: &mut App, ctx: &mut WidgetCtx, n: u8, value: bool) {
        let _ =
            app.with_query_one_mut_as::<Switch, _>(&Self::selector(n), |s| s.set_value(value, ctx));
    }

    fn flip_switch(&mut self, app: &mut App, ctx: &mut WidgetCtx, n: u8) {
        let current = self.read_switch(app, n);
        self.set_switch(app, ctx, n, !current);
    }

    fn on_switches(&mut self, app: &mut App) -> Vec<u8> {
        (1..=9u8).filter(|&n| self.read_switch(app, n)).collect()
    }

    fn check_win(&mut self, app: &mut App) {
        if !self.won && is_win(&self.on_switches(app)) {
            self.won = true;
            self.running = false;
            app.notify(
                "You win!",
                "Congratulations",
                ToastSeverity::Information,
                None,
            );
        }
    }

    /// Refresh the timer display, at most once per elapsed second.
    fn refresh_timer(&mut self, app: &mut App) {
        if !self.running {
            return;
        }
        let Some(t) = self.start else { return };
        let secs = t.elapsed().as_secs();
        if self.last_shown_secs != Some(secs) {
            self.last_shown_secs = Some(secs);
            let text = fmt_elapsed(secs);
            let _ = app.with_query_one_mut_as::<Static, _>("#timer", |d| d.update(text));
        }
    }
}

impl TextualApp for MerlinApp {
    fn compose(&mut self) -> AppRoot {
        let root = AppRoot::new().with_child(Static::new("0:00").id("timer"));
        let mut grid = Grid::new(3, 3);
        // Python order: 7,8,9 / 4,5,6 / 1,2,3 (phone-keypad rows).
        for n in [7u8, 8, 9, 4, 5, 6, 1, 2, 3] {
            grid = grid.with_child(LabelSwitch::new(n));
        }
        root.with_child(grid)
    }

    fn configure(&mut self, app: &mut App) -> rusty_textual::Result<()> {
        app.load_stylesheet(CSS);
        Ok(())
    }

    fn on_mount_with_app(&mut self, app: &mut App, ctx: &mut WidgetCtx) {
        self.start = Some(Instant::now());
        // Cache switch node ids, then deal a random opening position with
        // cascades suppressed (Python toggles each switch on mount).
        for n in 1..=9u8 {
            self.ids[(n - 1) as usize] = app.query_one(&Self::selector(n)).ok();
        }
        self.dealing = true;
        for n in 1..=9u8 {
            if self.next_bit() {
                self.set_switch(app, ctx, n, true);
            }
        }
        self.dealing = false;
    }

    fn on_tick_with_app(&mut self, app: &mut App, _tick: u64, _ctx: &mut WidgetCtx) {
        self.refresh_timer(app);
    }

    fn on_message_with_app(&mut self, app: &mut App, message: &MessageEvent, ctx: &mut WidgetCtx) {
        if message.downcast_ref::<SwitchChanged>().is_none() {
            return;
        }
        // Identify the switch by control node (Python `event.switch.name`).
        let control = message.control.or(Some(message.sender));
        let fired = (1..=9u8).find(|&n| self.ids[(n - 1) as usize] == control);
        let Some(n) = fired else { return };
        if self.dealing || self.cascading {
            return;
        }
        // Cascade to partners, then check the win (Python `on_switch_changed`).
        self.cascading = true;
        for &m in toggles(n) {
            self.flip_switch(app, ctx, m);
        }
        self.cascading = false;
        self.check_win(app);
        self.refresh_timer(app);
    }

    fn on_key_with_app(&mut self, app: &mut App, key: &KeyEventData, ctx: &mut WidgetCtx) {
        // Python `on_key`: digit keys flip the matching switch.
        if key.key.len() == 1 {
            if let Some(ch) = key.key.chars().next() {
                if let Some(n) = ch.to_digit(10).filter(|&d| (1..=9).contains(&d)) {
                    self.flip_switch(app, ctx, n as u8);
                }
            }
        }
        let _ = ctx;
    }
}

fn main() -> rusty_textual::Result<()> {
    run_sync(MerlinApp::new())
}

#[cfg(test)]
mod smoke {
    use super::*;

    fn on_set(pilot: &mut Pilot) -> Vec<u8> {
        (1..=9u8)
            .filter(|&n| {
                pilot
                    .app_mut()
                    .with_query_one_mut_as::<Switch, _>(&format!("#switch-{n}"), |s| s.value())
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Layout: the nine switches form a 3x3 grid (phone-keypad rows),
    /// not a crushed column. Regression test — `width/height: auto` on the
    /// grid collapsed the tracks; the grid now sizes from its parent.
    #[test]
    fn headless_switches_form_three_by_three_grid() {
        fn rect(pilot: &mut Pilot, sel: &str) -> (u16, u16, u16, u16) {
            let node = pilot.app().query_one(sel).expect("switch node");
            pilot.app().layout_rect_for_test(node).expect("layout rect")
        }

        run_test_sized(MerlinApp::new(), 80, 24, |pilot| {
            pilot.pause()?;
            for row in [
                ["#switch-7", "#switch-8", "#switch-9"],
                ["#switch-4", "#switch-5", "#switch-6"],
                ["#switch-1", "#switch-2", "#switch-3"],
            ] {
                let rects: Vec<_> = row.iter().map(|sel| rect(pilot, sel)).collect();
                for r in &rects {
                    assert!(r.2 > r.0 && r.3 > r.1, "switch has zero area: {r:?}");
                    assert_eq!((r.1, r.3), (rects[0].1, rects[0].3), "row shares a y band");
                }
                for pair in rects.windows(2) {
                    assert!(pair[0].0 < pair[1].0, "row orders left-to-right");
                    assert!(pair[0].2 <= pair[1].0, "switches do not overlap");
                }
            }
            Ok(())
        })
        .expect("run_test_sized");
    }

    /// End-to-end: all nine switches compose, and flipping switch 1 through
    /// the real key path changes the on-set (its cascade partners move).
    #[test]
    fn headless_flip_cascades() {
        run_test(MerlinApp::new(), |pilot| {
            pilot.pause()?;
            for n in 1..=9u8 {
                pilot
                    .app()
                    .query_one(&format!("#switch-{n}"))
                    .expect("switch node");
            }
            let before = on_set(pilot);
            let s1_before = before.contains(&1);
            pilot.press_key("1")?;
            let after = on_set(pilot);
            assert_eq!(after.contains(&1), !s1_before, "switch 1 itself must flip");
            assert_ne!(
                before, after,
                "flipping switch 1 must move its cascade partners"
            );
            Ok(())
        })
        .expect("run_test");
    }
}

// ---------------------------------------------------------------------------
// Regression tests — pure game logic, no runtime needed
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacency_table_covers_keypad() {
        assert_eq!(toggles(5), &[2, 4, 6, 8]);
        assert_eq!(toggles(1), &[2, 4, 5]);
        assert_eq!(toggles(9), &[5, 6, 8]);
        assert!(toggles(0).is_empty());
    }

    #[test]
    fn win_set_excludes_only_center() {
        assert!(is_win(&[1, 2, 3, 4, 6, 7, 8, 9]));
        assert!(!is_win(&[1, 2, 3, 4, 5, 6, 7, 8, 9]));
        assert!(!is_win(&[1, 2, 3]));
        // Order-insensitive.
        assert!(is_win(&[9, 8, 7, 6, 4, 3, 2, 1]));
    }

    #[test]
    fn elapsed_formats_like_python_timedelta() {
        assert_eq!(fmt_elapsed(0), "0:00");
        assert_eq!(fmt_elapsed(7), "0:07");
        assert_eq!(fmt_elapsed(65), "1:05");
    }
}
