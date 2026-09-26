//! PR-11: a missing app-level `css_path` fails startup with
//! `Error::StylesheetError` (Python parity) instead of silently running
//! unstyled.

use rusty_textual::Error;
use rusty_textual::prelude::*;

struct BadCssPathApp;

impl TextualApp for BadCssPathApp {
    fn compose(&mut self) -> AppRoot {
        AppRoot::new()
    }

    fn css_path(&self) -> Option<&'static str> {
        Some("pr11-definitely-missing/missing.tcss")
    }
}

#[test]
fn missing_css_path_fails_startup_with_stylesheet_error() {
    let err = run_sync(BadCssPathApp).expect_err("missing css_path must fail startup");
    assert!(
        matches!(err, Error::StylesheetError { .. }),
        "unexpected error: {err}"
    );
}
