//! A pseudo terminal for PTY tests: runs a binary under `sh`, feeds its
//! output to a vt100 parser, and answers the queries a terminal answers.
//!
//! Included by path (`#[path = "support/pty.rs"] mod pty;`) so a test binary
//! compiles only this file. The harness answers, as configured by
//! [`Answers`]: cursor position reports (`CSI 6 n`), DECRQM mode queries for
//! synchronized output (2026, reported supported) and in-band resize (2048,
//! not recognized), and primary device attributes (`CSI c`). Replies go out
//! in query order, optionally after a delay.

// Shared by several PTY test binaries; each uses a subset of the helpers.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub const COLS: u16 = 100;
pub const ROWS: u16 = 30;
const POLL: Duration = Duration::from_millis(50);
const TIMEOUT: Duration = Duration::from_secs(20);
const SETTLE_POLLS: usize = 6;

/// Prints two shell lines, then replaces the shell with the binary.
pub const SHELL_THEN_EXEC: &str = "printf 'shell-1\\nshell-2\\n'; exec \"$0\"";
/// Prints two shell lines, runs the binary, then prints a marker and the
/// terminal's canonical-input and echo flags.
pub const SHELL_AROUND: &str = "printf 'shell-1\\nshell-2\\n'; \"$0\"; printf 'after-exit\\n'; \
     stty -a | tr ' ;' '\\n\\n' | grep -E '^-?(icanon|echo)$' | tr '\\n' ' '; printf '\\n'";

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Executables a cargo build produced, by target name.
pub type Built = Result<HashMap<String, PathBuf>, String>;

/// Run `cargo build <args>` once per test binary run and return the
/// executable cargo reports for `target`. The paths come from cargo's own
/// artifact messages, so a `CARGO_TARGET_DIR` in the environment can never
/// leave a stale binary at a guessed path in play.
pub fn built(cell: &'static OnceLock<Built>, args: &[&str], target: &str) -> PathBuf {
    let result = cell.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let output = Command::new(cargo)
            .args(args)
            .arg("--message-format=json-render-diagnostics")
            .current_dir(repo_root())
            .output()
            .map_err(|e| format!("spawn cargo: {e}"))?;
        if !output.status.success() {
            let log = String::from_utf8_lossy(&output.stderr);
            let tail: Vec<&str> = log.lines().rev().take(20).collect();
            return Err(tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        }
        let mut executables = HashMap::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if let (Some(name), Some(path)) = (
                message["target"]["name"].as_str(),
                message["executable"].as_str(),
            ) {
                executables.insert(name.to_string(), PathBuf::from(path));
            }
        }
        Ok(executables)
    });
    match result {
        Ok(executables) => executables
            .get(target)
            .unwrap_or_else(|| panic!("cargo {} built no executable {target}", args.join(" ")))
            .clone(),
        Err(log) => panic!("cargo {} failed:\n{log}", args.join(" ")),
    }
}

/// The root package's calculator example.
pub fn calculator() -> PathBuf {
    static CELL: OnceLock<Built> = OnceLock::new();
    built(&CELL, &["build", "--example", "calculator"], "calculator")
}

/// The probe app in `tests/fixtures/inline_probe`, a standalone package built
/// into its own target directory.
pub fn probe() -> PathBuf {
    static CELL: OnceLock<Built> = OnceLock::new();
    let target_dir = repo_root().join("target/inline-probe");
    let target_arg = target_dir.display().to_string();
    let manifest = "tests/fixtures/inline_probe/Cargo.toml";
    built(
        &CELL,
        &[
            "build",
            "--manifest-path",
            manifest,
            "--target-dir",
            &target_arg,
        ],
        "inline_probe",
    )
}

/// Which terminal queries the harness answers, and after what delay.
#[derive(Clone, Copy)]
pub struct Answers {
    pub cursor_position: bool,
    pub modes: bool,
    pub device_attributes: bool,
    /// Delay before answering the mode and device attributes queries.
    pub delay: Duration,
    /// Delay before answering a cursor position query.
    pub cursor_delay: Duration,
}

impl Answers {
    /// Answer everything at once, like a modern terminal.
    pub const TERMINAL: Self = Self {
        cursor_position: true,
        modes: true,
        device_attributes: true,
        delay: Duration::ZERO,
        cursor_delay: Duration::ZERO,
    };
    /// Answer nothing, like a bare vt100 parser.
    pub const NONE: Self = Self {
        cursor_position: false,
        modes: false,
        device_attributes: false,
        delay: Duration::ZERO,
        cursor_delay: Duration::ZERO,
    };
}

#[derive(Clone, Copy)]
enum Query {
    CursorPosition,
    SyncMode,
    InBandResizeMode,
    DeviceAttributes,
}

const QUERIES: [(&[u8], Query); 4] = [
    (b"\x1b[6n", Query::CursorPosition),
    (b"\x1b[?2026$p", Query::SyncMode),
    (b"\x1b[?2048$p", Query::InBandResizeMode),
    (b"\x1b[c", Query::DeviceAttributes),
];

/// The earliest complete query in `bytes`: (end offset, query).
fn find_query(bytes: &[u8]) -> Option<(usize, Query)> {
    QUERIES
        .iter()
        .filter_map(|(pattern, query)| {
            bytes
                .windows(pattern.len())
                .position(|w| w == *pattern)
                .map(|at| (at + pattern.len(), *query))
        })
        .min_by_key(|(end, _)| *end)
}

/// Length of the longest suffix of `bytes` that could start a query.
fn partial_query_len(bytes: &[u8]) -> usize {
    QUERIES
        .iter()
        .flat_map(|(pattern, _)| (1..pattern.len()).map(move |n| (pattern, n)))
        .filter(|(pattern, n)| bytes.ends_with(&pattern[..*n]))
        .map(|(_, n)| n)
        .max()
        .unwrap_or(0)
}

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// The reply to `query`, or `None` when `answers` leaves it unanswered.
fn reply(query: Query, answers: Answers, parser: &vt100::Parser) -> Option<Vec<u8>> {
    match query {
        Query::CursorPosition if answers.cursor_position => {
            let (row, col) = parser.screen().cursor_position();
            Some(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes())
        }
        Query::SyncMode if answers.modes => Some(b"\x1b[?2026;2$y".to_vec()),
        Query::InBandResizeMode if answers.modes => Some(b"\x1b[?2048;0$y".to_vec()),
        Query::DeviceAttributes if answers.device_attributes => Some(b"\x1b[?62;22c".to_vec()),
        _ => None,
    }
}

/// Feed pty output to the parser and queue replies to terminal queries.
fn pump(
    mut reader: Box<dyn Read + Send>,
    parser: &Mutex<vt100::Parser>,
    raw: &Mutex<Vec<u8>>,
    answers: Answers,
    replies: &Sender<(Instant, Vec<u8>)>,
) {
    let mut buf = [0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        raw.lock().unwrap().extend_from_slice(&buf[..n]);
        pending.extend_from_slice(&buf[..n]);
        while let Some((end, query)) = find_query(&pending) {
            let mut parser = parser.lock().unwrap();
            parser.process(&pending[..end]);
            if let Some(bytes) = reply(query, answers, &parser) {
                let delay = if matches!(query, Query::CursorPosition) {
                    answers.cursor_delay
                } else {
                    answers.delay
                };
                let _ = replies.send((Instant::now() + delay, bytes));
            }
            drop(parser);
            pending.drain(..end);
        }
        let upto = pending.len() - partial_query_len(&pending);
        parser.lock().unwrap().process(&pending[..upto]);
        pending.drain(..upto);
    }
}

/// A binary running in a pseudo terminal, seen through a vt100 parser.
pub struct Term {
    parser: Arc<Mutex<vt100::Parser>>,
    raw: Arc<Mutex<Vec<u8>>>,
    writer: SharedWriter,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    reader: Option<JoinHandle<()>>,
}

impl Term {
    /// Run `/bin/sh -c script bin` (the script runs the binary as `$0`) with
    /// `env` in a fresh terminal that answers queries as `answers` says. The
    /// shell is `/bin/sh`, not the first `sh` on `PATH`, so the mechanisms'
    /// identity can name it.
    pub fn spawn(script: &str, bin: &Path, env: &[(&str, &str)], answers: Answers) -> Self {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", script]);
        cmd.arg(bin);
        cmd.cwd(repo_root());
        // Nothing from the test's own environment (NO_COLOR, TEXTUAL_*, ...)
        // reaches the program; it sees only what is set here.
        cmd.env_clear();
        for key in ["PATH", "HOME"] {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("LANG", "en_US.UTF-8");
        cmd.env("TEXTUAL_KEYBOARD_PROTOCOL", "off");
        for (key, value) in env {
            cmd.env(key, value);
        }
        let child = pty.slave.spawn_command(cmd).expect("spawn in pty");
        drop(pty.slave);
        let reader = pty.master.try_clone_reader().expect("pty reader");
        let writer: SharedWriter =
            Arc::new(Mutex::new(pty.master.take_writer().expect("pty writer")));
        let parser = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        let raw = Arc::new(Mutex::new(Vec::new()));
        let (replies, due) = channel::<(Instant, Vec<u8>)>();
        let reply_writer = Arc::clone(&writer);
        std::thread::spawn(move || {
            for (at, bytes) in due {
                std::thread::sleep(at.saturating_duration_since(Instant::now()));
                let mut w = reply_writer.lock().unwrap();
                let _ = w.write_all(&bytes);
                let _ = w.flush();
            }
        });
        let thread = {
            let (parser, raw) = (Arc::clone(&parser), Arc::clone(&raw));
            std::thread::spawn(move || pump(reader, &parser, &raw, answers, &replies))
        };
        Self {
            parser,
            raw,
            writer,
            master: pty.master,
            child,
            reader: Some(thread),
        }
    }

    pub fn screen(&self) -> vt100::Screen {
        self.parser.lock().unwrap().screen().clone()
    }

    /// Everything the program has written so far.
    pub fn raw(&self) -> Vec<u8> {
        self.raw.lock().unwrap().clone()
    }

    /// The last bytes the program wrote, escaped, for failure messages.
    pub fn raw_tail(&self) -> String {
        let raw = self.raw();
        let tail = &raw[raw.len().saturating_sub(400)..];
        String::from_utf8_lossy(tail).escape_debug().to_string()
    }

    pub fn send(&self, bytes: &[u8]) {
        let mut w = self.writer.lock().unwrap();
        w.write_all(bytes).expect("write to pty");
        w.flush().expect("flush pty");
    }

    /// Poll until `pred` holds for the screen; panic with the screen and the
    /// last output on timeout.
    pub fn wait_for(&self, what: &str, pred: impl Fn(&vt100::Screen) -> bool) -> vt100::Screen {
        let start = Instant::now();
        loop {
            let screen = self.screen();
            if pred(&screen) {
                return screen;
            }
            assert!(
                start.elapsed() < TIMEOUT,
                "timed out waiting for {what}; screen:\n{}\nlast output: {}",
                dump(&screen),
                self.raw_tail()
            );
            std::thread::sleep(POLL);
        }
    }

    /// Wait until the screen text has not changed for a few polls.
    pub fn settle(&self) -> vt100::Screen {
        let start = Instant::now();
        let mut last = String::new();
        let mut same = 0;
        loop {
            std::thread::sleep(POLL);
            let screen = self.screen();
            let now = dump(&screen);
            same = if now == last { same + 1 } else { 0 };
            if same >= SETTLE_POLLS {
                return screen;
            }
            assert!(start.elapsed() < TIMEOUT, "screen never settled:\n{now}");
            last = now;
        }
    }

    pub fn resize(&self, rows: u16) {
        self.parser.lock().unwrap().set_size(rows, COLS);
        self.master
            .resize(PtySize {
                rows,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("resize pty");
    }

    pub fn pid(&self) -> u32 {
        self.child.process_id().expect("child pid")
    }

    /// Wait for the shell to exit and all output to be read.
    pub fn finish(mut self) -> vt100::Screen {
        let start = Instant::now();
        while self.child.try_wait().expect("try_wait").is_none() {
            assert!(
                start.elapsed() < TIMEOUT,
                "process did not exit; screen:\n{}\nlast output: {}",
                dump(&self.screen()),
                self.raw_tail()
            );
            std::thread::sleep(POLL);
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.screen()
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The screen as text, one trimmed line per row.
pub fn lines(screen: &vt100::Screen) -> Vec<String> {
    let (rows, cols) = screen.size();
    (0..rows)
        .map(|row| {
            let mut line = String::new();
            for col in 0..cols {
                if let Some(cell) = screen.cell(row, col) {
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let text = cell.contents();
                    line.push_str(if text.is_empty() { " " } else { &text });
                }
            }
            line.trim_end().to_string()
        })
        .collect()
}

pub fn dump(screen: &vt100::Screen) -> String {
    lines(screen)
        .iter()
        .enumerate()
        .map(|(row, line)| format!("{row:2}|{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rows with at least one cell whose background is not the terminal
/// default: the rows the app paints.
pub fn painted_rows(screen: &vt100::Screen) -> Vec<u16> {
    let (rows, cols) = screen.size();
    (0..rows)
        .filter(|&row| {
            (0..cols).any(|col| {
                screen
                    .cell(row, col)
                    .is_some_and(|cell| cell.bgcolor() != vt100::Color::Default)
            })
        })
        .collect()
}

pub fn row_of(screen: &vt100::Screen, text: &str) -> Option<usize> {
    lines(screen).iter().position(|line| line.contains(text))
}

pub fn has_text(text: &'static str) -> impl Fn(&vt100::Screen) -> bool {
    move |screen| row_of(screen, text).is_some()
}

pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
