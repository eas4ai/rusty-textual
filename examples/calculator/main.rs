/// Port of Python Textual `examples/calculator.py`.
///
/// A working desk calculator (macOS layout): click buttons or press the
/// equivalent keys. State machine mirrors the Python original
/// (`left`/`right`/`value`/`operator`); `f64` stands in for `Decimal`
/// (no decimal dependency — display rounds to 10 significant digits).
/// Both AC and C always show (the Python original swaps them via
/// `compute_show_ac`).
///
/// Run with:
///
/// ```text
/// cargo run --example calculator
/// ```
use rusty_textual::prelude::*;

// ---------------------------------------------------------------------------
// Embedded CSS (mirrors calculator.tcss from the Python Textual repo)
// ---------------------------------------------------------------------------

const CSS: &str = r#"
#calculator {
    grid-size: 4 6;
    grid-gutter: 1;
    padding: 1 2;
    width: auto;
    height: auto;
    align: center middle;
}

#numbers {
    column-span: 4;
    height: 3;
    content-align: right middle;
    text-style: bold;
}

Button {
    width: 100%;
}
"#;

// ---------------------------------------------------------------------------
// Pure calculator state (unit-testable without the runtime)
// ---------------------------------------------------------------------------

/// Which binary operator is pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Operator {
    #[default]
    Plus,
    Minus,
    Divide,
    Multiply,
}

impl Operator {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "plus" => Some(Operator::Plus),
            "minus" => Some(Operator::Minus),
            "divide" => Some(Operator::Divide),
            "multiply" => Some(Operator::Multiply),
            _ => None,
        }
    }

    fn apply(self, left: f64, right: f64) -> Option<f64> {
        match self {
            Operator::Plus => Some(left + right),
            Operator::Minus => Some(left - right),
            Operator::Divide => {
                if right == 0.0 {
                    None
                } else {
                    Some(left / right)
                }
            }
            Operator::Multiply => Some(left * right),
        }
    }
}

/// Mirrors the Python `CalculatorApp` reactive cluster:
/// `numbers` (display), `value` (current entry), `left`/`right`, `operator`.
#[derive(Debug, Default)]
struct CalcState {
    numbers: String,
    value: String,
    left: f64,
    right: f64,
    operator: Operator,
}

impl CalcState {
    fn new() -> Self {
        Self {
            numbers: "0".to_string(),
            ..Self::default()
        }
    }

    /// Format a float the way the calculator display should: drop the
    /// `.0`, keep it short.
    fn fmt(n: f64) -> String {
        if !n.is_finite() {
            return "Error".to_string();
        }
        // Round to 10 significant digits to hide f64 noise (Python's
        // Decimal is exact; this is the documented f64 trade-off).
        let rounded = format!("{:.10}", n)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
        if rounded.is_empty() || rounded == "-0" {
            "0".to_string()
        } else {
            rounded
        }
    }

    /// LEFT OPERATOR RIGHT (Python `_do_math`).
    fn do_math(&mut self) {
        match self.operator.apply(self.left, self.right) {
            Some(result) => {
                self.left = result;
                self.numbers = Self::fmt(result);
                self.value.clear();
            }
            None => {
                self.numbers = "Error".to_string();
                self.value.clear();
            }
        }
    }

    fn press_number(&mut self, digit: char) {
        // Python: `self.numbers = self.value = self.value.lstrip("0") + number`
        let stripped = self.value.trim_start_matches('0').to_string();
        self.value = format!("{stripped}{digit}");
        self.numbers = self.value.clone();
    }

    fn press_point(&mut self) {
        if !self.value.contains('.') {
            self.value = format!(
                "{}{}",
                if self.value.is_empty() {
                    "0"
                } else {
                    &self.value
                },
                "."
            );
            // Reborrow-safe rebuild (value borrowed above).
            let v = self.value.clone();
            self.numbers = v;
        }
    }

    fn press_plus_minus(&mut self) {
        let n: f64 = if self.value.is_empty() {
            0.0
        } else {
            self.value.parse().unwrap_or(0.0)
        };
        self.value = Self::fmt(-n);
        self.numbers = self.value.clone();
    }

    fn press_percent(&mut self) {
        let n: f64 = if self.value.is_empty() {
            0.0
        } else {
            self.value.parse().unwrap_or(0.0)
        };
        self.value = Self::fmt(n / 100.0);
        self.numbers = self.value.clone();
    }

    fn press_ac(&mut self) {
        self.value.clear();
        self.left = 0.0;
        self.right = 0.0;
        self.operator = Operator::Plus;
        self.numbers = "0".to_string();
    }

    fn press_c(&mut self) {
        self.value.clear();
        self.numbers = "0".to_string();
    }

    fn press_op(&mut self, op: Operator) {
        self.right = if self.value.is_empty() {
            0.0
        } else {
            self.value.parse().unwrap_or(0.0)
        };
        self.do_math();
        self.operator = op;
    }

    fn press_equals(&mut self) {
        if !self.value.is_empty() {
            self.right = self.value.parse().unwrap_or(0.0);
        }
        self.do_math();
    }

    /// Route a button id to its action (Python `@on(Button.Pressed, …)`).
    /// Returns `true` when the id was handled.
    fn press_button(&mut self, id: &str) -> bool {
        if let Some(digit) = id.strip_prefix("number-") {
            if digit.len() == 1 {
                if let Some(ch) = digit.chars().next() {
                    if ch.is_ascii_digit() {
                        self.press_number(ch);
                        return true;
                    }
                }
            }
            return false;
        }
        match id {
            "plus-minus" => self.press_plus_minus(),
            "percent" => self.press_percent(),
            "point" => self.press_point(),
            "ac" => self.press_ac(),
            "c" => self.press_c(),
            "equals" => self.press_equals(),
            other => {
                if let Some(op) = Operator::from_id(other) {
                    self.press_op(op);
                } else {
                    return false;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// Maps physical key names onto button ids (Python `NAME_MAP`).
fn button_for_key(key: &str) -> Option<&'static str> {
    match key {
        "asterisk" => Some("multiply"),
        "slash" => Some("divide"),
        "underscore" | "plus_minus_sign" => Some("plus-minus"),
        "full_stop" => Some("point"),
        "percent_sign" => Some("percent"),
        "equals_sign" => Some("equals"),
        "minus" => Some("minus"),
        "plus" => Some("plus"),
        _ => None,
    }
}

pub struct CalculatorApp {
    state: CalcState,
}

impl Default for CalculatorApp {
    fn default() -> Self {
        Self::new()
    }
}

impl CalculatorApp {
    pub fn new() -> Self {
        Self {
            state: CalcState::new(),
        }
    }

    fn button(label: &str, id: &str, variant: ButtonVariant) -> Button {
        Button::new(label).id(id).variant(variant)
    }

    /// Apply one button press and refresh the display (shared by mouse and
    /// keyboard paths — Python presses the widget; here both paths run the
    /// same state transition, which is what `Button.press()` would trigger
    /// via `ButtonPressed` anyway).
    fn handle_button(&mut self, app: &mut App, id: &str) {
        if self.state.press_button(id) {
            let text = self.state.numbers.clone();
            let _ = app.with_query_one_mut_as::<Static, _>("#numbers", |d| {
                d.update(text);
            });
        }
    }
}

impl TextualApp for CalculatorApp {
    fn compose(&mut self) -> AppRoot {
        use ButtonVariant as V;
        AppRoot::new()
            .with_child(Static::new("0").id("numbers"))
            .with_child(Self::button("AC", "ac", V::Primary))
            .with_child(Self::button("C", "c", V::Primary))
            .with_child(Self::button("+/-", "plus-minus", V::Primary))
            .with_child(Self::button("%", "percent", V::Primary))
            .with_child(Self::button("÷", "divide", V::Warning))
            .with_child(Self::button("7", "number-7", V::Default))
            .with_child(Self::button("8", "number-8", V::Default))
            .with_child(Self::button("9", "number-9", V::Default))
            .with_child(Self::button("×", "multiply", V::Warning))
            .with_child(Self::button("4", "number-4", V::Default))
            .with_child(Self::button("5", "number-5", V::Default))
            .with_child(Self::button("6", "number-6", V::Default))
            .with_child(Self::button("-", "minus", V::Warning))
            .with_child(Self::button("1", "number-1", V::Default))
            .with_child(Self::button("2", "number-2", V::Default))
            .with_child(Self::button("3", "number-3", V::Default))
            .with_child(Self::button("+", "plus", V::Warning))
            .with_child(Self::button("0", "number-0", V::Default))
            .with_child(Self::button(".", "point", V::Default))
            .with_child(Self::button("=", "equals", V::Warning))
    }

    fn configure(&mut self, app: &mut App) -> rusty_textual::Result<()> {
        app.load_stylesheet(CSS);
        Ok(())
    }

    fn on_message_with_app(
        &mut self,
        app: &mut App,
        message: &MessageEvent,
        ctx: &mut rusty_textual::event::WidgetCtx,
    ) {
        let _ = ctx;
        if let Some(pressed) = message.downcast_ref::<ButtonPressed>() {
            if let Some(id) = pressed.button_id.clone() {
                self.handle_button(app, &id);
            }
        }
    }

    fn on_key_with_app(
        &mut self,
        app: &mut App,
        key: &KeyEventData,
        ctx: &mut rusty_textual::event::WidgetCtx,
    ) {
        let _ = ctx;
        // Python `on_key`: digits press number-N; `c` clears; NAME_MAP rest.
        if key.key.len() == 1 {
            if let Some(ch) = key.key.chars().next() {
                if ch.is_ascii_digit() {
                    self.handle_button(app, &format!("number-{ch}"));
                    return;
                }
                if ch == 'c' {
                    self.handle_button(app, "c");
                    self.handle_button(app, "ac");
                    return;
                }
            }
        }
        if let Some(id) = button_for_key(&key.key) {
            self.handle_button(app, id);
        }
    }
}

fn main() -> rusty_textual::Result<()> {
    run_sync(CalculatorApp::new())
}

#[cfg(test)]
mod smoke {
    use super::*;

    fn display(pilot: &mut Pilot) -> String {
        let node = pilot.app().query_one("#numbers").expect("display node");
        pilot
            .app_mut()
            .with_widget_mut_as::<Static, _>(node, |d| d.text().to_string())
            .expect("display text")
    }

    /// End-to-end through the real key path: 9 - 3 = 6 on the display.
    /// (`+` is the press-spec modifier joiner, so subtraction exercises the
    /// same operator path headlessly; `+` works live from a real terminal.)
    #[test]
    fn headless_subtraction_updates_display() {
        run_test(CalculatorApp::new(), |pilot| {
            pilot.pause()?;
            assert_eq!(display(pilot), "0");
            for key in ["9", "-", "3", "="] {
                pilot.press_key(key)?;
            }
            assert_eq!(display(pilot), "6");
            Ok(())
        })
        .expect("run_test");
    }
}

// ---------------------------------------------------------------------------
// Regression tests — pure state machine, no runtime needed
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn entered(state: &mut CalcState, digits: &str) {
        for ch in digits.chars() {
            state.press_number(ch);
        }
    }

    #[test]
    fn chained_addition() {
        // 2 + 3 + 4 = 9 (left-associative like the Python original).
        let mut s = CalcState::new();
        entered(&mut s, "2");
        s.press_op(Operator::Plus);
        entered(&mut s, "3");
        s.press_op(Operator::Plus);
        entered(&mut s, "4");
        s.press_equals();
        assert_eq!(s.numbers, "9");
    }

    #[test]
    fn multiply_then_equals_repeats_last_op() {
        let mut s = CalcState::new();
        entered(&mut s, "6");
        s.press_op(Operator::Multiply);
        entered(&mut s, "7");
        s.press_equals();
        assert_eq!(s.numbers, "42");
    }

    #[test]
    fn divide_by_zero_errors() {
        let mut s = CalcState::new();
        entered(&mut s, "5");
        s.press_op(Operator::Divide);
        entered(&mut s, "0");
        s.press_equals();
        assert_eq!(s.numbers, "Error");
    }

    #[test]
    fn percent_and_plus_minus() {
        let mut s = CalcState::new();
        entered(&mut s, "50");
        s.press_percent();
        assert_eq!(s.numbers, "0.5");
        s.press_plus_minus();
        assert_eq!(s.numbers, "-0.5");
    }

    #[test]
    fn point_entry_guards_double_dot() {
        let mut s = CalcState::new();
        entered(&mut s, "3");
        s.press_point();
        s.press_point();
        entered(&mut s, "5");
        assert_eq!(s.numbers, "3.5");
    }

    #[test]
    fn ac_resets_everything() {
        let mut s = CalcState::new();
        entered(&mut s, "9");
        s.press_op(Operator::Plus);
        s.press_ac();
        assert_eq!(s.numbers, "0");
        assert_eq!(s.left, 0.0);
        assert_eq!(s.operator, Operator::Plus);
    }

    #[test]
    fn unknown_button_id_is_rejected() {
        let mut s = CalcState::new();
        assert!(!s.press_button("number-"));
        assert!(!s.press_button("bogus"));
        assert!(s.press_button("number-7"));
        assert_eq!(s.numbers, "7");
    }

    #[test]
    fn key_map_covers_python_name_map() {
        assert_eq!(button_for_key("asterisk"), Some("multiply"));
        assert_eq!(button_for_key("slash"), Some("divide"));
        assert_eq!(button_for_key("full_stop"), Some("point"));
        assert_eq!(button_for_key("percent_sign"), Some("percent"));
        assert_eq!(button_for_key("equals_sign"), Some("equals"));
        assert_eq!(button_for_key("tab"), None);
    }
}
