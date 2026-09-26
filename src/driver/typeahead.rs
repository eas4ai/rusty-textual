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
//! The mapping follows crossterm 0.28's Unix parser: text, control keys,
//! Esc and Alt+key, the `CSI` and `SS3` key sequences with their
//! modifiers, `CSI u` keys (the kitty keyboard protocol) and bracketed
//! paste. Two differences: `\n` is Enter, because the terminal turns Enter
//! into `\n` in the bytes typed before the driver enters raw mode (ICRNL),
//! and kitty's keys in the private use area (keypad, media and lone
//! modifier keys) are dropped.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

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
        b'\r' | b'\n' => (key(KeyCode::Enter, KeyModifiers::NONE), 1),
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
        None => (Some(Event::Key(KeyCode::Esc.into())), 1),
        Some(b'[') => csi(bytes),
        Some(b'O') => ss3(bytes),
        Some(&ESC) => (Some(Event::Key(KeyCode::Esc.into())), 2),
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
    if bytes.get(2) == Some(&b'M') {
        // An X10 mouse report: three bytes follow the M.
        return (None, bytes.len().min(6));
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
    let Some(&last) = bytes.get(seen) else {
        // Cut off by the end of the input: drop it.
        return (None, bytes.len());
    };
    if !(0x40..=0x7e).contains(&last) {
        // Not a final byte: drop what came before it.
        return (None, seen);
    }
    let used = seen + 1;
    // Parameter bytes are ASCII, so this never fails.
    let params = std::str::from_utf8(&body[..params_len]).unwrap_or_default();
    if params == "200" && last == b'~' {
        return paste(bytes, used);
    }
    (csi_key(params, inter_len > 0, last).map(Event::Key), used)
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
    let (modifiers, kind) = modifiers_and_kind(fields.next());
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
        b'~' => numbered_key(first)?,
        b'u' => return csi_u_key(first, modifiers, kind),
        // Cursor position (R), focus (I, O), mouse (M, m) and the rest.
        _ => return None,
    };
    Some(KeyEvent::new_with_kind(code, modifiers, kind))
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
fn csi_u_key(first: &str, modifiers: KeyModifiers, kind: KeyEventKind) -> Option<KeyEvent> {
    let codepoint: u32 = first.split(':').next()?.parse().ok()?;
    let code = match char::from_u32(codepoint)? {
        '\x1b' => KeyCode::Esc,
        '\r' | '\n' => KeyCode::Enter,
        '\t' if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
        '\t' => KeyCode::Tab,
        '\x7f' => KeyCode::Backspace,
        '\u{e000}'..='\u{f8ff}' => return None,
        c => KeyCode::Char(c),
    };
    Some(KeyEvent::new_with_kind(code, modifiers, kind))
}

/// The modifiers and event kind of a `<mask>[:<kind>]` field.
fn modifiers_and_kind(field: Option<&str>) -> (KeyModifiers, KeyEventKind) {
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
    (modifiers, kind)
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
                plain(KeyCode::Enter),
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
        assert_eq!(parse(b"\x1b\x1b"), vec![plain(KeyCode::Esc)]);
        assert_eq!(
            parse(b"\x1ba"),
            vec![key(KeyCode::Char('a'), KeyModifiers::ALT)]
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
            parse(b"\x1b[97;5u\x1b[13u\x1b[9;2u\x1b[57441u"),
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
