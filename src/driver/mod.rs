//! Terminal driver: terminal lifecycle, capability detection, keyboard-protocol and
//! pointer-shape control built directly on top of [`crossterm`].
//!
//! This is `textual-rs`'s own self-contained crossterm driver (no external backend crate).

use std::io;

#[cfg(target_os = "linux")]
mod hangup;
mod platform;

pub use platform::CapabilityProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerShape {
    Default,
    Pointer,
    Text,
    NotAllowed,
}

impl PointerShape {
    #[must_use]
    pub fn as_kitty_name(self) -> &'static str {
        match self {
            PointerShape::Default => "default",
            PointerShape::Pointer => "pointer",
            PointerShape::Text => "text",
            PointerShape::NotAllowed => "not-allowed",
        }
    }
}

/// Kitty keyboard protocol mode.
///
/// Controls whether the terminal reports enhanced key events that disambiguate
/// keys like Tab vs Ctrl+I, Enter vs Ctrl+M, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardProtocol {
    /// Do not enable keyboard enhancement (legacy mode).
    #[default]
    Off,
    /// Auto-detect: enable on terminals known to support Kitty protocol.
    Auto,
    /// Force enable keyboard enhancement.
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent switches, each its own Python driver argument (`mouse`, `inline`, ...).
pub struct DriverOptions {
    pub enable_mouse: bool,
    pub enable_pointer_shapes: bool,
    pub enable_focus_change: bool,
    pub keyboard_protocol: KeyboardProtocol,
    /// Inline mode (Python `App.run(inline=True)`): stay on the main screen
    /// and leave line wrap alone instead of entering the alternate screen.
    pub inline: bool,
}

impl Default for DriverOptions {
    fn default() -> Self {
        Self {
            // Python parity (`mouse=True`): capture mouse by default (PR-15a).
            enable_mouse: true,
            enable_pointer_shapes: detect_pointer_shapes_enabled(),
            enable_focus_change: false,
            keyboard_protocol: KeyboardProtocol::Off,
            inline: false,
        }
    }
}

pub(crate) mod live;
pub(crate) mod negotiate;

/// Bracketed-paste mode commands (PR-15a).
///
/// Single source for the exact bytes the platform drivers emit on
/// start/stop (DECSET/DECRST 2004), so the headless escape-sequence test
/// pins the wire contract without a live terminal.
pub(crate) fn bracketed_paste_enable_command() -> crossterm::event::EnableBracketedPaste {
    crossterm::event::EnableBracketedPaste
}

/// Bracketed-paste mode commands (PR-15a): teardown half of
/// [`bracketed_paste_enable_command`].
pub(crate) fn bracketed_paste_disable_command() -> crossterm::event::DisableBracketedPaste {
    crossterm::event::DisableBracketedPaste
}

pub struct TerminalDriver {
    size: Size,
    started: bool,
    options: DriverOptions,
    keyboard_enhanced: bool,
    capabilities: CapabilityProfile,
    negotiated: negotiate::NegotiatedModes,
    platform: Box<dyn platform::PlatformDriver>,
    /// Runs while the driver is started; see [`hangup`].
    #[cfg(target_os = "linux")]
    hangup_watch: Option<hangup::HangupWatch>,
}

impl TerminalDriver {
    /// Create a driver for the current platform and read the terminal size.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] when the terminal size cannot be read, for
    /// example when no terminal is attached.
    pub fn new(options: DriverOptions) -> io::Result<Self> {
        let mut platform = platform::make_platform_driver();
        let size = platform.refresh_size()?;
        Ok(Self {
            size,
            started: false,
            options,
            keyboard_enhanced: false,
            capabilities: platform::capability_profile(),
            negotiated: negotiate::NegotiatedModes::default(),
            platform,
            #[cfg(target_os = "linux")]
            hangup_watch: None,
        })
    }

    #[must_use]
    pub fn size(&self) -> Size {
        self.size
    }

    #[must_use]
    pub fn started(&self) -> bool {
        self.started
    }

    #[must_use]
    pub fn options(&self) -> DriverOptions {
        self.options
    }

    /// Choose inline mode (see [`DriverOptions::inline`]). Takes effect at the
    /// next [`start`](Self::start); a started driver keeps its mode until it
    /// stops.
    pub fn set_inline(&mut self, inline: bool) {
        if !self.started {
            self.options.inline = inline;
        }
    }

    /// Terminal capability profile for the active platform driver.
    #[must_use]
    pub fn capabilities(&self) -> CapabilityProfile {
        self.capabilities
    }

    /// Whether the Kitty keyboard enhancement protocol is currently active.
    #[must_use]
    pub fn keyboard_enhanced(&self) -> bool {
        self.keyboard_enhanced
    }

    /// Outcome of the startup mode negotiation (PR-15b): DECRQM answers for
    /// SYNC (2026) and in-band resize (2048), or defaults when skipped
    /// (piped, Apple Terminal for SYNC) or unanswered.
    #[must_use]
    pub fn negotiated_modes(&self) -> negotiate::NegotiatedModes {
        self.negotiated
    }

    /// Put the terminal into application mode. Does nothing when the driver
    /// has already started.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] when raw mode cannot be enabled, or when
    /// writing a mode command to stdout fails (alternate screen, hidden
    /// cursor, no line wrap, focus reporting, mouse capture, or bracketed
    /// paste). On a write failure the driver first restores the terminal on a
    /// best-effort basis. A failure to enable the Kitty keyboard protocol is
    /// not an error.
    pub fn start(&mut self) -> io::Result<()> {
        if self.started {
            return Ok(());
        }
        let (keyboard_enhanced, negotiated) = self
            .platform
            .start(self.options, self.options.keyboard_protocol)?;
        self.keyboard_enhanced = keyboard_enhanced;
        self.negotiated = negotiated;
        self.started = true;
        // Best effort: without the watch, a terminal that closes without a
        // SIGHUP leaves the process spinning inside crossterm (see `hangup`).
        #[cfg(target_os = "linux")]
        {
            self.hangup_watch = hangup::HangupWatch::start().ok();
        }
        Ok(())
    }

    /// Restore the terminal to its normal mode. Does nothing when the driver
    /// has not started.
    ///
    /// # Errors
    ///
    /// Returns the first [`io::Error`] from the restore steps: writing a mode
    /// command to stdout, or disabling raw mode. All steps still run after a
    /// failure, and the driver is marked as stopped either way.
    pub fn stop(&mut self) -> io::Result<()> {
        if !self.started {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            self.hangup_watch = None;
        }
        let result = self.platform.stop(self.options, self.keyboard_enhanced);
        self.keyboard_enhanced = false;
        self.started = false;
        result
    }

    /// Read the current terminal size, store it, and return it.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] when the terminal size cannot be read. The
    /// stored size is left unchanged in that case.
    pub fn refresh_size(&mut self) -> io::Result<Size> {
        self.size = self.platform.refresh_size()?;
        Ok(self.size)
    }

    /// Re-apply runtime modes that some terminals may reset on resize.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] when writing the mode commands to stdout
    /// fails. When the driver has not started, it writes nothing and returns
    /// `Ok(())`.
    pub fn reassert_runtime_modes(&mut self) -> io::Result<()> {
        // Inline mode never changed line wrap, so there is nothing to reassert.
        if self.options.inline {
            return Ok(());
        }
        self.platform.reassert_runtime_modes(self.started)
    }

    /// Set the mouse pointer shape using Kitty pointer-shapes protocol.
    ///
    /// Best effort: terminals that don't support it should ignore the OSC sequence.
    ///
    /// Protocol: `ESC ] 22 ; <shape> BEL`
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] when writing or flushing the sequence to
    /// stdout fails. When pointer shapes are not supported or not enabled, or
    /// the driver has not started, it writes nothing and returns `Ok(())`.
    pub fn set_pointer_shape(&mut self, shape: PointerShape) -> io::Result<()> {
        if !self.capabilities.supports_pointer_shapes {
            return Ok(());
        }
        self.platform
            .set_pointer_shape(self.started, self.options, shape)
    }
}

impl Drop for TerminalDriver {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn detect_pointer_shapes_enabled() -> bool {
    platform::detect_pointer_shapes_enabled()
}

#[cfg(test)]
mod tests {
    use super::platform;

    // NOTE: the upstream richtui-crossterm driver also had tests for the
    // TEXTUAL_POINTER_SHAPES env-var override. They were dropped during the port into
    // textual-rs because they mutate process env via `std::env::set_var`/`remove_var`,
    // which is `unsafe` in edition 2024, and this crate sets `unsafe_code = "forbid"`.

    /// PR-15a: mouse capture defaults on (Python `mouse=True`).
    #[test]
    fn mouse_capture_defaults_on() {
        assert!(super::DriverOptions::default().enable_mouse);
    }

    /// PR-15a: the platform drivers emit exactly DECSET/DECRST 2004.
    ///
    /// Headless pin on the wire contract: the commands executed at
    /// start/stop must encode to the bracketed-paste sequences, or live
    /// pastes arrive as raw keystrokes instead of `PasteEvent`s.
    #[test]
    fn bracketed_paste_commands_encode_decset_2004() {
        let mut enable = Vec::new();
        crossterm::execute!(enable, super::bracketed_paste_enable_command())
            .expect("encode enable");
        assert_eq!(enable, b"\x1b[?2004h");
        let mut disable = Vec::new();
        crossterm::execute!(disable, super::bracketed_paste_disable_command())
            .expect("encode disable");
        assert_eq!(disable, b"\x1b[?2004l");
    }

    #[test]
    fn capability_profile_has_required_flags() {
        let profile = platform::capability_profile();
        assert!(profile.requires_mode_reassert_on_resize);
        assert!(profile.supports_focus_change);

        #[cfg(not(target_os = "windows"))]
        {
            assert!(profile.supports_dim_reliably);
            assert!(profile.supports_reverse_reliably);
        }
    }
}
