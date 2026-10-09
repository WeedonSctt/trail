//! Keys for the terminal panel: the chords Trail keeps for itself, and the
//! bytes every other keystroke becomes on its way to a shell.
//!
//! Two directions, kept in one place because they must agree. A chord in
//! `[keymap.terminal]` is parsed into a [`KeyChord`] and matched against
//! crossterm's `KeyEvent`; anything that does not match is translated by
//! [`encode`] into the byte sequence a terminal would have sent, which is what
//! a shell on the other side of a pseudo-terminal expects to read.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// One key plus the modifiers that must be held with it, as written in
/// `[keymap.terminal]` — `ctrl-.`, `f12`, `shift-pageup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

/// Explanation attached to a binding [`KeyChord::parse`] rejects.
pub const KEY_CHORD_REASON: &str = "expected a key such as f12, ctrl-., alt-x or shift-pageup";

impl KeyChord {
    /// Parses a binding: any of `ctrl-`, `alt-`, `shift-` in any order, then a
    /// single character or a key name (`enter`, `esc`, `tab`, `space`,
    /// `backspace`, arrows, `home`, `end`, `pageup`, `pagedown`, `insert`,
    /// `delete`, `f1`–`f12`).
    ///
    /// Returns `None` for anything else, including a bare modifier.
    ///
    /// ```
    /// use trail::terminal::keys::KeyChord;
    ///
    /// assert!(KeyChord::parse("ctrl-.").is_some());
    /// assert!(KeyChord::parse("F12").is_some());
    /// assert!(KeyChord::parse("ctrl-").is_none());
    /// ```
    #[must_use]
    pub fn parse(binding: &str) -> Option<Self> {
        let mut rest = binding.trim();
        let mut modifiers = KeyModifiers::NONE;
        loop {
            let lower = rest.to_ascii_lowercase();
            let (flag, len) = if lower.starts_with("ctrl-") {
                (KeyModifiers::CONTROL, 5)
            } else if lower.starts_with("alt-") {
                (KeyModifiers::ALT, 4)
            } else if lower.starts_with("shift-") {
                (KeyModifiers::SHIFT, 6)
            } else {
                break;
            };
            // `ctrl--` is Ctrl plus the minus key, not an empty key name.
            if rest.len() == len {
                return None;
            }
            modifiers |= flag;
            rest = &rest[len..];
        }

        let mut chars = rest.chars();
        let code = match (chars.next(), chars.next()) {
            (Some(ch), None) => KeyCode::Char(ch.to_ascii_lowercase()),
            _ => named_key(&rest.to_ascii_lowercase())?,
        };
        Some(Self { code, modifiers })
    }

    /// Whether `key` is this chord.
    ///
    /// Control and Alt must match exactly. Shift must match exactly on named
    /// keys, but is ignored on characters, because a character carries its own
    /// shift — crossterm reports `Shift`+`/` as `?` on one layout and as `_` on
    /// another, and a binding cannot know which.
    #[must_use]
    pub fn matches(&self, key: &KeyEvent) -> bool {
        let strict = KeyModifiers::CONTROL | KeyModifiers::ALT;
        match (self.code, key.code) {
            (KeyCode::Char(want), KeyCode::Char(got)) => {
                want == got.to_ascii_lowercase()
                    && (key.modifiers & strict) == (self.modifiers & strict)
            }
            (want, got) => {
                let mask = strict | KeyModifiers::SHIFT;
                want == normalise(got) && (key.modifiers & mask) == (self.modifiers & mask)
            }
        }
    }
}

/// Folds crossterm's alternative spellings of one key into a single code.
fn normalise(code: KeyCode) -> KeyCode {
    match code {
        // Some terminals report Shift+Tab as a distinct key.
        KeyCode::BackTab => KeyCode::Tab,
        other => other,
    }
}

fn named_key(name: &str) -> Option<KeyCode> {
    Some(match name {
        "enter" => KeyCode::Enter,
        "esc" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "space" => KeyCode::Char(' '),
        "backspace" => KeyCode::Backspace,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "insert" => KeyCode::Insert,
        "delete" => KeyCode::Delete,
        _ => {
            let number: u8 = name.strip_prefix('f')?.parse().ok()?;
            if !(1..=12).contains(&number) {
                return None;
            }
            KeyCode::F(number)
        }
    })
}

/// The bytes a terminal sends for `key`, or `None` for a key that has no
/// encoding (a bare modifier, a media key).
///
/// `application_cursor` is the shell's DECCKM state — full-screen programs such
/// as `vim` and `less` switch it on, and expect arrows as `ESC O A` rather than
/// `ESC [ A` while it is.
///
/// AltGr arrives on Windows as Ctrl+Alt, and is text: on the Latin American
/// layout it is how `\` and `@` are typed. Only Ctrl+Alt with an ASCII letter is
/// treated as the chord it spells.
#[must_use]
pub fn encode(key: &KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    let bytes = match key.code {
        KeyCode::Char(ch) => {
            let altgr = ctrl && alt && !ch.is_ascii_alphabetic();
            if altgr || !(ctrl || alt) {
                return Some(utf8(ch));
            }
            let mut out = Vec::new();
            if alt {
                out.push(0x1b);
            }
            if ctrl {
                out.push(control_byte(ch)?);
            } else {
                out.extend(utf8(ch));
            }
            return Some(out);
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab if shift => b"\x1b[Z".to_vec(),
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        // DEL, not BS: what every modern terminal sends, and what readline and
        // PSReadLine bind to "delete the previous character". Ctrl+Backspace
        // sends BS, which both bind to "delete the previous word".
        KeyCode::Backspace if ctrl => vec![0x08],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => cursor_key(b'A', key.modifiers, application_cursor),
        KeyCode::Down => cursor_key(b'B', key.modifiers, application_cursor),
        KeyCode::Right => cursor_key(b'C', key.modifiers, application_cursor),
        KeyCode::Left => cursor_key(b'D', key.modifiers, application_cursor),
        KeyCode::Home => cursor_key(b'H', key.modifiers, application_cursor),
        KeyCode::End => cursor_key(b'F', key.modifiers, application_cursor),
        KeyCode::Insert => tilde_key(2, key.modifiers),
        KeyCode::Delete => tilde_key(3, key.modifiers),
        KeyCode::PageUp => tilde_key(5, key.modifiers),
        KeyCode::PageDown => tilde_key(6, key.modifiers),
        KeyCode::F(n @ 1..=4) => {
            let letter = b"PQRS"[usize::from(n - 1)];
            match modifier_param(key.modifiers) {
                Some(m) => format!("\x1b[1;{m}{}", char::from(letter)).into_bytes(),
                None => vec![0x1b, b'O', letter],
            }
        }
        KeyCode::F(n @ 5..=12) => {
            let code = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)];
            tilde_key(code, key.modifiers)
        }
        _ => return None,
    };

    // Alt with a named key is an ESC prefix, as with a character — except where
    // the modifier is already spelled inside the sequence.
    if alt && matches!(key.code, KeyCode::Enter | KeyCode::Backspace | KeyCode::Esc) {
        let mut out = vec![0x1b];
        out.extend(bytes);
        return Some(out);
    }
    Some(bytes)
}

fn utf8(ch: char) -> Vec<u8> {
    let mut buf = [0u8; 4];
    ch.encode_utf8(&mut buf).as_bytes().to_vec()
}

/// The C0 byte Ctrl turns `ch` into, as a VT100 keyboard produced it.
fn control_byte(ch: char) -> Option<u8> {
    Some(match ch.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        ' ' | '@' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '7' | '/' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// xterm's modifier parameter: 1 + Shift(1) + Alt(2) + Ctrl(4), or `None` when
/// no modifier is held and the short form applies.
fn modifier_param(modifiers: KeyModifiers) -> Option<u8> {
    let mut param = 1;
    if modifiers.contains(KeyModifiers::SHIFT) {
        param += 1;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        param += 2;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        param += 4;
    }
    (param > 1).then_some(param)
}

fn cursor_key(letter: u8, modifiers: KeyModifiers, application_cursor: bool) -> Vec<u8> {
    match modifier_param(modifiers) {
        Some(m) => format!("\x1b[1;{m}{}", char::from(letter)).into_bytes(),
        None if application_cursor => vec![0x1b, b'O', letter],
        None => vec![0x1b, b'[', letter],
    }
}

fn tilde_key(code: u8, modifiers: KeyModifiers) -> Vec<u8> {
    match modifier_param(modifiers) {
        Some(m) => format!("\x1b[{code};{m}~").into_bytes(),
        None => format!("\x1b[{code}~").into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn plain(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::NONE)
    }

    #[test]
    fn the_default_chords_parse_and_match() {
        let toggle = KeyChord::parse("ctrl-.").unwrap();
        assert!(toggle.matches(&key(KeyCode::Char('.'), KeyModifiers::CONTROL)));
        // The whole reason the modifier is checked: a plain `.` toggles hidden
        // files in the listing and must never reach the panel's toggle.
        assert!(!toggle.matches(&plain(KeyCode::Char('.'))));

        let focus = KeyChord::parse("f12").unwrap();
        assert!(focus.matches(&plain(KeyCode::F(12))));
        assert!(!focus.matches(&key(KeyCode::F(12), KeyModifiers::SHIFT)));

        let next = KeyChord::parse("ctrl-pagedown").unwrap();
        assert!(next.matches(&key(KeyCode::PageDown, KeyModifiers::CONTROL)));
        assert!(!next.matches(&plain(KeyCode::PageDown)));
    }

    #[test]
    fn modifiers_may_come_in_any_order_and_case() {
        assert_eq!(
            KeyChord::parse("Shift-Ctrl-PageUp"),
            KeyChord::parse("ctrl-shift-pageup")
        );
        assert_eq!(
            KeyChord::parse("ctrl--"),
            Some(KeyChord {
                code: KeyCode::Char('-'),
                modifiers: KeyModifiers::CONTROL
            })
        );
    }

    #[test]
    fn nonsense_is_rejected() {
        for bad in ["", "ctrl-", "f13", "f0", "hyper-x", "pagedwn"] {
            assert_eq!(KeyChord::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_character_chord_ignores_the_shift_its_layout_needed() {
        let chord = KeyChord::parse("ctrl-?").unwrap();
        assert!(chord.matches(&key(
            KeyCode::Char('?'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        )));
    }

    #[test]
    fn text_is_sent_as_utf8() {
        assert_eq!(encode(&plain(KeyCode::Char('a')), false).unwrap(), b"a");
        assert_eq!(
            encode(&key(KeyCode::Char('Q'), KeyModifiers::SHIFT), false).unwrap(),
            b"Q"
        );
        assert_eq!(
            encode(&plain(KeyCode::Char('ñ')), false).unwrap(),
            "ñ".as_bytes()
        );
    }

    /// The v1.9.2 bug must not come back through the panel: Windows reports
    /// AltGr as Ctrl+Alt, and on es-MX that is how `\` is typed.
    #[test]
    fn altgr_characters_are_text() {
        let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
        assert_eq!(
            encode(&key(KeyCode::Char('\\'), altgr), false).unwrap(),
            b"\\"
        );
        assert_eq!(
            encode(&key(KeyCode::Char('@'), altgr), false).unwrap(),
            b"@"
        );
        // Ctrl+Alt with a letter is a real chord: ESC then the control byte.
        assert_eq!(
            encode(&key(KeyCode::Char('a'), altgr), false).unwrap(),
            [0x1b, 0x01]
        );
    }

    #[test]
    fn control_and_alt_chords() {
        assert_eq!(
            encode(&key(KeyCode::Char('c'), KeyModifiers::CONTROL), false).unwrap(),
            [0x03]
        );
        assert_eq!(
            encode(&key(KeyCode::Char('D'), KeyModifiers::CONTROL), false).unwrap(),
            [0x04]
        );
        assert_eq!(
            encode(&key(KeyCode::Char(' '), KeyModifiers::CONTROL), false).unwrap(),
            [0x00]
        );
        assert_eq!(
            encode(&key(KeyCode::Char('.'), KeyModifiers::ALT), false).unwrap(),
            b"\x1b."
        );
        assert_eq!(
            encode(&key(KeyCode::Char('b'), KeyModifiers::ALT), false).unwrap(),
            b"\x1bb"
        );
        assert_eq!(
            encode(&key(KeyCode::Char('.'), KeyModifiers::CONTROL), false),
            None
        );
    }

    #[test]
    fn named_keys() {
        assert_eq!(encode(&plain(KeyCode::Enter), false).unwrap(), b"\r");
        assert_eq!(encode(&plain(KeyCode::Backspace), false).unwrap(), [0x7f]);
        assert_eq!(
            encode(&key(KeyCode::Backspace, KeyModifiers::CONTROL), false).unwrap(),
            [0x08]
        );
        assert_eq!(encode(&plain(KeyCode::Tab), false).unwrap(), b"\t");
        assert_eq!(encode(&plain(KeyCode::BackTab), false).unwrap(), b"\x1b[Z");
        assert_eq!(encode(&plain(KeyCode::Delete), false).unwrap(), b"\x1b[3~");
        assert_eq!(encode(&plain(KeyCode::PageUp), false).unwrap(), b"\x1b[5~");
        assert_eq!(encode(&plain(KeyCode::F(1)), false).unwrap(), b"\x1bOP");
        assert_eq!(encode(&plain(KeyCode::F(5)), false).unwrap(), b"\x1b[15~");
        assert_eq!(encode(&plain(KeyCode::F(12)), false).unwrap(), b"\x1b[24~");
        assert_eq!(
            encode(&key(KeyCode::Enter, KeyModifiers::ALT), false).unwrap(),
            b"\x1b\r"
        );
    }

    #[test]
    fn arrows_follow_the_cursor_key_mode_and_carry_modifiers() {
        assert_eq!(encode(&plain(KeyCode::Up), false).unwrap(), b"\x1b[A");
        assert_eq!(encode(&plain(KeyCode::Up), true).unwrap(), b"\x1bOA");
        assert_eq!(
            encode(&key(KeyCode::Left, KeyModifiers::CONTROL), true).unwrap(),
            b"\x1b[1;5D"
        );
        assert_eq!(
            encode(&key(KeyCode::Right, KeyModifiers::SHIFT), false).unwrap(),
            b"\x1b[1;2C"
        );
        assert_eq!(
            encode(&key(KeyCode::Delete, KeyModifiers::CONTROL), false).unwrap(),
            b"\x1b[3;5~"
        );
    }
}
