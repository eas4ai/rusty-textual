//! Terminal mode negotiation core (PR-15b).
//!
//! Pure-`std` by design: the live proof probe (`probe/neg_probe.rs`,
//! compiled with plain `rustc` on each target machine) `include!`s this
//! file, so the bytes the probe sends and parses are byte-identical to the
//! driver's — no reimplementation drift between proof and product.
//!
//! Why a synchronous round-trip at startup instead of async parsing:
//! crossterm 0.28 has no DECRQM (`?$y`) arm, so a query reply reaching its
//! input parser returns `Ok(None)` and pollutes the *next* parse. The driver
//! therefore performs exactly one bounded round-trip per mode inside
//! `start()` — before the input loop owns stdin — and never enables a mode
//! whose reports it cannot consume (in-band resize reports are `CSI-u` with
//! code 48, which crossterm would misread as key events).

use std::io::Read;
use std::time::Duration;

/// Synchronized-output mode (DECSET 2026).
pub const SYNC_MODE: u16 = 2026;
/// In-band window-resize reports (DECSET 2048).
pub const IN_BAND_RESIZE_MODE: u16 = 2048;

/// Kitty progressive-enhancement flags we push (Python parity):
/// `DISAMBIGUATE (1) | REPORT_ALL_KEYS (8) | REPORT_ASSOCIATED_TEXT (16)`.
pub const KITTY_FLAGS: u16 = 0b0001_1001;

/// Upper bound for one query reply.
///
/// Automatic terminal replies are immediate; the budget covers remote links.
/// Terminals that never answer cost exactly this, once, at startup.
pub const QUERY_TIMEOUT: Duration = Duration::from_millis(100);

/// Outcome of startup negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NegotiatedModes {
    /// A DECRQM reply for 2026 reported support (`Ps != 0`).
    pub sync_supported: bool,
    /// A DECRQM reply for 2048 reported support. Recorded only: enabling
    /// in-band reports is unsafe while input flows through crossterm (see
    /// module docs), so the driver queries but never enables.
    pub in_band_resize_supported: bool,
}

/// DECRQM query bytes for `mode`: `CSI ? <mode> $ p`.
pub fn decrqm_query(mode: u16) -> Vec<u8> {
    format!("\x1b[?{mode}$p").into_bytes()
}

/// Parse a DECRQM reply (`CSI ? <mode> ; <Ps> $ y`) for `mode`.
///
/// Returns `Some(supported)` with `supported = Ps != 0` (standard DECRQM:
/// 0 = not recognized, 1 = set, 2 = reset, 3/4 = permanently set/reset), or
/// `None` when the bytes carry no recognizable reply for `mode` — including
/// Apple Terminal's stray `p`, which is why Python special-cases it.
pub fn parse_decrqm_reply(bytes: &[u8], mode: u16) -> Option<bool> {
    let text = std::str::from_utf8(bytes).ok()?;
    let marker = format!("?{mode};");
    let at = text.find(&marker)?;
    let ps: String = text[at + marker.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let ps: u16 = ps.parse().ok()?;
    let rest = &text[at + marker.len() + ps.to_string().len()..];
    if !rest.starts_with("$y") && !rest.starts_with(" $y") {
        return None;
    }
    Some(ps != 0)
}

/// Whether the SYNC query may be sent (Python parity + hardening).
///
/// Python sends unless `TERM_PROGRAM == "Apple_Terminal"` (which answers a
/// stray `p` and doesn't support SYNC anyway); we additionally require a tty
/// stdin so piped runs never emit queries into a file.
pub fn sync_query_allowed(is_stdin_tty: bool, term_program: &str) -> bool {
    is_stdin_tty && term_program != "Apple_Terminal"
}

/// Whether the in-band-resize query may be sent.
///
/// Python sends unconditionally; we require a tty for the same piped-output
/// reason (deliberate, documented deviation).
pub fn in_band_resize_query_allowed(is_stdin_tty: bool) -> bool {
    is_stdin_tty
}

/// Python `_get_environ_bool` exact semantics: only `"1"` disables.
pub fn kitty_disabled_by_env(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Resolve kitty support from explicit inputs (race-free core; the driver
/// reads the env/tty and forwards here).
///
/// An explicit `TEXTUAL_DISABLE_KITTY_KEY=1` wins over everything, including
/// forced `On` (the operator opt-out beats the API request, as in Python
/// where it is the only switch). `Auto` additionally requires a tty stdin
/// so piped runs never emit enhancement sequences into a file.
pub fn resolve_kitty_support(
    force: Option<bool>,
    disabled_by_env: bool,
    stdin_is_tty: bool,
) -> bool {
    if disabled_by_env {
        return false;
    }
    match force {
        Some(on) => on,
        None => stdin_is_tty,
    }
}

/// Run one synchronous query round-trip: `write(query)` then wait for reply
/// bytes up to [`QUERY_TIMEOUT`].
///
/// The reply reader runs on a helper thread so the wait is bounded; a leaked
/// thread on timeout exits at the next stdin byte/EOF. Call only before the
/// input loop owns stdin.
pub fn query_round_trip(
    query: &[u8],
    write: impl FnOnce(&[u8]) -> std::io::Result<()>,
    read_stdin: impl FnOnce() -> Option<Vec<u8>> + Send + 'static,
) -> Option<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read_stdin());
    });
    write(query).ok()?;
    rx.recv_timeout(QUERY_TIMEOUT).ok()?
}

/// Blocking stdin read for a DECRQM reply: bytes up to and including the
/// first `y` (DECRQM terminator), capped, `None` on EOF/empty.
///
/// Single-byte locked reads, deliberately unbuffered: a `BufReader` could
/// over-read past the reply and steal input bytes from the loop that owns
/// stdin next.
pub fn read_decrqm_reply() -> Option<Vec<u8>> {
    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    let mut buf = Vec::with_capacity(32);
    let mut one = [0u8; 1];
    loop {
        match locked.read(&mut one) {
            Ok(0) => break,
            Ok(_) => {
                buf.push(one[0]);
                if one[0] == b'y' || buf.len() >= 64 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    if buf.is_empty() {
        None
    } else {
        Some(buf)
    }
}

/// Production negotiation against the real terminal (PR-15b).
///
/// Reads `TERM_PROGRAM`/tty state from the environment and performs one
/// bounded round-trip per mode. Call only from driver `start()`, before the
/// input loop owns stdin.
pub fn negotiate_live() -> NegotiatedModes {
    use std::io::IsTerminal;
    use std::io::Write;
    let is_tty = std::io::stdin().is_terminal();
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let mut stdout = std::io::stdout();
    negotiate_with(is_tty, &term_program, |mode| {
        query_round_trip(
            &decrqm_query(mode),
            |q| stdout.write_all(q).and_then(|_| stdout.flush()),
            read_decrqm_reply,
        )
    })
}

/// Negotiate with an injected transport (unit tests + live probe share this).
///
/// `transact(mode)` sends the DECRQM query for `mode` and returns the raw
/// reply bytes, or `None` on timeout/skip.
pub fn negotiate_with(
    is_stdin_tty: bool,
    term_program: &str,
    mut transact: impl FnMut(u16) -> Option<Vec<u8>>,
) -> NegotiatedModes {
    let mut out = NegotiatedModes::default();
    if sync_query_allowed(is_stdin_tty, term_program) {
        if let Some(reply) = transact(SYNC_MODE) {
            out.sync_supported = parse_decrqm_reply(&reply, SYNC_MODE).unwrap_or(false);
        }
    }
    if in_band_resize_query_allowed(is_stdin_tty) {
        if let Some(reply) = transact(IN_BAND_RESIZE_MODE) {
            out.in_band_resize_supported =
                parse_decrqm_reply(&reply, IN_BAND_RESIZE_MODE).unwrap_or(false);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_flags_match_python_25() {
        assert_eq!(KITTY_FLAGS, 1 | 8 | 16);
        // Exact bytes the platform drivers push (`CSI > 25 u`).
        assert_eq!(format!("\x1b[>{KITTY_FLAGS}u"), "\x1b[>25u");
    }

    #[test]
    fn decrqm_query_bytes() {
        assert_eq!(decrqm_query(2026), b"\x1b[?2026$p");
        assert_eq!(decrqm_query(2048), b"\x1b[?2048$p");
    }

    #[test]
    fn parses_supported_replies() {
        assert_eq!(parse_decrqm_reply(b"\x1b[?2026;1$y", 2026), Some(true));
        assert_eq!(parse_decrqm_reply(b"\x1b[?2026;2$y", 2026), Some(true));
        assert_eq!(parse_decrqm_reply(b"\x1b[?2048;1$y", 2048), Some(true));
    }

    #[test]
    fn parses_unsupported_and_garbage() {
        assert_eq!(parse_decrqm_reply(b"\x1b[?2026;0$y", 2026), Some(false));
        // Apple Terminal's stray `p`: no recognizable reply.
        assert_eq!(parse_decrqm_reply(b"p", 2026), None);
        assert_eq!(parse_decrqm_reply(b"", 2026), None);
        // Wrong mode number is not our reply.
        assert_eq!(parse_decrqm_reply(b"\x1b[?2048;1$y", 2026), None);
        // Truncated / malformed.
        assert_eq!(parse_decrqm_reply(b"\x1b[?2026;$y", 2026), None);
        assert_eq!(parse_decrqm_reply(b"\x1b[?2026;1$x", 2026), None);
    }

    #[test]
    fn sync_gate_excludes_apple_and_pipes() {
        assert!(sync_query_allowed(true, "iTerm.app"));
        assert!(sync_query_allowed(true, ""));
        assert!(!sync_query_allowed(true, "Apple_Terminal"));
        assert!(!sync_query_allowed(false, "iTerm.app"));
        assert!(in_band_resize_query_allowed(true));
        assert!(!in_band_resize_query_allowed(false));
    }

    #[test]
    fn kitty_env_gate_is_exact() {
        assert!(kitty_disabled_by_env(Some("1")));
        assert!(!kitty_disabled_by_env(Some("true")));
        assert!(!kitty_disabled_by_env(Some("0")));
        assert!(!kitty_disabled_by_env(None));
        assert!(resolve_kitty_support(None, false, true));
        assert!(!resolve_kitty_support(None, false, false));
        assert!(!resolve_kitty_support(None, true, true));
        assert!(!resolve_kitty_support(Some(true), true, true));
        assert!(resolve_kitty_support(Some(true), false, false));
        assert!(!resolve_kitty_support(Some(false), false, true));
    }

    #[test]
    fn negotiate_honors_gates_and_replies() {
        // Apple: no SYNC query sent, in-band still queried.
        let mut queried = Vec::new();
        let modes = negotiate_with(true, "Apple_Terminal", |mode| {
            queried.push(mode);
            Some(format!("\x1b[?{mode};1$y").into_bytes())
        });
        assert_eq!(queried, vec![IN_BAND_RESIZE_MODE]);
        assert!(!modes.sync_supported);
        assert!(modes.in_band_resize_supported);

        // Piped: nothing queried.
        let modes = negotiate_with(false, "", |_| {
            panic!("must not query when piped");
        });
        assert_eq!(modes, NegotiatedModes::default());

        // Timeout (None): unsupported, no hang.
        let modes = negotiate_with(true, "", |_| None);
        assert_eq!(modes, NegotiatedModes::default());
    }
}
