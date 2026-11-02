use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

/// Control byte for `ctrl+<c>`, accepting the alternate digits that legacy
/// terminals report (e.g. ctrl+] arrives as ctrl+5).
pub fn ctrl_byte(c: char) -> Option<u8> {
    Some(match c.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' | '/' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// Parses a prefix spec such as "ctrl-]" into its control byte.
pub fn parse_prefix(spec: &str) -> Option<u8> {
    let c = spec.strip_prefix("ctrl-")?.chars().next()?;
    ctrl_byte(c)
}

pub fn is_ctrl_byte(key: &KeyEvent, byte: u8) -> bool {
    match key.code {
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ctrl_byte(c) == Some(byte)
        }
        _ => false,
    }
}

/// Encodes a key press as the bytes an xterm would send to the program.
pub fn encode(key: &KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let m = key.modifiers;
    let shift = m.contains(KeyModifiers::SHIFT);
    let alt = m.contains(KeyModifiers::ALT);
    let ctrl = m.contains(KeyModifiers::CONTROL);
    // xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl.
    let modp = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;

    let cursor = |c: char| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[1;{modp}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[{n};{modp}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };

    let (bytes, alt_prefix) = match key.code {
        KeyCode::Char(c) if ctrl => (vec![ctrl_byte(c)?], alt),
        KeyCode::Char(c) => (c.to_string().into_bytes(), alt),
        // Shift+Enter → meta+Enter, which Claude Code treats as a newline.
        KeyCode::Enter => (b"\r".to_vec(), alt || shift),
        KeyCode::Backspace => (vec![if ctrl { 0x08 } else { 0x7f }], alt),
        KeyCode::Tab => (b"\t".to_vec(), alt),
        KeyCode::BackTab => (b"\x1b[Z".to_vec(), false),
        KeyCode::Esc => (vec![0x1b], alt),
        KeyCode::Up => (cursor('A'), false),
        KeyCode::Down => (cursor('B'), false),
        KeyCode::Right => (cursor('C'), false),
        KeyCode::Left => (cursor('D'), false),
        KeyCode::Home => (cursor('H'), false),
        KeyCode::End => (cursor('F'), false),
        KeyCode::Insert => (tilde(2), false),
        KeyCode::Delete => (tilde(3), false),
        KeyCode::PageUp => (tilde(5), false),
        KeyCode::PageDown => (tilde(6), false),
        KeyCode::F(n @ 1..=4) => (format!("\x1bO{}", (b'P' + n - 1) as char).into_bytes(), false),
        KeyCode::F(n) => {
            let code = match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                _ => return None,
            };
            (tilde(code), false)
        }
        _ => return None,
    };

    if alt_prefix {
        let mut out = vec![0x1b];
        out.extend(bytes);
        Some(out)
    } else {
        Some(bytes)
    }
}

/// Encodes a mouse event for a program that enabled mouse reporting.
/// `col`/`row` are 0-based and relative to the pane. Returns `None` when the
/// program didn't ask for this kind of event.
pub fn encode_mouse(
    m: &MouseEvent,
    col: u16,
    row: u16,
    mode: MouseProtocolMode,
    enc: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    use MouseProtocolMode as M;
    let code = |b: MouseButton| match b {
        MouseButton::Left => 0u16,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let motion = matches!(mode, M::ButtonMotion | M::AnyMotion);
    let (mut button, release) = match m.kind {
        _ if mode == M::None => return None,
        MouseEventKind::Down(b) => (code(b), false),
        MouseEventKind::Up(b) if mode != M::Press => (code(b), true),
        MouseEventKind::Drag(b) if motion => (code(b) + 32, false),
        MouseEventKind::Moved if mode == M::AnyMotion => (3 + 32, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        _ => return None,
    };
    if m.modifiers.contains(KeyModifiers::SHIFT) {
        button += 4;
    }
    if m.modifiers.contains(KeyModifiers::ALT) {
        button += 8;
    }
    if m.modifiers.contains(KeyModifiers::CONTROL) {
        button += 16;
    }
    let (x, y) = (col + 1, row + 1);
    match enc {
        MouseProtocolEncoding::Sgr => {
            let end = if release { 'm' } else { 'M' };
            Some(format!("\x1b[<{button};{x};{y}{end}").into_bytes())
        }
        _ => {
            // Legacy X10 encoding: release has no button, coords cap at 222.
            if release {
                button = 3;
            }
            let x = u8::try_from(x + 32).ok()?;
            let y = u8::try_from(y + 32).ok()?;
            Some(vec![0x1b, b'[', b'M', (button + 32) as u8, x, y])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn encodes_common_keys() {
        let none = KeyModifiers::NONE;
        assert_eq!(encode(&key(KeyCode::Char('a'), none), false).unwrap(), b"a");
        assert_eq!(
            encode(&key(KeyCode::Char('c'), KeyModifiers::CONTROL), false).unwrap(),
            [3]
        );
        assert_eq!(encode(&key(KeyCode::Up, none), false).unwrap(), b"\x1b[A");
        assert_eq!(encode(&key(KeyCode::Up, none), true).unwrap(), b"\x1bOA");
        assert_eq!(
            encode(&key(KeyCode::Left, KeyModifiers::ALT), false).unwrap(),
            b"\x1b[1;3D"
        );
        assert_eq!(
            encode(&key(KeyCode::Enter, KeyModifiers::SHIFT), false).unwrap(),
            b"\x1b\r"
        );
        assert_eq!(encode(&key(KeyCode::Char('é'), none), false).unwrap(), "é".as_bytes());
    }

    #[test]
    fn encodes_mouse_only_when_requested() {
        let m = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        let sgr = MouseProtocolEncoding::Sgr;
        assert_eq!(encode_mouse(&m, 4, 2, MouseProtocolMode::None, sgr), None);
        assert_eq!(
            encode_mouse(&m, 4, 2, MouseProtocolMode::PressRelease, sgr).unwrap(),
            b"\x1b[<0;5;3M"
        );
        assert_eq!(
            encode_mouse(&m, 4, 2, MouseProtocolMode::Press, MouseProtocolEncoding::Default)
                .unwrap(),
            [0x1b, b'[', b'M', 32, 37, 35]
        );
    }

    #[test]
    fn prefix_matches_legacy_digit_form() {
        let b = parse_prefix("ctrl-]").unwrap();
        assert!(is_ctrl_byte(&key(KeyCode::Char(']'), KeyModifiers::CONTROL), b));
        assert!(is_ctrl_byte(&key(KeyCode::Char('5'), KeyModifiers::CONTROL), b));
        assert!(!is_ctrl_byte(&key(KeyCode::Char(']'), KeyModifiers::NONE), b));
    }
}
