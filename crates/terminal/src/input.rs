//! The bytes a terminal sends to its program for keys, the mouse, pasted
//! text and focus, as xterm sends them and as VS Code does on the Mac.

use alacritty_terminal::term::TermMode;

/// The modifier keys held with a key or a click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
    pub command: bool,
}

impl Modifiers {
    /// The xterm parameter for keys with modifiers: 1 plus shift 1, alt 2,
    /// control 4.
    fn parameter(self) -> u8 {
        1 + self.shift as u8 + 2 * self.alt as u8 + 4 * self.control as u8
    }
}

/// A key pressed in a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key<'a> {
    /// A key that writes: its name, such as `a` or `space`, and the text the
    /// keyboard layout made of it, such as `A` or `å`.
    Character {
        key: &'a str,
        text: Option<&'a str>,
    },
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Function(u8),
}

/// The bytes for a key, or `None` when the key is not for the terminal
/// (Cmd+C, Cmd+T…), so the window can use it.
///
/// With `option_is_meta` false, as in VS Code, Option+letter writes the
/// letter macOS makes of it (`å`); with it true, it sends Esc and the
/// letter, as shells expect for Alt.
pub fn key_bytes(
    key: Key,
    modifiers: Modifiers,
    mode: TermMode,
    option_is_meta: bool,
) -> Option<Vec<u8>> {
    if modifiers.command {
        // VS Code's own Mac shortcuts: to the start and end of the line,
        // and delete the whole line.
        return match key {
            Key::Left if !modifiers.shift => Some(vec![0x01]),
            Key::Right if !modifiers.shift => Some(vec![0x05]),
            Key::Backspace => Some(vec![0x15]),
            _ => None,
        };
    }
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let parameter = modifiers.parameter();
    let cursor_key = |letter: u8| -> Vec<u8> {
        if parameter > 1 {
            format!("\x1b[1;{parameter}{}", letter as char).into_bytes()
        } else if app_cursor {
            vec![0x1b, b'O', letter]
        } else {
            vec![0x1b, b'[', letter]
        }
    };
    let tilde_key = |number: u8| -> Vec<u8> {
        if parameter > 1 {
            format!("\x1b[{number};{parameter}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        }
    };
    let only_alt = modifiers.alt && !modifiers.shift && !modifiers.control;
    let bytes = match key {
        Key::Enter if modifiers.shift || modifiers.alt => b"\x1b\r".to_vec(),
        Key::Enter => b"\r".to_vec(),
        Key::Tab if modifiers.shift => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Backspace if modifiers.control => vec![0x08],
        Key::Backspace if modifiers.alt => b"\x1b\x7f".to_vec(),
        Key::Backspace => vec![0x7f],
        Key::Escape if modifiers.alt => b"\x1b\x1b".to_vec(),
        Key::Escape => vec![0x1b],
        // Option+arrow moves by word in the Mac's terminals.
        Key::Left if only_alt => b"\x1bb".to_vec(),
        Key::Right if only_alt => b"\x1bf".to_vec(),
        Key::Up => cursor_key(b'A'),
        Key::Down => cursor_key(b'B'),
        Key::Right => cursor_key(b'C'),
        Key::Left => cursor_key(b'D'),
        Key::Home => cursor_key(b'H'),
        Key::End => cursor_key(b'F'),
        Key::Insert => tilde_key(2),
        Key::Delete => tilde_key(3),
        Key::PageUp => tilde_key(5),
        Key::PageDown => tilde_key(6),
        Key::Function(number @ 1..=4) => {
            let letter = b'P' + (number - 1);
            if parameter > 1 {
                format!("\x1b[1;{parameter}{}", letter as char).into_bytes()
            } else {
                vec![0x1b, b'O', letter]
            }
        }
        Key::Function(number @ 5..=12) => {
            tilde_key([15, 17, 18, 19, 20, 21, 23, 24][number as usize - 5])
        }
        Key::Function(_) => return None,
        Key::Character { key, text } => {
            return character_bytes(key, text, modifiers, option_is_meta);
        }
    };
    Some(bytes)
}

fn character_bytes(
    key: &str,
    text: Option<&str>,
    modifiers: Modifiers,
    option_is_meta: bool,
) -> Option<Vec<u8>> {
    let key = if key == "space" { " " } else { key };
    let mut chars = key.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(char), None) => Some(char),
        _ => None,
    };
    if modifiers.control {
        let code = match single? {
            char @ 'a'..='z' => char as u8 - b'a' + 1,
            ' ' | '@' | '2' => 0,
            '[' | '3' => 0x1b,
            '\\' | '4' => 0x1c,
            ']' | '5' => 0x1d,
            '^' | '6' => 0x1e,
            '_' | '-' | '/' | '7' => 0x1f,
            '?' | '8' => 0x7f,
            _ => return None,
        };
        return Some(if modifiers.alt && option_is_meta {
            vec![0x1b, code]
        } else {
            vec![code]
        });
    }
    if modifiers.alt && option_is_meta {
        let char = single?;
        let char = if modifiers.shift {
            char.to_ascii_uppercase()
        } else {
            char
        };
        let mut bytes = vec![0x1b];
        bytes.extend(char.to_string().into_bytes());
        return Some(bytes);
    }
    match text {
        Some(text) if !text.is_empty() => Some(text.as_bytes().to_vec()),
        _ => single.map(|char| char.to_string().into_bytes()),
    }
}

/// A mouse button, or a turn of the wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    /// The mouse moved, with the button still held, if any.
    Move,
}

/// The bytes that tell a program about the mouse, when it asked for it
/// (Claude Code, vim, htop…). `column` and `line` count from zero.
pub fn mouse_bytes(
    button: Option<MouseButton>,
    action: MouseAction,
    column: usize,
    line: usize,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !mode.intersects(TermMode::MOUSE_MODE) {
        return None;
    }
    if action == MouseAction::Move {
        let reported = mode.contains(TermMode::MOUSE_MOTION)
            || (mode.contains(TermMode::MOUSE_DRAG) && button.is_some());
        if !reported {
            return None;
        }
    }
    let mut code: u32 = match button {
        Some(MouseButton::Left) => 0,
        Some(MouseButton::Middle) => 1,
        Some(MouseButton::Right) => 2,
        Some(MouseButton::WheelUp) => 64,
        Some(MouseButton::WheelDown) => 65,
        None => 3,
    };
    if action == MouseAction::Move {
        code += 32;
    }
    code += 4 * modifiers.shift as u32 + 8 * modifiers.alt as u32 + 16 * modifiers.control as u32;
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{code};{};{}{end}", column + 1, line + 1).into_bytes());
    }
    if action == MouseAction::Release {
        // The old encoding cannot say which button went up.
        code = 3 + (code & !3 & !32 & !64);
    }
    let mut bytes = b"\x1b[M".to_vec();
    let utf8 = mode.contains(TermMode::UTF8_MOUSE);
    for value in [code, column as u32 + 1, line as u32 + 1] {
        let value = value + 32;
        if utf8 && value >= 128 {
            let char = char::from_u32(value)?;
            bytes.extend(char.to_string().into_bytes());
        } else if value <= 255 {
            bytes.push(value as u8);
        } else {
            return None;
        }
    }
    Some(bytes)
}

/// The bytes for pasted text. Programs that asked for bracketed paste
/// (zsh, Claude Code) get it in one piece, so a long text with new lines is
/// not run line by line.
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // A pasted Esc could end the bracket early and run what follows.
        let text = text.replace(['\x1b', '\x03'], "");
        let mut bytes = b"\x1b[200~".to_vec();
        bytes.extend(text.into_bytes());
        bytes.extend(b"\x1b[201~");
        bytes
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// What to send when the terminal gains or loses focus, for programs that
/// asked to know.
pub fn focus_bytes(focused: bool, mode: TermMode) -> Option<&'static [u8]> {
    if !mode.contains(TermMode::FOCUS_IN_OUT) {
        return None;
    }
    Some(if focused { b"\x1b[I" } else { b"\x1b[O" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> TermMode {
        TermMode::default()
    }

    fn key(key: Key, modifiers: Modifiers) -> Option<Vec<u8>> {
        key_bytes(key, modifiers, plain(), false)
    }

    fn character<'a>(key: &'a str, text: &'a str) -> Key<'a> {
        Key::Character {
            key,
            text: Some(text),
        }
    }

    const NONE: Modifiers = Modifiers {
        shift: false,
        alt: false,
        control: false,
        command: false,
    };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..NONE
    };
    const ALT: Modifiers = Modifiers { alt: true, ..NONE };
    const CONTROL: Modifiers = Modifiers {
        control: true,
        ..NONE
    };
    const COMMAND: Modifiers = Modifiers {
        command: true,
        ..NONE
    };

    #[test]
    fn letters_and_layout_text() {
        assert_eq!(key(character("a", "a"), NONE).unwrap(), b"a");
        assert_eq!(key(character("a", "A"), SHIFT).unwrap(), b"A");
        assert_eq!(key(character("space", " "), NONE).unwrap(), b" ");
        // Option+a on an Italian Mac writes å, as in VS Code.
        assert_eq!(key(character("a", "å"), ALT).unwrap(), "å".as_bytes());
        assert_eq!(
            key_bytes(character("b", "∫"), ALT, plain(), true).unwrap(),
            b"\x1bb"
        );
    }

    #[test]
    fn control_codes() {
        assert_eq!(key(character("c", "c"), CONTROL).unwrap(), [3]);
        assert_eq!(key(character("d", "d"), CONTROL).unwrap(), [4]);
        assert_eq!(key(character("v", "v"), CONTROL).unwrap(), [0x16]);
        assert_eq!(key(character("space", " "), CONTROL).unwrap(), [0]);
        assert_eq!(key(character("[", "["), CONTROL).unwrap(), [0x1b]);
    }

    #[test]
    fn special_keys() {
        assert_eq!(key(Key::Enter, NONE).unwrap(), b"\r");
        // Shift+Enter is a new line in Claude Code, as /terminal-setup does.
        assert_eq!(key(Key::Enter, SHIFT).unwrap(), b"\x1b\r");
        assert_eq!(key(Key::Tab, SHIFT).unwrap(), b"\x1b[Z");
        assert_eq!(key(Key::Backspace, NONE).unwrap(), [0x7f]);
        assert_eq!(key(Key::Backspace, ALT).unwrap(), b"\x1b\x7f");
        assert_eq!(key(Key::Up, NONE).unwrap(), b"\x1b[A");
        assert_eq!(key(Key::Up, SHIFT).unwrap(), b"\x1b[1;2A");
        assert_eq!(key(Key::Left, ALT).unwrap(), b"\x1bb");
        assert_eq!(key(Key::Delete, NONE).unwrap(), b"\x1b[3~");
        assert_eq!(key(Key::Function(1), NONE).unwrap(), b"\x1bOP");
        assert_eq!(key(Key::Function(5), NONE).unwrap(), b"\x1b[15~");
        assert_eq!(key(Key::Function(12), CONTROL).unwrap(), b"\x1b[24;5~");
        assert_eq!(
            key_bytes(Key::Up, NONE, TermMode::APP_CURSOR, false).unwrap(),
            b"\x1bOA"
        );
    }

    #[test]
    fn command_keys_are_the_windows_but_for_line_moves() {
        assert_eq!(key(Key::Left, COMMAND).unwrap(), [0x01]);
        assert_eq!(key(Key::Right, COMMAND).unwrap(), [0x05]);
        assert_eq!(key(Key::Backspace, COMMAND).unwrap(), [0x15]);
        assert_eq!(key(character("c", "c"), COMMAND), None);
    }

    #[test]
    fn mouse_reports_only_when_asked() {
        let click = |mode| {
            mouse_bytes(
                Some(MouseButton::Left),
                MouseAction::Press,
                4,
                2,
                NONE,
                mode,
            )
        };
        assert_eq!(click(plain()), None);
        let sgr = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(click(sgr).unwrap(), b"\x1b[<0;5;3M");
        assert_eq!(
            mouse_bytes(
                Some(MouseButton::WheelUp),
                MouseAction::Press,
                0,
                0,
                NONE,
                sgr
            )
            .unwrap(),
            b"\x1b[<64;1;1M"
        );
        assert_eq!(
            mouse_bytes(None, MouseAction::Move, 0, 0, NONE, sgr),
            None,
            "no motion without the motion mode"
        );
        assert_eq!(
            click(TermMode::MOUSE_REPORT_CLICK).unwrap(),
            [0x1b, b'[', b'M', 32, 37, 35]
        );
        assert_eq!(
            mouse_bytes(
                Some(MouseButton::Left),
                MouseAction::Release,
                4,
                2,
                NONE,
                TermMode::MOUSE_REPORT_CLICK
            )
            .unwrap(),
            [0x1b, b'[', b'M', 35, 37, 35]
        );
    }

    #[test]
    fn pasted_text() {
        assert_eq!(paste_bytes("a\nb", plain()), b"a\rb");
        assert_eq!(
            paste_bytes("a\nb\x1b[201~rm", TermMode::BRACKETED_PASTE),
            b"\x1b[200~a\nb[201~rm\x1b[201~"
        );
        assert_eq!(focus_bytes(true, plain()), None);
        assert_eq!(
            focus_bytes(false, TermMode::FOCUS_IN_OUT).unwrap(),
            b"\x1b[O"
        );
    }
}
