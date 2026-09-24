use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("terminal driver error: {0}")]
    Terminal(#[from] std::io::Error),
    #[error("text area language error: {0}")]
    TextAreaLanguage(String),
    #[error("app runtime stopped")]
    RuntimeStopped,
    /// Stylesheet path missing or unreadable (Python `StylesheetError`).
    ///
    /// Raised instead of silently rendering unstyled (PR-11): both the
    /// app-level `css_path` startup load and per-screen `css()` path loads
    /// fail the operation that requested them.
    #[error("stylesheet error in {path}: {message}")]
    StylesheetError { path: String, message: String },
    #[error("{0}")]
    Message(String),
}

pub type Result<T> = std::result::Result<T, Error>;
