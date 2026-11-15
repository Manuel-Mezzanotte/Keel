//! Turns VS Code key bindings, such as `cmd+shift+i`, into the keystroke
//! notation of GPUI, such as `cmd-shift-i`.

/// The GPUI keystrokes for a VS Code binding on macOS. The `mac` variant wins
/// when the extension gives one. Chords become keystrokes separated by a
/// space. Returns `None` for keys Keel cannot bind.
pub fn to_gpui(key: &str, mac: Option<&str>) -> Option<String> {
    let binding = mac.filter(|mac| !mac.trim().is_empty()).unwrap_or(key);
    let mut keystrokes = Vec::new();
    for chord in binding.split_whitespace() {
        keystrokes.push(keystroke(chord)?);
    }
    if keystrokes.is_empty() {
        None
    } else {
        Some(keystrokes.join(" "))
    }
}

fn keystroke(chord: &str) -> Option<String> {
    let lower = chord.to_ascii_lowercase();
    // A trailing "+" is the plus key, as in "cmd++".
    let (modifiers_part, key) = match lower.strip_suffix("++") {
        Some(rest) => (rest, "+".to_owned()),
        None => match lower.rsplit_once('+') {
            Some((modifiers, key)) => (modifiers, key.to_owned()),
            None => ("", lower.clone()),
        },
    };
    let mut modifiers = Vec::new();
    for modifier in modifiers_part.split('+').filter(|part| !part.is_empty()) {
        modifiers.push(match modifier {
            "cmd" | "meta" | "win" => "cmd",
            "ctrl" => "ctrl",
            "alt" | "option" => "alt",
            "shift" => "shift",
            _ => return None,
        });
    }
    // GPUI writes modifiers in a fixed order.
    let order = ["ctrl", "alt", "shift", "cmd"];
    modifiers.sort_by_key(|modifier| order.iter().position(|known| known == modifier));
    modifiers.dedup();
    let key = gpui_key(&key)?;
    modifiers.push(key);
    Some(modifiers.join("-"))
}

fn gpui_key(key: &str) -> Option<&str> {
    Some(match key {
        "enter" => "enter",
        "tab" => "tab",
        "space" => "space",
        "backspace" => "backspace",
        "delete" => "delete",
        "escape" => "escape",
        "left" => "left",
        "right" => "right",
        "up" => "up",
        "down" => "down",
        "home" => "home",
        "end" => "end",
        "pageup" => "pageup",
        "pagedown" => "pagedown",
        "insert" => "insert",
        _ if key.chars().count() == 1 => key,
        _ if key.starts_with('f')
            && key[1..]
                .parse::<u8>()
                .is_ok_and(|number| (1..=24).contains(&number)) =>
        {
            key
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_bindings_win() {
        assert_eq!(to_gpui("ctrl+i", Some("cmd+i")).as_deref(), Some("cmd-i"));
        assert_eq!(
            to_gpui("ctrl+shift+i", Some("cmd+shift+i")).as_deref(),
            Some("shift-cmd-i")
        );
        assert_eq!(to_gpui("ctrl+right", None).as_deref(), Some("ctrl-right"));
    }

    #[test]
    fn plain_keys_and_names() {
        assert_eq!(to_gpui("tab", None).as_deref(), Some("tab"));
        assert_eq!(to_gpui("shift+enter", None).as_deref(), Some("shift-enter"));
        assert_eq!(to_gpui("Escape", None).as_deref(), Some("escape"));
        assert_eq!(to_gpui("cmd+/", None).as_deref(), Some("cmd-/"));
        assert_eq!(to_gpui("cmd++", None).as_deref(), Some("cmd-+"));
        assert_eq!(to_gpui("f5", None).as_deref(), Some("f5"));
    }

    #[test]
    fn chords_become_sequences() {
        assert_eq!(to_gpui("cmd+k cmd+c", None).as_deref(), Some("cmd-k cmd-c"));
    }

    #[test]
    fn unknown_keys_are_skipped() {
        assert_eq!(to_gpui("numpad_add", None), None);
        assert_eq!(to_gpui("hyper+a", None), None);
        assert_eq!(to_gpui("", None), None);
    }
}
