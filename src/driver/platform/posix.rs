use std::io::{self, Write};

use crossterm::event::{
    DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
    PopKeyboardEnhancementFlags,
};
use crossterm::{cursor, execute, terminal};

#[cfg(feature = "trace")]
use tracing::debug;

use crate::driver::{DriverOptions, KeyboardProtocol, PointerShape, Size};

use super::{CapabilityProfile, PlatformDriver};

#[derive(Default)]
pub(crate) struct PosixPlatformDriver;

impl PlatformDriver for PosixPlatformDriver {
    fn start(
        &mut self,
        options: DriverOptions,
        keyboard_protocol: KeyboardProtocol,
    ) -> io::Result<(bool, crate::driver::negotiate::NegotiatedModes)> {
        let enable_keyboard = detect_kitty_keyboard_support(keyboard_protocol);

        #[cfg(feature = "trace")]
        debug!(
            enable_mouse = options.enable_mouse,
            enable_pointer_shapes = options.enable_pointer_shapes,
            enable_focus_change = options.enable_focus_change,
            keyboard_protocol = ?keyboard_protocol,
            enable_keyboard,
            "driver.start"
        );

        terminal::enable_raw_mode()?;
        if let Err(err) = execute!(
            std::io::stdout(),
            terminal::EnterAlternateScreen,
            cursor::Hide,
            terminal::DisableLineWrap
        ) {
            restore_terminal_best_effort();
            return Err(err);
        }
        if options.enable_focus_change {
            if let Err(err) = execute!(std::io::stdout(), EnableFocusChange) {
                restore_terminal_best_effort();
                return Err(err);
            }
        }
        if options.enable_mouse {
            if let Err(err) = execute!(std::io::stdout(), EnableMouseCapture) {
                restore_terminal_best_effort();
                return Err(err);
            }
        }
        // Bracketed paste (DECSET 2004, PR-15a): without this the terminal
        // delivers pastes as raw keystrokes instead of a single Paste event.
        if let Err(err) = execute!(
            std::io::stdout(),
            crate::driver::bracketed_paste_enable_command()
        ) {
            restore_terminal_best_effort();
            return Err(err);
        }

        let keyboard_enhanced = if enable_keyboard {
            // Full flag set, Python parity (DISAMBIGUATE | REPORT_ALL_KEYS |
            // REPORT_ASSOCIATED_TEXT = 25). Emitted raw: crossterm omits flag
            // 16, and non-supporting terminals ignore the sequence
            // (progressive enhancement). Pop on stop mirrors Python.
            write!(
                std::io::stdout(),
                "\x1b[>{}u",
                crate::driver::negotiate::KITTY_FLAGS
            )
            .and_then(|_| std::io::stdout().flush())
            .is_ok()
        } else {
            false
        };

        // Synchronous mode negotiation (PR-15b): bounded DECRQM round-trips
        // before the input loop owns stdin. Never enables in-band resize
        // (its reports are unparseable through crossterm) — records support
        // only. Skipped entirely when piped or on Apple Terminal (SYNC).
        let negotiated = crate::driver::live::negotiate_live();

        Ok((keyboard_enhanced, negotiated))
    }

    fn stop(&mut self, options: DriverOptions, keyboard_enhanced: bool) -> io::Result<()> {
        #[cfg(feature = "trace")]
        debug!(keyboard_enhanced = keyboard_enhanced, "driver.stop");

        let mut first_err: Option<io::Error> = None;
        let mut record = |res: io::Result<()>| {
            if let Err(err) = res {
                if first_err.is_none() {
                    first_err = Some(err);
                }
            }
        };

        if keyboard_enhanced {
            record(execute!(std::io::stdout(), PopKeyboardEnhancementFlags));
        }
        if options.enable_mouse {
            record(execute!(std::io::stdout(), DisableMouseCapture));
        }
        if options.enable_focus_change {
            record(execute!(std::io::stdout(), DisableFocusChange));
        }
        record(execute!(
            std::io::stdout(),
            crate::driver::bracketed_paste_disable_command()
        ));
        record(execute!(
            std::io::stdout(),
            cursor::Show,
            terminal::EnableLineWrap,
            terminal::LeaveAlternateScreen
        ));
        record(terminal::disable_raw_mode());

        if let Some(err) = first_err {
            return Err(err);
        }
        Ok(())
    }

    fn refresh_size(&mut self) -> io::Result<Size> {
        let (width, height) = terminal::size()?;
        #[cfg(feature = "trace")]
        debug!(width, height, "driver.refresh_size");
        Ok(Size { width, height })
    }

    fn reassert_runtime_modes(&mut self, started: bool) -> io::Result<()> {
        if !started {
            return Ok(());
        }
        execute!(std::io::stdout(), terminal::DisableLineWrap)
    }

    fn set_pointer_shape(
        &mut self,
        started: bool,
        options: DriverOptions,
        shape: PointerShape,
    ) -> io::Result<()> {
        if !started || !options.enable_pointer_shapes {
            return Ok(());
        }
        #[cfg(feature = "trace")]
        debug!(shape = shape.as_kitty_name(), "driver.set_pointer_shape");
        let seq = format!("\x1b]22;{}\x07", shape.as_kitty_name());
        let mut out = std::io::stdout();
        out.write_all(seq.as_bytes())?;
        out.flush()?;
        Ok(())
    }
}

pub(crate) fn capability_profile() -> CapabilityProfile {
    CapabilityProfile {
        supports_dim_reliably: true,
        supports_reverse_reliably: true,
        supports_pointer_shapes: detect_pointer_shapes_enabled(),
        supports_kitty_keyboard: detect_kitty_keyboard_support(KeyboardProtocol::Auto),
        supports_focus_change: true,
        requires_mode_reassert_on_resize: true,
    }
}

pub(crate) fn detect_kitty_keyboard_support(protocol: KeyboardProtocol) -> bool {
    // Explicit `TEXTUAL_DISABLE_KITTY_KEY=1` wins over everything, including
    // forced `On` (Python `constants.DISABLE_KITTY_KEY` exact semantics: only
    // `"1"` disables — the operator opt-out beats the API request).
    let disabled_by_env = crate::driver::negotiate::kitty_disabled_by_env(
        std::env::var("TEXTUAL_DISABLE_KITTY_KEY").ok().as_deref(),
    );
    let explicit = match protocol {
        KeyboardProtocol::Off => Some(false),
        KeyboardProtocol::On => Some(true),
        KeyboardProtocol::Auto => match std::env::var("TEXTUAL_KEYBOARD_PROTOCOL")
            .ok()
            .as_deref()
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("off") | Some("0") | Some("false") => Some(false),
            Some("on") | Some("1") | Some("true") => Some(true),
            _ => None,
        },
    };
    // `Auto` with no explicit override additionally requires a tty stdin, so
    // piped runs never emit enhancement sequences into a file (PR-15b).
    let stdin_is_tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    crate::driver::negotiate::resolve_kitty_support(explicit, disabled_by_env, stdin_is_tty)
}

pub(crate) fn detect_pointer_shapes_enabled() -> bool {
    if let Ok(value) = std::env::var("TEXTUAL_POINTER_SHAPES") {
        let v = value.to_lowercase();
        return !(v == "0" || v == "false" || v == "off" || v == "no");
    }

    if std::env::var("TERM_PROGRAM").ok().as_deref().unwrap_or("") == "Apple_Terminal" {
        return false;
    }

    // Default on for POSIX terminals (except known-incompatible Terminal.app above).
    true
}

fn restore_terminal_best_effort() {
    // Best-effort subset of stop(): leave no mode behind that start() may
    // have enabled before failing (PR-15a adds bracketed paste here).
    let _ = execute!(
        std::io::stdout(),
        crate::driver::bracketed_paste_disable_command()
    );
    let _ = execute!(
        std::io::stdout(),
        cursor::Show,
        terminal::EnableLineWrap,
        terminal::LeaveAlternateScreen
    );
    let _ = terminal::disable_raw_mode();
}
