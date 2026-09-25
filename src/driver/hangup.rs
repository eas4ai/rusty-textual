//! Hang-up watchdog for the live terminal (Linux).
//!
//! crossterm's Unix input source (0.28 and 0.29) reads the terminal in a loop
//! that never returns once the terminal has hung up: `read` keeps returning 0
//! bytes (or `EIO`), which the loop treats as "try again"
//! (crossterm-rs/crossterm#793). A process whose terminal closes without a
//! SIGHUP, for example one with no controlling terminal, then spins at 100%
//! CPU inside `event::poll` and never exits.
//!
//! [`HangupWatch`] waits in `poll(2)` on the terminal for hang-up only, so it
//! sleeps until then. When the terminal hangs up, it raises SIGHUP: the
//! process ends as it would if the terminal were its controlling terminal.
//! Python Textual exits in the same case, when its input thread's `os.read`
//! fails.
//!
//! Linux only: macOS `poll(2)` does not support devices (its man page lists
//! this under BUGS), so a terminal cannot be watched this way there.

use std::io::{self, IsTerminal};
use std::os::fd::{AsFd, OwnedFd};
use std::thread::JoinHandle;

use rustix::event::{PollFd, PollFlags, poll};

/// Watches the terminal for hang-up while the driver runs. Dropping it stops
/// the watch without firing.
pub(crate) struct HangupWatch {
    /// Write end of the wake pipe. Closing it wakes the thread.
    wake: Option<OwnedFd>,
    thread: Option<JoinHandle<()>>,
}

impl HangupWatch {
    /// Watch the terminal crossterm reads from (stdin when it is a terminal,
    /// else `/dev/tty`) and raise SIGHUP when it hangs up.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] when the terminal cannot be opened, or the
    /// wake pipe or the thread cannot be created.
    pub(crate) fn start() -> io::Result<Self> {
        Self::start_with(terminal_input()?, || {
            let _ = rustix::process::kill_process(
                rustix::process::getpid(),
                rustix::process::Signal::Hup,
            );
        })
    }

    fn start_with(tty: OwnedFd, on_hangup: impl FnOnce() + Send + 'static) -> io::Result<Self> {
        let (wake_read, wake_write) = rustix::pipe::pipe()?;
        let thread = std::thread::Builder::new()
            .name("textual-hangup-watch".to_string())
            .spawn(move || {
                if wait_for_hangup(&tty, &wake_read) {
                    on_hangup();
                }
            })?;
        Ok(Self {
            wake: Some(wake_write),
            thread: Some(thread),
        })
    }
}

impl Drop for HangupWatch {
    fn drop(&mut self) {
        // Closing the write end makes the read end readable (end of file).
        drop(self.wake.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The terminal crossterm reads input from.
fn terminal_input() -> io::Result<OwnedFd> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        stdin.as_fd().try_clone_to_owned()
    } else {
        Ok(std::fs::File::open("/dev/tty")?.into())
    }
}

/// Block until the terminal hangs up (`true`) or the wake pipe closes
/// (`false`). The terminal is polled with no requested events, so only
/// hang-up and error conditions wake it. An invalid descriptor (`NVAL`)
/// stops the watch without firing.
fn wait_for_hangup(tty: &OwnedFd, wake: &OwnedFd) -> bool {
    loop {
        let mut fds = [
            PollFd::new(tty, PollFlags::empty()),
            PollFd::new(wake, PollFlags::IN),
        ];
        match poll(&mut fds, -1) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => return false,
        }
        let tty_events = fds[0].revents();
        if !fds[1].revents().is_empty() || tty_events.contains(PollFlags::NVAL) {
            return false;
        }
        if tty_events.intersects(PollFlags::HUP | PollFlags::ERR) {
            return true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HangupWatch;
    use rustix::fs::{Mode, OFlags};
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    use std::os::fd::OwnedFd;
    use std::sync::mpsc;
    use std::time::Duration;

    /// A pseudo-terminal pair: (controlling side, terminal side). The
    /// terminal side is opened with `NOCTTY`, so the test process gains no
    /// controlling terminal.
    fn pty_pair() -> (OwnedFd, OwnedFd) {
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).expect("openpt");
        grantpt(&master).expect("grantpt");
        unlockpt(&master).expect("unlockpt");
        let name = ptsname(&master, Vec::new()).expect("ptsname");
        let slave = rustix::fs::open(
            name.as_c_str(),
            OFlags::RDWR | OFlags::NOCTTY,
            Mode::empty(),
        )
        .expect("open pty slave");
        (master, slave)
    }

    #[test]
    fn fires_when_the_terminal_hangs_up() {
        let (master, slave) = pty_pair();
        let (tx, rx) = mpsc::channel();
        let watch = HangupWatch::start_with(slave, move || {
            let _ = tx.send(());
        })
        .expect("start watch");

        assert!(
            rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "no hang-up while the terminal is open"
        );
        drop(master);
        rx.recv_timeout(Duration::from_secs(5))
            .expect("closing the terminal should fire the watch");
        drop(watch);
    }

    #[test]
    fn stops_without_firing_when_dropped() {
        let (master, slave) = pty_pair();
        let (tx, rx) = mpsc::channel();
        let watch = HangupWatch::start_with(slave, move || {
            let _ = tx.send(());
        })
        .expect("start watch");

        // Joins the thread; the callback (and its sender) is dropped unused.
        drop(watch);
        assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
        drop(master);
    }
}
