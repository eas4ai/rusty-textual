/// Diff viewer example: maps [`similar`](https://github.com/mitsuhiko/similar)
/// change tags onto styled text.
///
/// `similar::TextDiff` computes the ops; each `ChangeTag::{Equal, Delete,
/// Insert}` becomes a line prefixed with ` `/`-`/`+` and styled plain, red,
/// or green. Run with two files, or with no arguments to diff the embedded
/// sample:
///
/// ```text
/// cargo run --example diff -- old.txt new.txt
/// cargo run --example diff
/// ```
use rich_rs::{Style, Text};
use rusty_textual::prelude::*;
use similar::{ChangeTag, TextDiff};
use std::env;
use std::fs;

// ---------------------------------------------------------------------------
// Pure diff logic (unit-testable without the runtime)
// ---------------------------------------------------------------------------

/// One rendered diff row: the `-/+/ ` sigil plus the line text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffRow {
    sigil: char,
    text: String,
}

impl DiffRow {
    fn styled_line(&self) -> String {
        format!("{} {}", self.sigil, self.text)
    }
}

/// Line-diff `old` against `new` via `similar` (the mapping under test).
fn diff_lines(old: &str, new: &str) -> Vec<DiffRow> {
    let diff = TextDiff::from_lines(old, new);
    let mut rows = Vec::new();
    for op in diff.ops() {
        for change in diff.iter_changes(op) {
            let sigil = match change.tag() {
                ChangeTag::Delete => '-',
                ChangeTag::Insert => '+',
                ChangeTag::Equal => ' ',
            };
            rows.push(DiffRow {
                sigil,
                text: change.value().trim_end_matches('\n').to_string(),
            });
        }
    }
    rows
}

fn style_for(sigil: char) -> Option<Style> {
    match sigil {
        '-' => Style::parse("red"),
        '+' => Style::parse("green"),
        _ => None,
    }
}

/// Render rows into styled rich text for display.
fn build_text(rows: &[DiffRow]) -> Text {
    let mut text = Text::new();
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            text.append("\n", None);
        }
        text.append(row.styled_line(), style_for(row.sigil));
    }
    text
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

const OLD_SAMPLE: &str = "fn add(a: int, b: int):\n    return a + b\n\n\nprint(add(2, 3))\n";
const NEW_SAMPLE: &str =
    "fn add(a: int, b: int):\n    total = a + b\n    return total\n\n\nprint(add(2, 4))\n";

pub struct DiffApp {
    old: String,
    new: String,
}

impl DiffApp {
    #[must_use]
    pub fn new(old: String, new: String) -> Self {
        Self { old, new }
    }
}

impl Default for DiffApp {
    fn default() -> Self {
        Self::new(OLD_SAMPLE.to_string(), NEW_SAMPLE.to_string())
    }
}

impl TextualApp for DiffApp {
    fn compose(&mut self) -> AppRoot {
        AppRoot::new().with_child(Static::new("").id("diff-view"))
    }

    fn configure(&mut self, app: &mut App) -> rusty_textual::Result<()> {
        app.load_stylesheet("#diff-view { width: 100%; height: 100%; }");
        Ok(())
    }

    fn on_mount_with_app(&mut self, app: &mut App, _ctx: &mut WidgetCtx) {
        let rows = diff_lines(&self.old, &self.new);
        let text = build_text(&rows);
        let node = app.query_one("#diff-view").expect("diff view node");
        let _ = app.with_widget_mut_as::<Static, _>(node, |d| d.update_rich(text));
    }
}

fn read_or_fallback(path: &str, fallback: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|_| fallback.to_string())
}

fn main() -> rusty_textual::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let app = match args.as_slice() {
        [old_path, new_path] => DiffApp::new(
            read_or_fallback(old_path, OLD_SAMPLE),
            read_or_fallback(new_path, NEW_SAMPLE),
        ),
        _ => DiffApp::default(),
    };
    run_sync(app)
}

// ---------------------------------------------------------------------------
// Regression tests — pure logic plus a headless mount smoke test
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_text_produces_only_context() {
        let rows = diff_lines("a\nb\n", "a\nb\n");
        assert!(rows.iter().all(|r| r.sigil == ' '));
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn changed_line_becomes_delete_plus_insert() {
        let rows = diff_lines("old\n", "new\n");
        assert_eq!(
            rows,
            vec![
                DiffRow {
                    sigil: '-',
                    text: "old".to_string()
                },
                DiffRow {
                    sigil: '+',
                    text: "new".to_string()
                },
            ]
        );
    }

    #[test]
    fn insertion_keeps_context_around_it() {
        let rows = diff_lines("a\nc\n", "a\nb\nc\n");
        let sigils: String = rows.iter().map(|r| r.sigil).collect();
        assert_eq!(sigils, " + ");
    }

    #[test]
    fn styled_text_carries_sigils_and_styles() {
        let rows = diff_lines("old\n", "new\n");
        let text = build_text(&rows);
        assert_eq!(text.plain_text(), "- old\n+ new");
        assert!(style_for('-').is_some());
        assert!(style_for('+').is_some());
        assert!(style_for(' ').is_none());
    }
}

#[cfg(test)]
mod smoke {
    use super::*;

    /// Mount smoke: the app composes and the diff view node exists with the
    /// diff applied (no panic through the real mount path).
    #[test]
    fn headless_mount_renders_diff_view() {
        run_test(DiffApp::default(), |pilot| {
            pilot.pause()?;
            pilot.app().query_one("#diff-view").expect("diff view node");
            Ok(())
        })
        .expect("run_test");
    }
}
