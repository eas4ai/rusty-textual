//! Keys typed while the terminal starts (TRM-004).
//!
//! The Linux driver reads stdin itself while it waits for the terminal's
//! replies to its startup queries (see [`super::live`]), so every key typed
//! before or during that exchange is read there, not by crossterm. This
//! module turns those bytes back into the events crossterm would have made
//! of them, as Python's input thread parses every byte it reads, replies
//! and keys alike. The terminal's replies (`CSI ? ...`), cursor position
//! reports, and mouse and focus reports are dropped: they are not keys
//! (INL-007).
//!
//! Each key becomes the event crossterm 0.28's Unix parser makes of the
//! same bytes in raw mode when they arrive as separate keys: text, control
//! keys (`\n` is Ctrl+J, as in Python), Esc and Alt+key, the `CSI` and
//! `SS3` key sequences with their modifiers, the Linux console's F1 to F5,
//! `CSI u` keys (the kitty keyboard protocol, its keypad, media, lock and
//! modifier keys too) and bracketed paste. All the bytes arrive here in one
//! buffer, so an ESC always starts a new sequence, as in Python's parser: an
//! Esc typed just before another sequence stays a lone Esc. A malformed
//! sequence is dropped up to its final byte.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MediaKeyCode,
    ModifierKeyCode,
};

const ESC: u8 = 0x1b;

/// The events the keys in `bytes` stand for, in order.
pub(crate) fn parse(bytes: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let (event, used) = next_event(rest);
        events.extend(event);
        rest = &rest[used..];
    }
    events
}

/// The event the bytes at the start of `bytes` stand for, if any, and how
/// many bytes it takes. Takes at least one byte of a non-empty `bytes`.
fn next_event(bytes: &[u8]) -> (Option<Event>, usize) {
    if bytes[0] == ESC {
        escape(bytes)
    } else {
        let (key, used) = plain_key(bytes);
        (key.map(Event::Key), used)
    }
}

/// A key that does not start with ESC: text or a control key.
fn plain_key(bytes: &[u8]) -> (Option<KeyEvent>, usize) {
    let key = |code, modifiers| Some(KeyEvent::new(code, modifiers));
    match bytes[0] {
        b'\r' => (key(KeyCode::Enter, KeyModifiers::NONE), 1),
        b'\t' => (key(KeyCode::Tab, KeyModifiers::NONE), 1),
        0x7f => (key(KeyCode::Backspace, KeyModifiers::NONE), 1),
        0 => (key(KeyCode::Char(' '), KeyModifiers::CONTROL), 1),
        c @ 0x01..=0x1a => (
            key(
                KeyCode::Char(char::from(c - 0x01 + b'a')),
                KeyModifiers::CONTROL,
            ),
            1,
        ),
        c @ 0x1c..=0x1f => (
            key(
                KeyCode::Char(char::from(c - 0x1c + b'4')),
                KeyModifiers::CONTROL,
            ),
            1,
        ),
        _ => match utf8_char(bytes) {
            Some((c, used)) => {
                let modifiers = if c.is_uppercase() {
                    KeyModifiers::SHIFT
                } else {
                    KeyModifiers::NONE
                };
                (key(KeyCode::Char(c), modifiers), used)
            }
            // Not the start of a UTF-8 character: skip the byte.
            None => (None, 1),
        },
    }
}

/// The UTF-8 character at the start of `bytes`, and its length.
fn utf8_char(bytes: &[u8]) -> Option<(char, usize)> {
    let len = match bytes[0] {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return None,
    };
    let text = std::str::from_utf8(bytes.get(..len)?).ok()?;
    text.chars().next().map(|c| (c, len))
}

/// A sequence that starts with ESC: a lone Esc, Alt+key, `CSI` or `SS3`.
fn escape(bytes: &[u8]) -> (Option<Event>, usize) {
    match bytes.get(1) {
        // A lone Esc, at the end or before an ESC, which starts a new
        // sequence (Python's parser).
        None | Some(&ESC) => (Some(Event::Key(KeyCode::Esc.into())), 1),
        Some(b'[') => csi(bytes),
        Some(b'O') => ss3(bytes),
        Some(_) => {
            let (key, used) = plain_key(&bytes[1..]);
            let alt = key.map(|mut key| {
                key.modifiers |= KeyModifiers::ALT;
                Event::Key(key)
            });
            (alt, used + 1)
        }
    }
}

/// `ESC O <final>`: arrows, Home, End and F1-F4.
fn ss3(bytes: &[u8]) -> (Option<Event>, usize) {
    let Some(&last) = bytes.get(2) else {
        return (None, bytes.len());
    };
    let code = match last {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'P'..=b'S' => KeyCode::F(1 + last - b'P'),
        _ => return (None, 3),
    };
    (Some(Event::Key(code.into())), 3)
}

/// `ESC [ <parameters> <intermediates> <final>`.
fn csi(bytes: &[u8]) -> (Option<Event>, usize) {
    match bytes.get(2) {
        // An X10 mouse report: three bytes follow the M.
        Some(b'M') => return (None, bytes.len().min(6)),
        // The Linux console's F1 to F5: `ESC [ [ A` to `ESC [ [ E`.
        Some(b'[') => {
            return match bytes.get(3) {
                Some(&last @ b'A'..=b'E') => {
                    (Some(Event::Key(KeyCode::F(1 + last - b'A').into())), 4)
                }
                _ => (None, malformed_end(bytes, 3)),
            };
        }
        _ => {}
    }
    let body = &bytes[2..];
    let params_len = body
        .iter()
        .take_while(|b| (0x30..=0x3f).contains(*b))
        .count();
    let inter_len = body[params_len..]
        .iter()
        .take_while(|b| (0x20..=0x2f).contains(*b))
        .count();
    let seen = 2 + params_len + inter_len;
    match bytes.get(seen) {
        Some(&last) if (0x40..=0x7e).contains(&last) => {
            let used = seen + 1;
            // Parameter bytes are ASCII, so this never fails.
            let params = std::str::from_utf8(&body[..params_len]).unwrap_or_default();
            if params == "200" && last == b'~' {
                return paste(bytes, used);
            }
            (csi_key(params, inter_len > 0, last).map(Event::Key), used)
        }
        // Malformed (a negative mouse coordinate, say) or cut off.
        _ => (None, malformed_end(bytes, seen)),
    }
}

/// Where a malformed sequence ends when its parsing stopped at `from`:
/// after its final byte, before an ESC that starts the next sequence, or
/// at the end of the input.
fn malformed_end(bytes: &[u8], from: usize) -> usize {
    bytes
        .iter()
        .enumerate()
        .skip(from)
        .find_map(|(at, &b)| match b {
            ESC => Some(at),
            0x40..=0x7e => Some(at + 1),
            _ => None,
        })
        .unwrap_or(bytes.len())
}

/// The key a `CSI` sequence stands for, if it is a key.
fn csi_key(params: &str, intermediate: bool, last: u8) -> Option<KeyEvent> {
    // The terminal's replies (`CSI ? ...`, `CSI > ...`, with `$` and the
    // like), and SGR mouse reports (`CSI < ...`), are not keys.
    if intermediate || params.starts_with(['?', '<', '>', '=']) {
        return None;
    }
    let mut fields = params.split(';');
    let first = fields.next().unwrap_or_default();
    let field = modifier_field(fields.next());
    let code = match last {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'P' => KeyCode::F(1),
        b'Q' => KeyCode::F(2),
        b'S' => KeyCode::F(4),
        b'Z' => return Some(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
        b'~' => {
            return Some(KeyEvent::new_with_kind_and_state(
                numbered_key(first)?,
                field.modifiers,
                field.kind,
                field.state,
            ));
        }
        b'u' => return csi_u_key(first, field),
        // Cursor position (R), focus (I, O), mouse (M, m) and the rest.
        _ => return None,
    };
    Some(KeyEvent::new_with_kind(code, field.modifiers, field.kind))
}

/// `CSI <n> ~` keys.
fn numbered_key(first: &str) -> Option<KeyCode> {
    let code = match first.parse::<u8>().ok()? {
        1 | 7 => KeyCode::Home,
        2 => KeyCode::Insert,
        3 => KeyCode::Delete,
        4 | 8 => KeyCode::End,
        5 => KeyCode::PageUp,
        6 => KeyCode::PageDown,
        n @ 11..=15 => KeyCode::F(n - 10),
        n @ 17..=21 => KeyCode::F(n - 11),
        n @ 23..=26 => KeyCode::F(n - 12),
        n @ 28..=29 => KeyCode::F(n - 15),
        n @ 31..=34 => KeyCode::F(n - 17),
        _ => return None,
    };
    Some(code)
}

/// `CSI <codepoint> ; <modifiers> u` keys.
fn csi_u_key(first: &str, field: ModifierField) -> Option<KeyEvent> {
    let codepoint: u32 = first.split(':').next()?.parse().ok()?;
    let ModifierField {
        mut modifiers,
        kind,
        mut state,
    } = field;
    let code = if let Some((code, key_state)) = kitty_functional_key(codepoint) {
        state |= key_state;
        code
    } else {
        match char::from_u32(codepoint)? {
            '\x1b' => KeyCode::Esc,
            '\r' => KeyCode::Enter,
            '\t' if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
            '\t' => KeyCode::Tab,
            '\x7f' => KeyCode::Backspace,
            c => KeyCode::Char(c),
        }
    };
    // A lone modifier key sets its own modifier, as in crossterm.
    if let KeyCode::Modifier(key) = code {
        modifiers |= match key {
            ModifierKeyCode::LeftShift | ModifierKeyCode::RightShift => KeyModifiers::SHIFT,
            ModifierKeyCode::LeftControl | ModifierKeyCode::RightControl => KeyModifiers::CONTROL,
            ModifierKeyCode::LeftAlt | ModifierKeyCode::RightAlt => KeyModifiers::ALT,
            ModifierKeyCode::LeftSuper | ModifierKeyCode::RightSuper => KeyModifiers::SUPER,
            ModifierKeyCode::LeftHyper | ModifierKeyCode::RightHyper => KeyModifiers::HYPER,
            ModifierKeyCode::LeftMeta | ModifierKeyCode::RightMeta => KeyModifiers::META,
            _ => KeyModifiers::NONE,
        };
    }
    Some(KeyEvent::new_with_kind_and_state(
        code, modifiers, kind, state,
    ))
}

/// The kitty keyboard protocol's keys in the private use area, as
/// crossterm's `translate_functional_key_code` maps them: keypad keys
/// (with the keypad state), lock keys, F13 to F35, media keys and lone
/// modifier keys.
fn kitty_functional_key(codepoint: u32) -> Option<(KeyCode, KeyEventState)> {
    const KEYPAD: [KeyCode; 29] = [
        KeyCode::Char('0'),
        KeyCode::Char('1'),
        KeyCode::Char('2'),
        KeyCode::Char('3'),
        KeyCode::Char('4'),
        KeyCode::Char('5'),
        KeyCode::Char('6'),
        KeyCode::Char('7'),
        KeyCode::Char('8'),
        KeyCode::Char('9'),
        KeyCode::Char('.'),
        KeyCode::Char('/'),
        KeyCode::Char('*'),
        KeyCode::Char('-'),
        KeyCode::Char('+'),
        KeyCode::Enter,
        KeyCode::Char('='),
        KeyCode::Char(','),
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Insert,
        KeyCode::Delete,
        KeyCode::KeypadBegin,
    ];
    const MEDIA: [MediaKeyCode; 13] = [
        MediaKeyCode::Play,
        MediaKeyCode::Pause,
        MediaKeyCode::PlayPause,
        MediaKeyCode::Reverse,
        MediaKeyCode::Stop,
        MediaKeyCode::FastForward,
        MediaKeyCode::Rewind,
        MediaKeyCode::TrackNext,
        MediaKeyCode::TrackPrevious,
        MediaKeyCode::Record,
        MediaKeyCode::LowerVolume,
        MediaKeyCode::RaiseVolume,
        MediaKeyCode::MuteVolume,
    ];
    const MODIFIERS: [ModifierKeyCode; 14] = [
        ModifierKeyCode::LeftShift,
        ModifierKeyCode::LeftControl,
        ModifierKeyCode::LeftAlt,
        ModifierKeyCode::LeftSuper,
        ModifierKeyCode::LeftHyper,
        ModifierKeyCode::LeftMeta,
        ModifierKeyCode::RightShift,
        ModifierKeyCode::RightControl,
        ModifierKeyCode::RightAlt,
        ModifierKeyCode::RightSuper,
        ModifierKeyCode::RightHyper,
        ModifierKeyCode::RightMeta,
        ModifierKeyCode::IsoLevel3Shift,
        ModifierKeyCode::IsoLevel5Shift,
    ];
    let offset = |base: u32| usize::try_from(codepoint - base).ok();
    let code = match codepoint {
        57399..=57427 => return Some((KEYPAD[offset(57399)?], KeyEventState::KEYPAD)),
        57358 => KeyCode::CapsLock,
        57359 => KeyCode::ScrollLock,
        57360 => KeyCode::NumLock,
        57361 => KeyCode::PrintScreen,
        57362 => KeyCode::Pause,
        57363 => KeyCode::Menu,
        57376..=57398 => KeyCode::F(13 + u8::try_from(codepoint - 57376).ok()?),
        57428..=57440 => KeyCode::Media(MEDIA[offset(57428)?]),
        57441..=57454 => KeyCode::Modifier(MODIFIERS[offset(57441)?]),
        _ => return None,
    };
    Some((code, KeyEventState::empty()))
}

/// The parts of a `<mask>[:<kind>]` modifier field.
#[derive(Clone, Copy)]
struct ModifierField {
    modifiers: KeyModifiers,
    kind: KeyEventKind,
    /// Caps Lock and Num Lock, bits 64 and 128 of the mask.
    state: KeyEventState,
}

/// Parse a `<mask>[:<kind>]` modifier field; an absent field is no
/// modifier on a key press.
fn modifier_field(field: Option<&str>) -> ModifierField {
    let mut parts = field.unwrap_or_default().split(':');
    let mask: u8 = parts.next().and_then(|m| m.parse().ok()).unwrap_or(1);
    let kind = match parts.next().and_then(|k| k.parse::<u8>().ok()) {
        Some(2) => KeyEventKind::Repeat,
        Some(3) => KeyEventKind::Release,
        _ => KeyEventKind::Press,
    };
    let bits = mask.saturating_sub(1);
    let modifiers = [
        (1, KeyModifiers::SHIFT),
        (2, KeyModifiers::ALT),
        (4, KeyModifiers::CONTROL),
        (8, KeyModifiers::SUPER),
        (16, KeyModifiers::HYPER),
        (32, KeyModifiers::META),
    ]
    .into_iter()
    .filter(|&(bit, _)| bits & bit != 0)
    .fold(KeyModifiers::NONE, |all, (_, modifier)| all | modifier);
    let mut state = KeyEventState::empty();
    if bits & 64 != 0 {
        state |= KeyEventState::CAPS_LOCK;
    }
    if bits & 128 != 0 {
        state |= KeyEventState::NUM_LOCK;
    }
    ModifierField {
        modifiers,
        kind,
        state,
    }
}

/// Bracketed paste: the text up to `ESC [ 201 ~`, or to the end of the
/// input when the paste was cut off.
fn paste(bytes: &[u8], start: usize) -> (Option<Event>, usize) {
    const END: &[u8] = b"\x1b[201~";
    let text = &bytes[start..];
    let (len, used) = text
        .windows(END.len())
        .position(|window| window == END)
        .map_or((text.len(), bytes.len()), |at| (at, start + at + END.len()));
    let pasted = String::from_utf8_lossy(&text[..len]).into_owned();
    (Some(Event::Paste(pasted)), used)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn plain(code: KeyCode) -> Event {
        key(code, KeyModifiers::NONE)
    }

    #[test]
    fn text_is_typed_as_characters() {
        assert_eq!(
            parse("eéA中".as_bytes()),
            vec![
                plain(KeyCode::Char('e')),
                plain(KeyCode::Char('é')),
                key(KeyCode::Char('A'), KeyModifiers::SHIFT),
                plain(KeyCode::Char('中')),
            ]
        );
    }

    #[test]
    fn control_bytes_are_their_keys() {
        assert_eq!(
            parse(b"\r\n\t\x7f\x05\x00\x1c"),
            vec![
                plain(KeyCode::Enter),
                key(KeyCode::Char('j'), KeyModifiers::CONTROL),
                plain(KeyCode::Tab),
                plain(KeyCode::Backspace),
                key(KeyCode::Char('e'), KeyModifiers::CONTROL),
                key(KeyCode::Char(' '), KeyModifiers::CONTROL),
                key(KeyCode::Char('4'), KeyModifiers::CONTROL),
            ]
        );
    }

    #[test]
    fn escape_alone_twice_or_before_a_key() {
        assert_eq!(parse(b"\x1b"), vec![plain(KeyCode::Esc)]);
        assert_eq!(
            parse(b"\x1b\x1b"),
            vec![plain(KeyCode::Esc), plain(KeyCode::Esc)]
        );
        assert_eq!(
            parse(b"\x1ba"),
            vec![key(KeyCode::Char('a'), KeyModifiers::ALT)]
        );
    }

    #[test]
    fn an_escape_before_a_sequence_leaves_the_sequence_whole() {
        // Esc then Up, and Esc just before the terminal's replies: the next
        // ESC starts a new sequence, as in Python's parser.
        assert_eq!(
            parse(b"\x1b\x1b[A"),
            vec![plain(KeyCode::Esc), plain(KeyCode::Up)]
        );
        assert_eq!(
            parse(b"\x1b\x1b[?2026;2$y\x1b\x1b[?62;22c"),
            vec![plain(KeyCode::Esc), plain(KeyCode::Esc)]
        );
    }

    #[test]
    fn key_sequences_with_and_without_modifiers() {
        assert_eq!(
            parse(b"\x1b[A\x1b[1;5C\x1bOP\x1b[15~\x1b[3;2~\x1b[Z\x1b[6~"),
            vec![
                plain(KeyCode::Up),
                key(KeyCode::Right, KeyModifiers::CONTROL),
                plain(KeyCode::F(1)),
                plain(KeyCode::F(5)),
                key(KeyCode::Delete, KeyModifiers::SHIFT),
                key(KeyCode::BackTab, KeyModifiers::SHIFT),
                plain(KeyCode::PageDown),
            ]
        );
    }

    #[test]
    fn csi_u_keys_follow_the_kitty_protocol() {
        assert_eq!(
            parse(b"\x1b[97;5u\x1b[13u\x1b[9;2u"),
            vec![
                key(KeyCode::Char('a'), KeyModifiers::CONTROL),
                plain(KeyCode::Enter),
                key(KeyCode::BackTab, KeyModifiers::SHIFT),
            ]
        );
        assert_eq!(
            parse(b"\x1b[97;1:3u"),
            vec![Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('a'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ))]
        );
    }

    #[test]
    fn kitty_functional_keys_are_mapped_as_crossterm_maps_them() {
        let with_state = |code, modifiers, state| {
            Event::Key(KeyEvent::new_with_kind_and_state(
                code,
                modifiers,
                KeyEventKind::Press,
                state,
            ))
        };
        assert_eq!(
            parse(b"\x1b[57399u\x1b[57414u\x1b[57376u\x1b[57428u\x1b[57441u\x1b[97;65u"),
            vec![
                with_state(
                    KeyCode::Char('0'),
                    KeyModifiers::NONE,
                    KeyEventState::KEYPAD
                ),
                with_state(KeyCode::Enter, KeyModifiers::NONE, KeyEventState::KEYPAD),
                plain(KeyCode::F(13)),
                plain(KeyCode::Media(MediaKeyCode::Play)),
                key(
                    KeyCode::Modifier(ModifierKeyCode::LeftShift),
                    KeyModifiers::SHIFT
                ),
                with_state(
                    KeyCode::Char('a'),
                    KeyModifiers::NONE,
                    KeyEventState::CAPS_LOCK
                ),
            ]
        );
    }

    #[test]
    fn linux_console_function_keys() {
        assert_eq!(
            parse(b"\x1b[[A\x1b[[E"),
            vec![plain(KeyCode::F(1)), plain(KeyCode::F(5))]
        );
    }

    #[test]
    fn a_malformed_sequence_is_dropped_up_to_its_final_byte() {
        // An SGR mouse report with a negative coordinate (Ghostty; Python
        // `_xterm_parser.py` works around it): nothing of it is a key.
        assert_eq!(parse(b"\x1b[<0;-1;5Me"), vec![plain(KeyCode::Char('e'))]);
        // A sequence broken by the next ESC restarts there.
        assert_eq!(parse(b"\x1b[1;\x1b[A"), vec![plain(KeyCode::Up)]);
    }

    #[test]
    fn replies_and_reports_are_not_keys() {
        // DECRQM and DA1 replies, a cursor position report, SGR and X10
        // mouse reports and a focus report, around the keys typed.
        let bytes = b"e\x1b[?2026;2$y\x1b[?2048;0$y\xc3\xa9\x1b[?62;22c\x1b[12;40R\
            \x1b[<0;10;5M\x1b[M !!\x1b[I\x1b[15~";
        assert_eq!(
            parse(bytes),
            vec![
                plain(KeyCode::Char('e')),
                plain(KeyCode::Char('é')),
                plain(KeyCode::F(5)),
            ]
        );
    }

    #[test]
    fn a_bracketed_paste_is_one_event() {
        assert_eq!(
            parse(b"\x1b[200~hi there\x1b[201~x"),
            vec![
                Event::Paste("hi there".to_string()),
                plain(KeyCode::Char('x'))
            ]
        );
        assert_eq!(
            parse(b"\x1b[200~cut"),
            vec![Event::Paste("cut".to_string())]
        );
    }

    #[test]
    fn cut_off_or_broken_input_is_dropped() {
        assert_eq!(parse(b"e\x1b[1;"), vec![plain(KeyCode::Char('e'))]);
        assert_eq!(parse(b"\x1bO"), vec![]);
        assert_eq!(parse(&[0xff, b'e']), vec![plain(KeyCode::Char('e'))]);
        assert_eq!(parse(&[0xc3]), vec![]);
    }
}
