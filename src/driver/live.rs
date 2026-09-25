//! Live DECRQM transport for startup mode negotiation.
//!
//! Crate-internal by design: the external live probe `include!`s
//! `super::negotiate` (the dependency-free query/parse/gate core) with its
//! own transport, so this module must never move into that file.
//!
//! No reply may reach crossterm's input reader: crossterm 0.28 cannot parse
//! a DECRQM reply (`CSI ? ... $ y`) and treats it as an incomplete sequence,
//! so it swallows every later input byte, and it blocks in `read` while
//! waiting for the rest. Waiting for the reply with `crossterm::event::poll`
//! did exactly that: the poll itself read the reply, and the app froze at
//! startup in any terminal that answers DECRQM.
//!
//! So on Linux the driver writes the allowed DECRQM queries and then a
//! primary device attributes query (DA1, which every VT-style terminal
//! answers, in order), and reads stdin itself until the DA1 reply ends. Any
//! mode reply arrives before it and is consumed here. Readiness comes from
//! `poll(2)`, which reads nothing, and bytes are read one at a time
//! straight from the file descriptor (not through std's buffered stdin), so
//! nothing past the DA1 reply is taken. Every wait is bounded in this
//! thread, so no reader is left behind on timeout.
//!
//! Other Unix systems skip negotiation: macOS `poll(2)` does not support
//! devices, and a query whose reply cannot be read safely must not be sent.
//! Windows keeps the crossterm-based wait.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use super::negotiate;

/// Primary device attributes query (DA1).
#[cfg(any(test, target_os = "linux"))]
const DEVICE_ATTRIBUTES_QUERY: &[u8] = b"\x1b[c";
/// Last byte of a DA1 reply (`CSI ? Ps ; ... c`). DECRQM replies contain
/// no `c`.
#[cfg(any(test, target_os = "linux"))]
const DEVICE_ATTRIBUTES_END: u8 = b'c';
/// Room for both DECRQM replies and the DA1 reply.
#[cfg(any(test, target_os = "linux"))]
const EXCHANGE_CAP: usize = 256;
/// Budget for the whole exchange: the same worst case as the two
/// per-query round-trips it replaces, for a terminal that answers nothing.
#[cfg(target_os = "linux")]
const EXCHANGE_TIMEOUT: Duration = negotiate::QUERY_TIMEOUT.saturating_mul(2);

/// Production negotiation against the real terminal.
///
/// Reads `TERM_PROGRAM`/tty state from the environment. Call only from
/// driver `start()`, before the input loop owns stdin.
#[cfg(target_os = "linux")]
pub(crate) fn negotiate_live() -> negotiate::NegotiatedModes {
    use std::io::IsTerminal;
    let is_tty = io::stdin().is_terminal();
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let mut queries = Vec::new();
    if negotiate::sync_query_allowed(is_tty, &term_program) {
        queries.extend(negotiate::decrqm_query(negotiate::SYNC_MODE));
    }
    if negotiate::in_band_resize_query_allowed(is_tty) {
        queries.extend(negotiate::decrqm_query(negotiate::IN_BAND_RESIZE_MODE));
    }
    if queries.is_empty() {
        return negotiate::NegotiatedModes::default();
    }
    queries.extend_from_slice(DEVICE_ATTRIBUTES_QUERY);
    let replies = exchange_with(
        &queries,
        DEVICE_ATTRIBUTES_END,
        EXCHANGE_CAP,
        EXCHANGE_TIMEOUT,
        |q| {
            io::stdout()
                .write_all(q)
                .and_then(|()| io::stdout().flush())
        },
        stdin_ready,
        read_stdin_byte,
    );
    // Each mode parses its own reply out of the collected bytes.
    negotiate::negotiate_with(is_tty, &term_program, |_| replies.clone())
}

/// Other Unix systems: no negotiation (see the module docs).
#[cfg(all(unix, not(target_os = "linux")))]
pub(crate) fn negotiate_live() -> negotiate::NegotiatedModes {
    negotiate::NegotiatedModes::default()
}

/// Windows: one DECRQM round-trip per mode, waiting through crossterm.
#[cfg(windows)]
pub(crate) fn negotiate_live() -> negotiate::NegotiatedModes {
    use std::io::IsTerminal;
    let is_tty = io::stdin().is_terminal();
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    negotiate::negotiate_with(is_tty, &term_program, |mode| {
        exchange_with(
            &negotiate::decrqm_query(mode),
            b'y',
            64,
            negotiate::QUERY_TIMEOUT,
            |q| {
                io::stdout()
                    .write_all(q)
                    .and_then(|()| io::stdout().flush())
            },
            crossterm::event::poll,
            read_stdin_byte,
        )
    })
}

/// Whether stdin has input within `timeout`, without reading any.
#[cfg(target_os = "linux")]
fn stdin_ready(timeout: Duration) -> io::Result<bool> {
    use rustix::event::{PollFd, PollFlags, poll};
    let stdin = io::stdin();
    let mut fds = [PollFd::new(&stdin, PollFlags::IN)];
    let millis = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    let ready = poll(&mut fds, millis)?;
    Ok(ready > 0 && fds[0].revents().contains(PollFlags::IN))
}

/// Read one byte from the stdin file descriptor (`None` on end of file).
///
/// Unbuffered: `io::stdin()` reads ahead into std's own buffer, where the
/// rest of a reply would be invisible to `poll(2)` and stranded.
#[cfg(target_os = "linux")]
fn read_stdin_byte() -> io::Result<Option<u8>> {
    use std::os::fd::AsFd;
    let mut one = [0u8; 1];
    match rustix::io::read(io::stdin().as_fd(), &mut one)? {
        1 => Ok(Some(one[0])),
        _ => Ok(None),
    }
}

/// Read one byte from stdin (`None` on end of file).
#[cfg(windows)]
fn read_stdin_byte() -> io::Result<Option<u8>> {
    use std::io::Read;
    let mut one = [0u8; 1];
    match io::stdin().read(&mut one)? {
        1 => Ok(Some(one[0])),
        _ => Ok(None),
    }
}

/// One bounded exchange over injected I/O: write `query`, then collect reply
/// bytes up to and including the first `end` byte, `cap` bytes, or
/// `timeout`, whichever comes first. `None` when nothing arrived.
///
/// `poll_ready` reports stdin readiness within the remaining budget without
/// consuming input; `read_byte` consumes one byte (`None` on end of file).
/// Both run in the calling thread, so a timeout leaves no reader behind.
/// Single-byte reads never take input past the reply.
#[cfg(any(test, target_os = "linux", windows))]
fn exchange_with(
    query: &[u8],
    end: u8,
    cap: usize,
    timeout: Duration,
    write: impl FnOnce(&[u8]) -> io::Result<()>,
    mut poll_ready: impl FnMut(Duration) -> io::Result<bool>,
    mut read_byte: impl FnMut() -> io::Result<Option<u8>>,
) -> Option<Vec<u8>> {
    write(query).ok()?;
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::with_capacity(32);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match poll_ready(remaining) {
            Ok(true) => {}
            // Timeout or poll error ends the exchange with whatever arrived.
            Ok(false) | Err(_) => break,
        }
        match read_byte() {
            Ok(Some(b)) => {
                buf.push(b);
                if b == end || buf.len() >= cap {
                    break;
                }
            }
            // End of file or a read error ends the exchange.
            Ok(None) | Err(_) => break,
        }
    }
    if buf.is_empty() { None } else { Some(buf) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Scripted transport: canned readiness answers plus a byte queue, with a
    /// read counter so tests can prove no read was attempted — i.e. no
    /// background consumer can exist after a timeout.
    struct Script {
        ready: VecDeque<bool>,
        bytes: VecDeque<u8>,
        reads: usize,
        writes: Vec<Vec<u8>>,
    }

    impl Script {
        fn new(ready: &[bool], bytes: &[u8]) -> Self {
            Self {
                ready: ready.iter().copied().collect(),
                bytes: bytes.iter().copied().collect(),
                reads: 0,
                writes: Vec::new(),
            }
        }

        fn run(&mut self, query: &[u8], timeout: Duration) -> Option<Vec<u8>> {
            exchange_with(
                query,
                b'y',
                64,
                timeout,
                |q| {
                    self.writes.push(q.to_vec());
                    Ok(())
                },
                |_| Ok(self.ready.pop_front().unwrap_or(false)),
                || {
                    self.reads += 1;
                    Ok(self.bytes.pop_front())
                },
            )
        }
    }

    #[test]
    fn unanswered_query_times_out_without_reading() {
        // Regression (timed-out DECRQM stole later stdin bytes): the old
        // helper-thread transport left a blocked reader parked on stdin, so a
        // Tab arriving after the timeout was eaten. The poll-bounded
        // transport attempts no read at all when nothing is ready.
        let mut script = Script::new(&[], &[]);
        let out = script.run(b"\x1b[?2026$p", Duration::from_millis(20));
        assert_eq!(out, None);
        assert_eq!(
            script.reads, 0,
            "no read may be attempted without readiness"
        );
        assert_eq!(script.writes, vec![b"\x1b[?2026$p".to_vec()]);
    }

    #[test]
    fn bytes_arriving_after_a_timeout_stay_available() {
        // The byte offered after the first (timed-out) round-trip must still
        // be there for the next one: nothing lingers between round-trips.
        let mut script = Script::new(&[false], &[]);
        assert_eq!(script.run(b"\x1b[?2026$p", Duration::from_millis(20)), None);
        script.ready.push_back(true);
        script.bytes.push_back(b'\t');
        assert_eq!(
            script.run(b"\x1b[?2048$p", Duration::from_millis(20)),
            Some(vec![b'\t'])
        );
    }

    #[test]
    fn reply_stops_at_first_y_terminator() {
        let reply = b"\x1b[?2026;1$yTRAILING";
        let ready = vec![true; reply.len()];
        let mut script = Script::new(&ready, reply);
        let out = script.run(b"\x1b[?2026$p", Duration::from_secs(5));
        assert_eq!(out, Some(b"\x1b[?2026;1$y".to_vec()));
        // Trailing bytes are never over-read.
        assert_eq!(script.bytes.len(), reply.len() - b"\x1b[?2026;1$y".len());
    }

    #[test]
    fn eof_yields_none_when_nothing_arrived() {
        let mut script = Script::new(&[true], &[]);
        assert_eq!(script.run(b"\x1b[?2026$p", Duration::from_secs(5)), None);
    }

    #[test]
    fn write_failure_yields_none() {
        let out = exchange_with(
            b"\x1b[?2026$p",
            b'y',
            64,
            Duration::from_secs(5),
            |_| Err(io::Error::other("no stdout")),
            |_| panic!("must not poll after a failed write"),
            || panic!("must not read after a failed write"),
        );
        assert_eq!(out, None);
    }

    #[test]
    fn exchange_reads_every_mode_reply_up_to_the_device_attributes_reply() {
        // Regression (app froze at startup in terminals that answer DECRQM):
        // every mode reply must be consumed here, never by crossterm's reader,
        // so the exchange reads through to the DA1 reply that follows them.
        let replies = b"\x1b[?2026;2$y\x1b[?2048;0$y\x1b[?62;22c";
        let mut stream: Vec<u8> = replies.to_vec();
        stream.extend_from_slice(b"typed");
        let mut script = Script::new(&vec![true; stream.len()], &stream);
        let out = exchange_with(
            b"queries",
            DEVICE_ATTRIBUTES_END,
            EXCHANGE_CAP,
            Duration::from_secs(5),
            |_| Ok(()),
            |_| Ok(script.ready.pop_front().unwrap_or(false)),
            || Ok(script.bytes.pop_front()),
        );
        assert_eq!(out, Some(replies.to_vec()));
        // Input after the DA1 reply is left for the input loop.
        assert_eq!(script.bytes.iter().copied().collect::<Vec<u8>>(), b"typed");
    }

    #[test]
    fn each_mode_is_parsed_from_the_batched_replies() {
        let replies = b"\x1b[?2026;2$y\x1b[?2048;0$y\x1b[?62;22c".to_vec();
        let modes = negotiate::negotiate_with(true, "", |_| Some(replies.clone()));
        assert!(
            modes.sync_supported,
            "2026 answered with Ps=2 (reset, supported)"
        );
        assert!(
            !modes.in_band_resize_supported,
            "2048 answered with Ps=0 (not recognized)"
        );
        assert_eq!(DEVICE_ATTRIBUTES_QUERY, b"\x1b[c");
    }
}
