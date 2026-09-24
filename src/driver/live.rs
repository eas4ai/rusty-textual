//! Live DECRQM transport for startup mode negotiation.
//!
//! Crate-internal by design (uses `crossterm`): the external live probe
//! `include!`s `super::negotiate` — the dependency-free query/parse/gate
//! core — with its own transport, so this module must never move into that
//! file.
//!
//! Why poll-bounded reads in the calling thread instead of a helper thread:
//! crossterm 0.28 has no DECRQM (`?$y`) arm, so the driver performs exactly
//! one synchronous round-trip per mode inside `start()`, before the input
//! loop owns stdin. A helper thread with a blocking stdin read cannot be
//! recalled on timeout — the leaked reader stays parked on stdin and eats
//! later input bytes (Tab/keypresses randomly lost on terminals that never
//! answer DECRQM). `crossterm::event::poll` bounds every wait instead, so a
//! timed-out query leaves no reader behind.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use super::negotiate;

/// Production negotiation against the real terminal.
///
/// Reads `TERM_PROGRAM`/tty state from the environment and performs one
/// bounded round-trip per mode. Call only from driver `start()`, before the
/// input loop owns stdin.
pub(crate) fn negotiate_live() -> negotiate::NegotiatedModes {
    use std::io::IsTerminal;
    let is_tty = io::stdin().is_terminal();
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    negotiate::negotiate_with(is_tty, &term_program, |mode| {
        transact(mode, negotiate::QUERY_TIMEOUT)
    })
}

/// One bounded round-trip for `mode`: emit the DECRQM query, then collect
/// reply bytes up to the first `y` (DECRQM terminator), the 64-byte cap, or
/// `timeout` — whichever comes first. `None` when nothing arrived.
///
/// Single-byte reads, deliberately unbuffered: a `BufReader` could over-read
/// past the reply and steal input bytes from the loop that owns stdin next.
/// No helper thread: every wait is `poll`-bounded in this thread, so on
/// timeout there is no lingering stdin reader.
fn transact(mode: u16, timeout: Duration) -> Option<Vec<u8>> {
    transact_with(
        &negotiate::decrqm_query(mode),
        timeout,
        |q| io::stdout().write_all(q).and_then(|_| io::stdout().flush()),
        crossterm::event::poll,
        || {
            let mut one = [0u8; 1];
            match io::stdin().read(&mut one) {
                Ok(1) => Ok(Some(one[0])),
                Ok(_) => Ok(None),
                Err(e) => Err(e),
            }
        },
    )
}

/// Round-trip over injected I/O (unit-test seam for [`transact`]).
///
/// `poll_ready` reports stdin readiness within the remaining budget;
/// `read_byte` consumes one byte (`None` on EOF). Both run in the calling
/// thread — a timeout returns with no background reader outstanding, which
/// is the regression contract this seam exists to pin.
fn transact_with(
    query: &[u8],
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
            // Timeout or poll error ends the round-trip with whatever arrived.
            Ok(false) => break,
            Err(_) => break,
        }
        match read_byte() {
            Ok(Some(b)) => {
                buf.push(b);
                if b == b'y' || buf.len() >= 64 {
                    break;
                }
            }
            // EOF or read error ends the round-trip with whatever arrived.
            Ok(None) => break,
            Err(_) => break,
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
            transact_with(
                query,
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
        let out = transact_with(
            b"\x1b[?2026$p",
            Duration::from_secs(5),
            |_| Err(io::Error::other("no stdout")),
            |_| panic!("must not poll after a failed write"),
            || panic!("must not read after a failed write"),
        );
        assert_eq!(out, None);
    }
}
